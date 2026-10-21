pub mod il2;
pub mod px;
pub mod remote;
pub mod sdk;
pub mod undo;
pub mod vis;

use crate::features::aa::AntiAim;
use crate::features::aim::Aim;
use crate::features::hits::{self, Hit};
use crate::features::magic::Magic;
use crate::features::melee::{Aura, Fists};
use crate::features::mods::{self, Mods};
use crate::features::rage::{self, Trigger};
use crate::features::silent::{self, Decoy, Silent};
use crate::features::tracers::{self, Pool, Trail};
use crate::game::il2::Il2;
use crate::game::px::Scene;
use crate::game::sdk::{HIT_WANT, HitDefs, Sdk, WANT, nat};
use crate::game::vis::Bvh;
use crate::os::mem::{self, Mem};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::{Duration, Instant};

pub const BONES: [&str; 19] = [
    "B_Pelvis",
    "B_Spine",
    "B_Spine1",
    "B_Spine2",
    "B_Neck",
    "B_Head",
    "B_Head_end",
    "B_L_UpperArm",
    "B_L_Forearm",
    "B_L_Hand",
    "B_R_UpperArm",
    "B_R_Forearm",
    "B_R_Hand",
    "B_L_Thigh",
    "B_L_Calf",
    "B_L_Foot",
    "B_R_Thigh",
    "B_R_Calf",
    "B_R_Foot",
];
pub const PELVIS: usize = 0;
pub const STOMACH: usize = 2;
pub const CHEST: usize = 3;
pub const NECK: usize = 4;
pub const HEAD: usize = 5;

pub const VIS: [usize; 5] = [HEAD, NECK, CHEST, STOMACH, PELVIS];
pub const LINKS: [(usize, usize); 18] = [(0, 1), (1, 2), (2, 3), (3, 4), (4, 5), (5, 6), (3, 7), (7, 8), (8, 9), (3, 10), (10, 11), (11, 12), (0, 13), (13, 14), (14, 15), (0, 16), (16, 17), (17, 18)];

pub type V3 = [f32; 3];

#[derive(Clone, Default)]
pub struct Ent {
    pub id: u64,

    pub team: bool,
    pub name: String,
    pub npc: bool,
    pub bones: [Option<V3>; 19],
    pub root: V3,
    pub dist: f32,

    pub vis: u32,
    pub dead: bool,
    pub vel: V3,
}

impl Ent {
    pub fn seen(&self, bone: usize) -> bool {
        self.vis & 1 << bone != 0
    }
}

#[derive(Clone, Copy, Default)]
pub struct Ballistic {
    pub speed: f32,
    pub drag: f32,
    pub grav: f32,
    pub inherit: f32,
}

#[derive(Clone, Copy, Default)]
pub struct LookNow {
    pub yaw_t: f32,
    pub yaw_a: f32,
    pub pitch_t: f32,
    pub pitch_a: f32,
    pub sens: f32,
}

#[derive(Clone)]
pub struct Snap {
    pub at: Instant,
    pub pid: i32,
    pub ok: bool,
    pub status: String,
    pub vp: [f32; 16],
    pub view: [f32; 16],
    pub fy: f32,
    pub cam: V3,
    pub cam_vel: V3,
    pub ball: Option<Ballistic>,
    pub look: Option<LookNow>,
    pub vis_tris: usize,
    pub ents: Vec<Ent>,

    pub aim_cam: u64,

    pub local: u64,
    pub rate: f32,
}

impl Default for Snap {
    fn default() -> Snap {
        Snap {
            at: Instant::now(),
            pid: 0,
            ok: false,
            status: String::new(),
            vp: [0.0; 16],
            view: [0.0; 16],
            fy: 1.0,
            cam: [0.0; 3],
            cam_vel: [0.0; 3],
            ball: None,
            look: None,
            vis_tris: 0,
            ents: vec![],
            aim_cam: 0,
            local: 0,
            rate: 0.0,
        }
    }
}

#[derive(Default)]
pub struct Shared {
    snaps: Mutex<VecDeque<Arc<Snap>>>,
    hits: Mutex<VecDeque<Hit>>,
    trails: Mutex<Arc<Vec<Trail>>>,

    pub tracers: AtomicBool,
}

impl Shared {
    fn publish(&self, s: Snap) {
        let mut q = self.snaps.lock().unwrap();
        q.push_back(Arc::new(s));
        while q.len() > 256 || q.front().is_some_and(|s| s.at.elapsed() > Duration::from_millis(250)) {
            q.pop_front();
        }
    }

    pub fn trails(&self) -> Arc<Vec<Trail>> {
        self.trails.lock().unwrap().clone()
    }

    fn hit(&self, h: Hit) {
        let mut q = self.hits.lock().unwrap();
        q.push_back(h);
        while q.len() > 16 {
            q.pop_front();
        }
    }

    pub fn hits(&self, age: Duration) -> Vec<Hit> {
        self.hits.lock().unwrap().iter().filter(|h| h.at.elapsed() < age).cloned().collect()
    }

    fn say(&self, pid: i32, s: impl Into<String>) {
        self.publish(Snap { pid, status: s.into(), ..Default::default() });
    }

    pub fn pick(&self, delay: Duration) -> Arc<Snap> {
        let q = self.snaps.lock().unwrap();
        let want = Instant::now().checked_sub(delay).unwrap_or_else(Instant::now);
        q.iter().rev().find(|s| s.at <= want).or(q.front()).cloned().unwrap_or_default()
    }
}

fn net_lead(w: &World, cfg: &crate::features::aim::Cfg) -> f32 {
    let lead = if w.host {
        0.0
    } else if cfg.magic() {
        w.net_delay + cfg.predict
    } else {
        cfg.predict
    };
    lead.max(0.0)
}

pub fn spawn(sh: Arc<Shared>, mods: Arc<Mods>, aim: Arc<Aim>) {
    std::thread::Builder::new().name("reader".into()).spawn(move || run(sh, mods, aim)).unwrap();
}

fn run(sh: Arc<Shared>, mods: Arc<Mods>, aim: Arc<Aim>) {
    loop {
        let Some(pid) = mem::find_pid() else {
            sh.say(0, "waiting for GoreBox");
            sleep(Duration::from_secs(1));
            continue;
        };
        let m = match Mem::open(pid) {
            Ok(m) => m,
            Err(e) => {
                sh.say(pid, mem::open_error(pid, &e));
                sleep(Duration::from_secs(2));
                continue;
            }
        };
        sh.say(pid, "scanning il2cpp classes");
        let il = Il2::scan(&m, WANT);
        let sdk = match Sdk::resolve(&m, &il) {
            Ok(s) => s,
            Err(e) => {
                sh.say(pid, format!("not ready: {e}"));
                sleep(Duration::from_secs(3));
                continue;
            }
        };
        let (m, sdk) = (Arc::new(m), Arc::new(sdk));
        let world = Arc::new(Mutex::new(Arc::new(World::default())));
        let stop = Arc::new(AtomicBool::new(false));
        let slow = {
            let (m, sdk, world, stop, aim, sh) = (m.clone(), sdk.clone(), world.clone(), stop.clone(), aim.clone(), sh.clone());
            std::thread::Builder::new()
                .name("reader-slow".into())
                .spawn(move || {
                    let mut s = Slow::new();
                    while !stop.load(Ordering::Relaxed) && m.alive() {
                        let mut w = s.tick(&m, &sdk);
                        w.decoy = s.decoy(&m, &sdk, &aim, w.local, w.cam_off);
                        let aura = aim.cfg.lock().unwrap().aura;
                        (w.fists, w.fists_out) = s.fists(&m, &sdk, w.local, aura);
                        let bullets = sh.tracers.load(Ordering::Relaxed) || aim.cfg.lock().unwrap().effective().magic();
                        w.pool = s.pool(&m, bullets && w.local != 0);
                        *world.lock().unwrap() = Arc::new(w);
                        sleep(Duration::from_millis(100));
                    }
                })
                .unwrap()
        };

        let writer = {
            let (m, world, stop, mods, aim, sh, k_pns) = (m.clone(), world.clone(), stop.clone(), mods.clone(), aim.clone(), sh.clone(), sdk.k_pns);
            std::thread::Builder::new()
                .name("mods".into())
                .spawn(move || {
                    let mut wr = mods::Writer::default();
                    let mut si = Silent::default();
                    let mut aura = Aura::default();
                    let mut trig = Trigger::default();
                    let mut aa = AntiAim::default();
                    let mut mg = Magic::default();
                    let mut held = Instant::now() - Duration::from_secs(1);

                    let mut logged = Instant::now();
                    while !stop.load(Ordering::Relaxed) && m.alive() {
                        let w = world.lock().unwrap().clone();
                        let quit = mods.quitting();
                        let cfg = aim.cfg.lock().unwrap().effective();
                        if quit {
                            aa.off(&m);
                            trig.lift();
                            aura.release(&m);
                            si.restore(&m);
                        } else {
                            let cam = w.mods_off.map_or(0, |o| o.gun_cam);
                            let snap = sh.pick(Duration::ZERO);

                            let ingame = !aim.menu.load(Ordering::Relaxed) && aim.focused.load(Ordering::Relaxed);
                            let punching = aura.tick(&m, &snap, &cfg, w.fists, w.fists_out && w.live.gun == 0, w.decoy, ingame, w.bvh.as_deref());
                            let (t, bone) = if punching != 0 {
                                si.release(&m);
                                (punching, HEAD)
                            } else {
                                si.tick(&m, &snap, &cfg, w.live.gun, cam, w.decoy, silent::Reach { limit: w.fire_reach, host: w.host, bvh: w.bvh.as_deref(), delay: net_lead(&w, &cfg) })
                            };
                            aa.tick(&m, k_pns, &snap, &cfg.aa);

                            trig.tick(cfg.rage && ingame && punching == 0 && t != 0 && si.sure, w.trigger.filter(|_| mem::ok(w.live.gun)));
                            let magic = match (&w.pool, cfg.magic()) {
                                (Some(p), true) => mg.tick(&m, p, &snap, &cfg),
                                _ => 0,
                            };
                            let mut st = aim.state.lock().unwrap();
                            (st.silent, st.silent_bone) = (t, bone);
                            if magic != 0 {
                                st.magic = magic;
                            }
                            st.moved = mg.moved;
                            st.rage_shots = trig.shots;
                            st.aa = aa.status.clone();
                            (st.reach, st.host) = (silent::reach(&snap, w.fire_reach), w.host);
                            (st.ping, st.lead_ms) = (w.ping, net_lead(&w, &cfg) * 1000.0);
                        }
                        if held.elapsed() >= Duration::from_millis(20) {
                            held = Instant::now();

                            let since = logged;
                            for h in sh.hits(Duration::from_secs(2)).into_iter().filter(|h| h.at > since) {
                                logged = h.at;
                                if aura.setup.as_ref().is_some_and(|l| l.name == h.name) {
                                    eprintln!("punch: {:<15} {} | {}", hits::part(h.part), h.name, aura.report(&m, w.fists));
                                }
                            }

                            let mc = *mods.cfg.lock().unwrap();
                            let mc = if cfg.rage { mods::Cfg { no_spread: true, no_recoil: true, ..mc } } else { mc };
                            wr.tick(&m, mc, w.mods_off, w.live, quit);
                        }
                        mods.set_clean(wr.is_clean() && si.idle() && aura.idle() && trig.idle() && aa.idle());
                        sleep(Duration::from_millis(1));
                    }
                })
                .unwrap()
        };
        let mut fast = Fast::default();
        let (mut tracker, mut traced) = (tracers::Tracker::default(), Instant::now());
        let (mut n, mut t0, mut rate) = (0u32, Instant::now(), 0.0);
        while m.alive() {
            let w = world.lock().unwrap().clone();
            let mut s = fast.tick(&m, &sdk, &w);
            n += 1;
            if t0.elapsed() >= Duration::from_secs(1) {
                rate = n as f32 / t0.elapsed().as_secs_f32();
                (n, t0) = (0, Instant::now());
            }
            (s.pid, s.rate) = (pid, rate);
            for h in fast.hits.drain(..) {
                sh.hit(h);
            }
            if traced.elapsed() >= Duration::from_millis(4) {
                traced = Instant::now();
                let t = match &w.pool {
                    Some(p) if s.ok => tracker.tick(&m, p, s.cam),
                    _ => vec![],
                };
                *sh.trails.lock().unwrap() = Arc::new(t);
            }
            sh.publish(s);
            sleep(Duration::from_micros(800));
        }
        stop.store(true, Ordering::Relaxed);
        let _ = slow.join();
        let _ = writer.join();
    }
}

pub fn targets(m: &Mem) -> Result<Vec<(u64, String, bool)>, String> {
    let sdk = Sdk::resolve(m, &Il2::scan(m, WANT))?;
    Ok(Slow::new().tick(m, &sdk).targets.into_iter().map(|t| (t.ctrl, t.name, t.npc)).collect())
}

pub fn local(m: &Mem) -> Result<(u64, u64, u64), String> {
    let sdk = Sdk::resolve(m, &Il2::scan(m, WANT))?;
    let w = Slow::new().tick(m, &sdk);
    let item = m.q(w.local + sdk.pc_item);
    Ok((w.local, item, if w.cam_off != 0 { m.q(item + w.cam_off) } else { 0 }))
}

pub fn ballistics(m: &Mem) -> Result<Option<Ballistic>, String> {
    let sdk = Sdk::resolve(m, &Il2::scan(m, WANT))?;
    Ok(Slow::new().tick(m, &sdk).ball)
}

pub fn aa_report() {
    let Some(m) = mem::find_pid().and_then(|p| Mem::open(p).ok()) else { return eprintln!("GoreBox not running") };
    let sdk = match Sdk::resolve(&m, &Il2::scan(&m, WANT)) {
        Ok(s) => s,
        Err(e) => return eprintln!("not ready: {e}"),
    };
    if std::env::args().any(|a| a == "dry") {
        return println!("{:?}", crate::features::aa::dry(&m, sdk.k_pns));
    }
    let w = Slow::new().tick(&m, &sdk);
    if w.local_pns == 0 {
        return eprintln!("not in a game");
    }

    let snap_bytes = |m: &Mem| {
        let arr = m.q(w.local_pns + 0x250);
        m.bytes(arr + 0x20, 0x12).map(|b| (b[4], b[0x10], b[0x11]))
    };
    let mut aa = AntiAim::default();
    let snap = Snap { view: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0], ..Default::default() };
    let t = Instant::now();
    let on = crate::features::aa::Cfg { pitch: crate::features::aa::Pitch::Down, yaw: crate::features::aa::Yaw::Spin, spin: 360.0 };
    for (until, cfg) in [(1.0, crate::features::aa::Cfg::default()), (4.0, on), (5.0, crate::features::aa::Cfg::default())] {
        while t.elapsed().as_secs_f32() < until {
            aa.tick(&m, sdk.k_pns, &snap, &cfg);
            sleep(Duration::from_millis(250));
            println!("{:4.1}s {:<34} head pitch, aim yaw, aim pitch bytes {:?}", t.elapsed().as_secs_f32(), if aa.status.is_empty() { "off" } else { &aa.status }, snap_bytes(&m));
        }
    }
    aa.off(&m);
    println!("off, game code restored: {}", aa.idle());
}

pub fn fists_report(secs: u64) {
    let Some(m) = mem::find_pid().and_then(|p| Mem::open(p).ok()) else { return eprintln!("GoreBox not running") };
    let sdk = match Sdk::resolve(&m, &Il2::scan(&m, WANT)) {
        Ok(s) => s,
        Err(e) => return eprintln!("not ready: {e}"),
    };
    let mut s = Slow::new();
    let t = Instant::now();
    let mut last = None;
    println!("watching for {secs}s, switch between fists and a gun");
    while t.elapsed() < Duration::from_secs(secs) {
        let w = s.tick(&m, &sdk);
        let (f, out) = s.fists(&m, &sdk, w.local, true);
        let now = (f.map(|f| f.obj), out, w.live.gun != 0);
        if last != Some(now) {
            println!("{:6.1}s fists {:x?} in hand {} gun {}", t.elapsed().as_secs_f32(), now.0, now.1, now.2);
            last = Some(now);
        }
        sleep(Duration::from_millis(100));
    }
}

pub fn vis_report() {
    let Some(m) = mem::find_pid().and_then(|p| Mem::open(p).ok()) else { return eprintln!("GoreBox not running") };
    let sdk = match Sdk::resolve(&m, &Il2::scan(&m, WANT)) {
        Ok(s) => s,
        Err(e) => return eprintln!("not ready: {e}"),
    };
    let w = Slow::new().tick(&m, &sdk);
    let rb = m.q(m.q(w.local + sdk.pc_rb) + nat::CACHED_PTR);
    let Some(sc) = Scene::find(&m, rb) else { return eprintln!("no PhysX scene from local rigidbody 0x{rb:x}") };
    eprintln!("scene 0x{:x} actors at +0x{:x}, {} actors, {} static", sc.np, sc.actors, sc.actors(&m).len(), sc.statics(&m));
    let t = sc.triangles(&m);
    eprintln!("{} triangles", t.len());
    eprintln!("{}", sc.report(&m));
}

#[derive(Clone, Copy)]
struct Bones {
    hier: u64,
    pelvis_tf: u64,
    idx: [i32; 19],
    max: usize,
}

#[derive(Clone)]
struct Target {
    ctrl: u64,
    team: bool,
    name: String,
    npc: bool,
    dead: bool,
    sys: u64,
    bones: Option<Bones>,
}

#[derive(Default)]
struct World {
    local: u64,

    local_pns: u64,
    cam_off: u64,
    look: Option<sdk::Look>,
    bvh: Option<Arc<Bvh>>,

    fists: Option<Fists>,
    fists_out: bool,

    fire_reach: f32,

    ping: f32,
    net_delay: f32,
    host: bool,
    mods_off: Option<mods::Off>,
    live: mods::Live,
    decoy: Option<Decoy>,
    ball: Option<Ballistic>,

    trigger: Option<rage::Gun>,
    hit_defs: Option<HitDefs>,
    pool: Option<Pool>,
    targets: Vec<Target>,
    status: String,
}

struct Slow {
    names: HashMap<i32, String>,
    names_off: Option<u64>,
    names_probe: Instant,
    bones: HashMap<u64, Bones>,
    bones_fail: HashMap<u64, Instant>,
    cam_off: HashMap<u64, u64>,
    look: HashMap<u64, Option<sdk::Look>>,
    scene: Option<Scene>,
    statics: usize,
    bvh: Option<Arc<Bvh>>,
    bvh_check: Instant,
    has: HashMap<(u64, &'static str), Option<u64>>,
    gun_off: Option<u64>,
    gun_probe: Instant,
    decoy: Option<Decoy>,
    decoy_try: Instant,

    melee_k: Option<Option<u64>>,
    fists: Option<(u64, u64, u64)>,

    fists_line: Option<(u64, u64)>,
    fists_try: Instant,
    hit_defs: Option<HitDefs>,
    hit_try: Instant,
    pool: Option<Pool>,
    pool_try: Instant,
}

impl Slow {
    fn new() -> Slow {
        let past = Instant::now() - Duration::from_secs(60);
        Slow {
            names: HashMap::new(),
            names_off: None,
            names_probe: past,
            bones: HashMap::new(),
            bones_fail: HashMap::new(),
            cam_off: HashMap::new(),
            look: HashMap::new(),
            scene: None,
            statics: 0,
            bvh: None,
            bvh_check: past,
            has: HashMap::new(),
            gun_off: None,
            gun_probe: past,
            decoy: None,
            decoy_try: past,
            melee_k: None,
            fists: None,
            fists_line: None,
            fists_try: past,
            hit_defs: None,
            hit_try: past,
            pool: None,
            pool_try: past,
        }
    }

    fn pool(&mut self, m: &Mem, want: bool) -> Option<Pool> {
        if self.pool.is_some_and(|p| !p.valid(m)) {
            self.pool = None;
        }
        if self.pool.is_none() && want && self.pool_try.elapsed() >= Duration::from_secs(15) {
            self.pool_try = Instant::now();
            self.pool = Pool::find(m);
        }
        self.pool
    }

    fn organs(&mut self, m: &Mem, pc: u64) -> u64 {
        let mut o = pc;
        for name in ["hostSource", "vitals", "system"] {
            let Some(off) = mem::ok(o).then(|| self.field(m, m.q(o), name)).flatten() else { return 0 };
            o = m.q(o + off);
        }
        o
    }

    fn hit_defs(&mut self, m: &Mem) -> Option<HitDefs> {
        if self.hit_defs.is_none() && self.hit_try.elapsed() >= Duration::from_secs(15) {
            self.hit_try = Instant::now();
            self.hit_defs = HitDefs::resolve(m, &Il2::scan(m, HIT_WANT));
        }
        self.hit_defs
    }

    fn fists(&mut self, m: &Mem, s: &Sdk, local: u64, want: bool) -> (Option<Fists>, bool) {
        if let Some((obj, cam, owner)) = self.fists {
            let native = m.q(obj + nat::CACHED_PTR);
            if mem::ok(native) && m.q(m.q(native + nat::NAT_MANAGED)) == obj && m.q(obj + owner) == local {
                let mut attacks = [(0u64, 0u64); 4];
                let arr = il2::field(m, m.q(obj), "attacks").map_or(0, |o| m.q(obj + o));
                let n = m.i32(arr + 0x18).clamp(0, 4) as u64;
                for i in 0..n {
                    let a = m.q(arr + 0x20 + i * 8);
                    if let Some(r) = mem::ok(a).then(|| il2::field(m, m.q(a), "radius")).flatten() {
                        attacks[i as usize] = (a, a + r);
                    }
                }
                return (Some(Fists { obj, class: m.q(obj), cam: obj + cam, attacks, line: self.fists_line.map(|(f, o)| (obj + f, obj + o)) }), m.u8(native + nat::COMP_ACTIVE) != 0);
            }
            self.fists = None;
        }
        if !want || local == 0 || self.fists_try.elapsed() < Duration::from_secs(5) {
            return (None, false);
        }
        self.fists_try = Instant::now();
        let Some(k) = *self.melee_k.get_or_insert_with(|| il2::class_with_fields(m, &["meleeId", "builtInUnarmed"])) else { return (None, false) };
        let fs = il2::fields(m, k);
        let of_type = |ty: u64| fs.iter().find(|f| !f.stat && f.ty == ty).map(|f| f.off);
        let (Some(cam), Some(owner), Some(unarmed)) = (of_type(m.q(s.k_cam + il2::K_TYPE)), of_type(m.q(s.k_pc + il2::K_TYPE)), fs.iter().find(|f| f.name == "builtInUnarmed").map(|f| f.off)) else {
            return (None, false);
        };
        let obj = mem::objects(m, k).into_iter().find(|&o| {
            let native = m.q(o + nat::CACHED_PTR);
            mem::ok(native) && m.q(m.q(native + nat::NAT_MANAGED)) == o && m.q(o + owner) == local && m.u8(o + unarmed) != 0
        });

        let field = |n: &str, te: u8| fs.iter().find(|f| !f.stat && f.name == n && f.te == te).map(|f| f.off);
        self.fists_line = match (field("dhqu", il2::TE_BOOL), field("dhqx", il2::TE_VALUETYPE), field("dhqy", il2::TE_VALUETYPE)) {
            (Some(f), Some(o), Some(d)) if d == o + 12 => Some((f, o)),
            _ => None,
        };
        self.fists = obj.map(|o| (o, cam, owner));
        self.fists(m, s, local, false)
    }

    fn decoy(&mut self, m: &Mem, s: &Sdk, aim: &Aim, local: u64, cam_off: u64) -> Option<Decoy> {
        if let Some(d) = self.decoy
            && !silent::live(m, &d)
        {
            self.decoy = None;
        }
        let want = {
            let c = aim.cfg.lock().unwrap();
            c.silent || c.rage || c.aura
        };
        if self.decoy.is_none() && want && cam_off != 0 && self.decoy_try.elapsed() >= Duration::from_secs(10) {
            self.decoy_try = Instant::now();
            aim.state.lock().unwrap().decoy = "searching".into();
            let cam = m.q(m.q(local + s.pc_item) + cam_off);
            let vt = m.q(m.q(cam + nat::CACHED_PTR));
            self.decoy = silent::decoy_from(m, s, vt);
            aim.state.lock().unwrap().decoy = if self.decoy.is_some() { "ok".into() } else { "none found".into() };
        }
        self.decoy
    }

    fn tick(&mut self, m: &Mem, s: &Sdk) -> World {
        let nm = m.q(s.statics(m, s.k_if) + s.if_nm);
        let (ping, tick) = s.net(m, nm);
        let cm = m.q(nm + s.nm_client);
        let me = m.q(cm + s.cm_conn);
        let my_cid = if mem::ok(me) { m.i32(me + s.conn_cid) } else { -1 };
        let sides = s.sides(m);
        let spawned = s.int_dict(m, m.q(m.q(cm + s.cm_objects) + s.mo_spawned)).unwrap_or_default();
        self.refresh_names(m, s);

        let mut w = World { host: my_cid >= 0 && s.host(m) == my_cid, ping, net_delay: ping / 2000.0 + 2.0 * tick, ..Default::default() };
        for (_, no) in spawned {
            let Some(pns) = s.list(m, m.q(no + s.no_behaviours)).into_iter().find(|&nb| s.is(m, nb, s.k_pns)) else {
                continue;
            };
            let pc = m.q(pns + s.pns_ctrl);
            let native = m.q(pc + nat::CACHED_PTR);
            if !s.is(m, pc, s.k_pc) || !mem::ok(native) || m.u8(native + nat::COMP_ACTIVE) == 0 {
                continue;
            }
            let owner = m.q(no + s.no_owner);
            let cid = if mem::ok(owner) { m.i32(owner + s.conn_cid) } else { -1 };
            let npc = m.u8(pc + s.pc_ai) != 0;
            if !npc && cid >= 0 && owner == me {
                (w.local, w.local_pns) = (pc, pns);
                w.fire_reach = s.pns_fire_origin.and_then(|o| m.rd::<f32>(pns + o)).filter(|v| v.is_finite()).unwrap_or(0.0);
                continue;
            }
            let name = match (npc, self.names.get(&cid)) {
                (true, _) => "npc".into(),
                (false, Some(n)) => n.clone(),
                (false, None) => format!("#{cid}"),
            };
            let bones = self.bones_for(m, s, pc);
            let dead = self.dead(m, s, pc);
            let sys = self.organs(m, pc);
            w.targets.push(Target { ctrl: pc, team: !npc && cid >= 0 && sides.mates(cid, my_cid), name, npc, dead, sys, bones });
        }
        self.bones.retain(|k, _| w.targets.iter().any(|t| t.ctrl == *k));
        self.bones_fail.retain(|k, _| w.targets.iter().any(|t| t.ctrl == *k));

        if w.local == 0 {
            w.status = "not in a server".into();
            return w;
        }

        let k = m.q(m.q(w.local + s.pc_item));
        w.cam_off = match self.cam_off.get(&k) {
            Some(&o) => o,
            None => match il2::field(m, k, "aimCamera") {
                Some(o) => *self.cam_off.entry(k).or_insert(o),
                None => 0,
            },
        };
        let ml = m.q(w.local + s.pc_look);
        w.look = *self.look.entry(m.q(ml)).or_insert_with(|| s.look(m, ml));
        self.world(m, s, w.local);
        w.bvh = self.bvh.clone();
        (w.mods_off, w.live) = self.mods(m, s, w.local);
        w.ball = self.ballistic(m, w.live.gun);
        w.trigger = self.trigger(m, w.live.gun);
        w.hit_defs = self.hit_defs(m);
        w.status = format!("{} tracked", w.targets.len());
        w
    }

    fn ballistic(&mut self, m: &Mem, gun: u64) -> Option<Ballistic> {
        let k = m.q(gun);
        let prof = m.q(gun + self.field(m, k, "profile")?);
        let pk = m.q(prof);
        let f = |o: Option<u64>| o.and_then(|o| m.rd::<f32>(prof + o));
        let (speed, drag, grav) = (f(self.field(m, pk, "muzzleSpeed"))?, f(self.field(m, pk, "drag")).unwrap_or(0.0), f(self.field(m, pk, "gravityScale")).unwrap_or(1.0));
        let inherit = self.field(m, k, "inheritVelocityScale").and_then(|o| m.rd::<f32>(gun + o)).unwrap_or(0.0);
        (speed > 1.0 && speed.is_finite()).then_some(Ballistic { speed, drag, grav, inherit })
    }

    fn trigger(&mut self, m: &Mem, gun: u64) -> Option<rage::Gun> {
        if !mem::ok(gun) {
            return None;
        }
        let k = m.q(gun);
        let auto = m.u8(gun + self.field(m, k, "automatic")?) != 0;
        let cooldown = m.rd::<f32>(gun + self.field(m, k, "fireCooldown")?).filter(|c| c.is_finite())?;
        Some(rage::Gun { auto, cooldown })
    }

    fn dead(&mut self, m: &Mem, s: &Sdk, pc: u64) -> bool {
        let item = m.q(pc + s.pc_item);
        let Some(pm_off) = mem::ok(item).then(|| self.field(m, m.q(item), "puppetMaster")).flatten() else { return false };
        let pm = m.q(item + pm_off);
        let Some(st) = mem::ok(pm).then(|| self.field(m, m.q(pm), "state")).flatten() else { return false };
        m.i32(pm + st) != 0
    }

    fn field(&mut self, m: &Mem, k: u64, name: &'static str) -> Option<u64> {
        *self.has.entry((k, name)).or_insert_with(|| il2::field(m, k, name))
    }

    fn mods(&mut self, m: &Mem, s: &Sdk, local: u64) -> (Option<mods::Off>, mods::Live) {
        let mut off = mods::Off::default();
        let mut live = mods::Live::default();
        let ml = m.q(local + s.pc_look);
        if mem::ok(ml) {
            let k = m.q(ml);
            live.ml = ml;
            off.ml_drift = self.field(m, k, "aimDrift").unwrap_or(0);
            off.ml_drift_deg = self.field(m, k, "driftDegrees").unwrap_or(0);
        }
        if self.gun_off.is_none() && self.gun_probe.elapsed() >= Duration::from_secs(2) {
            self.gun_probe = Instant::now();
            for &o in &s.pc_fields {
                let v = m.q(local + o);
                if mem::ok(v) && mem::ok(m.q(v)) && self.field(m, m.q(v), "cameraRecoilDegrees").is_some() {
                    self.gun_off = Some(o);
                    break;
                }
            }
        }
        let gun = self.gun_off.map_or(0, |o| m.q(local + o));
        if let Some(r) = mem::ok(gun).then(|| self.field(m, m.q(gun), "cameraRecoilDegrees")).flatten() {
            live.gun = gun;
            off.gun_recoil = r;
            off.gun_cam = self.field(m, m.q(gun), "sourceCamera").unwrap_or(0);
            for (i, name) in mods::SPREAD.iter().enumerate() {
                off.gun_spread[i] = self.field(m, m.q(gun), name).unwrap_or(0);
            }
            let go = m.q(m.q(gun + nat::CACHED_PTR) + nat::COMP_GO);
            let comps = m.q(go + nat::GO_COMPS);
            for i in 0..m.u32(go + nat::GO_NCOMPS).min(64) as u64 {
                let mg = m.q(m.q(m.q(comps + i * 16 + 8) + nat::NAT_MANAGED));
                if !mem::ok(mg) {
                    continue;
                }
                let k = m.q(mg);
                if let Some(mag) = self.field(m, k, "adsMagnification") {
                    live.vm = mg;
                    off.vm_mag = mag;
                    off.vm_fov = self.field(m, k, "adsFov").unwrap_or(0);
                    off.vm_recoil = self.field(m, k, "viewmodelRecoil").unwrap_or(0);
                    off.vm_sway = self.field(m, k, "viewmodelSway").unwrap_or(0);
                    break;
                }
            }
        }
        (Some(off), live)
    }

    fn world(&mut self, m: &Mem, s: &Sdk, local: u64) {
        let rb = m.q(m.q(local + s.pc_rb) + nat::CACHED_PTR);
        if self.scene.is_none_or(|sc| m.q(rb + nat::RB_SCENE) != sc.np) {
            self.scene = Scene::find(m, rb);
            (self.statics, self.bvh) = (0, None);
        }
        let Some(sc) = self.scene else { return };
        if self.bvh_check.elapsed() < Duration::from_secs(3) {
            return;
        }
        self.bvh_check = Instant::now();
        let n = sc.statics(m);
        if n != self.statics || self.bvh.is_none() {
            self.statics = n;
            self.bvh = Some(Arc::new(Bvh::build(sc.triangles(m))));
        }
    }

    fn refresh_names(&mut self, m: &Mem, s: &Sdk) {
        let reg = s.registry(m);
        if !mem::ok(reg) {
            return;
        }
        if let Some(off) = self.names_off {
            if let Some(n) = s.names_at(m, reg, off) {
                self.names = n.into_iter().collect();
                return;
            }
            self.names_off = None;
        }
        if self.names_probe.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.names_probe = Instant::now();
        for &off in &s.reg_fields {
            if let Some(n) = s.names_at(m, reg, off) {
                self.names = n.into_iter().collect();
                self.names_off = Some(off);
                return;
            }
        }
    }

    fn bones_for(&mut self, m: &Mem, s: &Sdk, ctrl: u64) -> Option<Bones> {
        let hier = m.q(m.q(m.q(ctrl + s.pc_head) + nat::CACHED_PTR) + nat::TF_HIER);
        if let Some(b) = self.bones.get(&ctrl) {
            if valid(m, b, hier) {
                return Some(*b);
            }
            self.bones.remove(&ctrl);
        }
        if self.bones_fail.get(&ctrl).is_some_and(|t| t.elapsed() < Duration::from_secs(3)) {
            return None;
        }
        match resolve_bones(m, hier) {
            Some(b) => {
                self.bones.insert(ctrl, b);
                Some(b)
            }
            None => {
                self.bones_fail.insert(ctrl, Instant::now());
                None
            }
        }
    }
}

fn valid(m: &Mem, b: &Bones, hier: u64) -> bool {
    b.hier == hier && m.q(m.q(hier + nat::H_TFS) + b.idx[PELVIS] as u64 * 8) == b.pelvis_tf
}

const HIST: usize = 7;
const CHECK: [usize; 6] = [PELVIS, HEAD, 9, 12, 15, 18];

#[derive(Default)]
struct Fast {
    hist: HashMap<u64, VecDeque<Ent>>,
    vel: HashMap<u64, Track>,
    cam: Track,
    watch: hits::Watch,
    hits: Vec<Hit>,
}

#[derive(Clone, Copy)]
struct Track {
    p: V3,
    at: Instant,
    v: V3,
}

impl Default for Track {
    fn default() -> Track {
        Track { p: [f32::NAN; 3], at: Instant::now(), v: [0.0; 3] }
    }
}

impl Track {
    fn push(&mut self, p: V3) -> V3 {
        let dt = self.at.elapsed().as_secs_f32();
        if p != self.p && dt > 1e-4 {
            if self.p[0].is_finite() && dt < 0.25 {
                let a = (dt / 0.06).min(1.0);
                for k in 0..3 {
                    let raw = (p[k] - self.p[k]) / dt;
                    self.v[k] += (raw - self.v[k]) * a;
                }
            }
            (self.p, self.at) = (p, Instant::now());
        } else if dt > 0.25 {
            self.v = [0.0; 3];
        }
        self.v
    }
}

impl Fast {
    fn tick(&mut self, m: &Mem, s: &Sdk, w: &World) -> Snap {
        if w.local == 0 || w.cam_off == 0 {
            self.hist.clear();
            return Snap { status: if w.status.is_empty() { "starting".into() } else { w.status.clone() }, ..Default::default() };
        }
        if let Some(d) = &w.hit_defs {
            let bodies: Vec<(u64, u64)> = w.targets.iter().filter(|t| t.sys != 0).map(|t| (t.ctrl, t.sys)).collect();
            if let Some((id, part, kind)) = self.watch.tick(m, d, &bodies)
                && let Some(t) = w.targets.iter().find(|t| t.ctrl == id)
            {
                self.hits.push(Hit { at: Instant::now(), name: t.name.clone(), npc: t.npc, part, kind });
            }
        }
        let cam = || m.q(m.q(m.q(w.local + s.pc_item) + w.cam_off) + nat::CACHED_PTR);

        for _ in 0..4 {
            let Some(c) = camera(m, cam()) else { break };
            let raw: Vec<(u64, Ent)> = w.targets.iter().filter_map(|t| Some((t.ctrl, ent(m, s, t, c.pos)?))).collect();
            if camera(m, cam()).is_some_and(|c2| c2.vp == c.vp) {
                self.hist.retain(|k, _| raw.iter().any(|(id, _)| id == k));
                self.vel.retain(|k, _| raw.iter().any(|(id, _)| id == k));
                let mut ents: Vec<Ent> = raw.into_iter().map(|(k, e)| self.settle(k, e)).collect();
                for e in &mut ents {
                    e.vel = self.vel.entry(e.id).or_default().push(e.bones[PELVIS].unwrap_or(e.root));
                }
                let cam_vel = self.cam.push(c.pos);
                if let Some(bvh) = &w.bvh {
                    for e in &mut ents {
                        e.vis = sight(bvh, c.pos, e);
                    }
                }
                return Snap {
                    ok: true,
                    status: w.status.clone(),
                    vp: c.vp,
                    view: c.view,
                    fy: c.fy,
                    cam: c.pos,
                    cam_vel,
                    ball: w.ball,
                    look: w.look.and_then(|l| look(m, s, w.local, l)),
                    vis_tris: w.bvh.as_ref().map_or(0, |b| b.len()),
                    ents,
                    aim_cam: m.q(m.q(w.local + s.pc_item) + w.cam_off),
                    local: w.local,
                    ..Default::default()
                };
            }
        }
        Snap { status: "no camera".into(), ..Default::default() }
    }

    fn settle(&mut self, k: u64, e: Ent) -> Ent {
        let h = self.hist.entry(k).or_default();
        h.push_back(e);
        if h.len() > HIST {
            h.pop_front();
        }
        let pick = (0..h.len()).rev().find(|&i| h.iter().filter(|o| close(o, &h[i])).count() * 2 > h.len());
        h[pick.unwrap_or(h.len() - 1)].clone()
    }
}

fn close(a: &Ent, b: &Ent) -> bool {
    CHECK.iter().all(|&i| match (a.bones[i], b.bones[i]) {
        (Some(p), Some(q)) => (p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2) < 0.35 * 0.35,
        _ => true,
    })
}

fn sight(bvh: &Bvh, cam: V3, e: &Ent) -> u32 {
    let mut vis = 0;
    for b in VIS {
        let Some(p) = e.bones[b] else { continue };
        let d = [cam[0] - p[0], cam[1] - p[1], cam[2] - p[2]];
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-3);
        let end = std::array::from_fn(|k| p[k] + d[k] / l * 0.1);
        if !bvh.blocked(cam, end) {
            vis |= 1 << b;
        }
    }
    vis
}

fn look(m: &Mem, s: &Sdk, local: u64, l: sdk::Look) -> Option<LookNow> {
    let ml = m.q(local + s.pc_look);
    let yt: [f32; 4] = m.rd(ml + l.yaw)?;
    let pt: [f32; 4] = m.rd(ml + l.pitch)?;
    let sens: f32 = m.rd(ml + l.sens)?;

    let tf = m.q(m.q(local + s.pc_head) + nat::CACHED_PTR);
    let hier = m.q(tf + nat::TF_HIER);
    let hidx = m.i32(tf + nat::TF_INDEX).max(0) as u64;
    let lt = m.q(hier + nat::H_LOCAL);
    let ya: [f32; 4] = m.rd(lt + 0x10)?;
    let pa: [f32; 4] = m.rd(lt + hidx * 0x30 + 0x10)?;
    let all = [yt, pt, ya, pa].concat();
    all.iter().all(|v| v.is_finite()).then(|| LookNow { yaw_t: yaw(yt), yaw_a: yaw(ya), pitch_t: -pitch(pt), pitch_a: -pitch(pa), sens })
}

fn yaw(q: [f32; 4]) -> f32 {
    let [x, y, z, w] = q;
    (2.0 * (w * y + x * z)).atan2(1.0 - 2.0 * (y * y + x * x)).to_degrees()
}

fn pitch(q: [f32; 4]) -> f32 {
    let [x, y, z, w] = q;
    (2.0 * (w * x - y * z)).clamp(-1.0, 1.0).asin().to_degrees()
}

struct Cam {
    vp: [f32; 16],
    view: [f32; 16],
    fy: f32,
    pos: V3,
}

fn camera(m: &Mem, cam: u64) -> Option<Cam> {
    let view: [f32; 16] = m.rd(cam + nat::CAM_VIEW)?;
    let proj: [f32; 16] = m.rd(cam + nat::CAM_PROJ)?;
    if proj[11] > -0.5 || view.iter().chain(proj.iter()).any(|v| !v.is_finite()) {
        return None;
    }

    let t = [view[12], view[13], view[14]];
    let pos = std::array::from_fn(|j| -(0..3).map(|i| view[j * 4 + i] * t[i]).sum::<f32>());
    Some(Cam { vp: mul(&proj, &view), view, fy: proj[5], pos })
}

fn ent(m: &Mem, s: &Sdk, t: &Target, cam: V3) -> Option<Ent> {
    let tf = m.q(m.q(t.ctrl + s.pc_head) + nat::CACHED_PTR);
    if !mem::ok(tf) {
        return None;
    }
    let hier = m.q(tf + nat::TF_HIER);
    let hidx = m.i32(tf + nat::TF_INDEX).max(0) as usize;
    let b = t.bones.filter(|b| valid(m, b, hier));
    let n = b.map_or(hidx, |b| b.max.max(hidx)) + 1;

    let lt: Vec<f32> = m.bytes(m.q(hier + nat::H_LOCAL), n * 0x30)?.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
    let par: Vec<i32> = m.bytes(m.q(hier + nat::H_PARENT), n * 4)?.chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect();

    let root = world(&lt, &par, 0)?;
    let mut e = Ent { id: t.ctrl, team: t.team, name: t.name.clone(), npc: t.npc, dead: t.dead, root, ..Default::default() };
    match b {
        Some(b) => {
            for (k, &i) in b.idx.iter().enumerate() {
                if i >= 0 {
                    e.bones[k] = world(&lt, &par, i as usize);
                }
            }
        }
        None => {
            e.bones[PELVIS] = Some(root);
            e.bones[HEAD] = world(&lt, &par, hidx);
        }
    }
    let c = e.bones[PELVIS].unwrap_or(root);
    e.dist = ((c[0] - cam[0]).powi(2) + (c[1] - cam[1]).powi(2) + (c[2] - cam[2]).powi(2)).sqrt();
    Some(e)
}

fn resolve_bones(m: &Mem, hier: u64) -> Option<Bones> {
    let cap = m.u32(hier + nat::H_CAP) as usize;
    if !(2..=8192).contains(&cap) {
        return None;
    }
    let tfb = m.bytes(m.q(hier + nat::H_TFS), cap * 8)?;
    let par: Vec<i32> = m.bytes(m.q(hier + nat::H_PARENT), cap * 4)?.chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect();
    let names: Vec<Option<String>> = tfb
        .chunks_exact(8)
        .map(|c| {
            let t = u64::from_le_bytes(c.try_into().unwrap());
            if !mem::ok(t) {
                return None;
            }
            m.cstr(m.q(m.q(t + nat::COMP_GO) + nat::GO_NAME), 48)
        })
        .collect();
    let is = |i: usize, s: &str| names[i].as_deref() == Some(s);

    let pelvis = (0..cap).find(|&i| is(i, "B_Pelvis") && (0..cap).any(|j| par[j] == i as i32 && is(j, "B_Pelvis_Hitbox"))).or_else(|| (0..cap).find(|&i| is(i, "B_Pelvis")))?;
    let arm = par[pelvis];
    let under = |mut i: i32| {
        for _ in 0..256 {
            if i < 0 || i as usize >= cap {
                return false;
            }
            if i == arm {
                return true;
            }
            i = par[i as usize];
        }
        false
    };

    let mut idx = [-1i32; 19];
    idx[PELVIS] = pelvis as i32;
    for (k, bone) in BONES.iter().enumerate().skip(1) {
        if let Some(i) = (0..cap).find(|&i| is(i, bone) && under(i as i32)) {
            idx[k] = i as i32;
        }
    }
    let max = idx.iter().copied().max()? as usize;
    let pelvis_tf = u64::from_le_bytes(tfb[pelvis * 8..pelvis * 8 + 8].try_into().unwrap());
    Some(Bones { hier, pelvis_tf, idx, max })
}

fn world(lt: &[f32], par: &[i32], i: usize) -> Option<V3> {
    let trs = |j: usize| lt.get(j * 12..j * 12 + 12);
    let t = trs(i)?;
    let mut p = [t[0], t[1], t[2]];
    let mut j = *par.get(i)?;
    for _ in 0..256 {
        if j < 0 {
            break;
        }
        let t = trs(j as usize)?;
        let s = [p[0] * t[8], p[1] * t[9], p[2] * t[10]];
        let r = px::rot([t[4], t[5], t[6], t[7]], s);
        p = [r[0] + t[0], r[1] + t[1], r[2] + t[2]];
        j = *par.get(j as usize)?;
    }
    p.iter().all(|v| v.is_finite() && v.abs() < 1e6).then_some(p)
}

pub fn mul(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    std::array::from_fn(|i| {
        let (c, r) = (i / 4, i % 4);
        (0..4).map(|k| a[k * 4 + r] * b[c * 4 + k]).sum()
    })
}
