use crate::game::Shared;
use crate::game::il2::{self, Il2};
use crate::game::sdk::nat;
use crate::os::mem::{self, Mem};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const WANT: &[&str] = &["GoreBox.Environment.DayNightSky", "UnityEngine.Rendering.Volume", "UnityEngine.Rendering.VolumeComponent", "UnityEngine.Rendering.VolumeParameter"];

#[derive(Clone, Copy, PartialEq)]
pub struct Cfg {
    pub time: bool,
    pub hour: f32,
    pub tint: bool,
    pub sky: [f32; 3],
    pub ground: [f32; 3],
    pub bright: f32,
    pub fog: bool,
    pub fog_scale: f32,
    pub weather: bool,
    pub clouds: f32,
    pub storm: f32,
    pub rain: f32,

    pub grade: bool,
    pub filter: [f32; 3],
    pub hue: f32,
    pub sat: f32,
    pub contrast: f32,
    pub exposure: f32,

    pub glow: bool,
    pub glow_int: f32,
    pub glow_thr: f32,
    pub glow_tint: [f32; 3],
}

impl Default for Cfg {
    fn default() -> Cfg {
        Cfg {
            time: false,
            hour: 12.0,
            tint: false,
            sky: [0.94, 0.97, 1.06],
            ground: [1.06, 1.0, 0.92],
            bright: 1.0,
            fog: false,
            fog_scale: 1.0,
            weather: false,
            clouds: 0.3,
            storm: 0.0,
            rain: 0.0,
            grade: false,
            filter: [1.0, 1.0, 1.0],
            hue: 0.0,
            sat: 0.0,
            contrast: 0.0,
            exposure: 0.0,
            glow: false,
            glow_int: 0.6,
            glow_thr: 0.9,
            glow_tint: [1.0, 1.0, 1.0],
        }
    }
}

impl Cfg {
    fn any(&self) -> bool {
        self.time || self.tint || self.fog || self.weather || self.grade || self.glow
    }
}

const fn look(
    hour: f32,
    sky: [f32; 3],
    ground: [f32; 3],
    bright: f32,
    fog: f32,
    clouds: f32,
    filter: [f32; 3],
    hue: f32,
    sat: f32,
    contrast: f32,
    exposure: f32,
    glow: f32,
    thr: f32,
    tint: [f32; 3],
) -> Cfg {
    Cfg {
        time: true,
        hour,
        tint: true,
        sky,
        ground,
        bright,
        fog: true,
        fog_scale: fog,
        weather: true,
        clouds,
        storm: 0.0,
        rain: 0.0,
        grade: true,
        filter,
        hue,
        sat,
        contrast,
        exposure,
        glow: true,
        glow_int: glow,
        glow_thr: thr,
        glow_tint: tint,
    }
}

#[rustfmt::skip]
pub const PRESETS: [(&str, Cfg); 8] = [
    ("blood moon", look(0.5, [1.0, 0.3, 0.3], [0.7, 0.2, 0.2], 1.3, 1.4, 0.2, [1.0, 0.38, 0.32], 0.0, 35.0, 25.0, -0.3, 2.5, 0.7, [1.0, 0.3, 0.25])),
    ("vaporwave", look(20.5, [1.0, 0.4, 1.0], [0.3, 0.6, 1.0], 1.6, 1.2, 0.2, [1.0, 0.62, 1.0], -25.0, 55.0, 20.0, 0.1, 3.0, 0.6, [1.0, 0.4, 0.9])),
    ("toxic", look(17.5, [0.6, 1.0, 0.5], [0.5, 0.9, 0.4], 1.2, 2.0, 0.4, [0.65, 1.0, 0.45], 35.0, 45.0, 30.0, -0.1, 2.0, 0.7, [0.5, 1.0, 0.3])),
    ("ice", look(10.0, [0.7, 0.85, 1.2], [0.7, 0.8, 1.0], 1.2, 0.8, 0.6, [0.7, 0.88, 1.15], -10.0, -15.0, 15.0, 0.25, 1.5, 0.8, [0.6, 0.8, 1.0])),
    ("noir", look(15.0, [1.0, 1.0, 1.0], [1.0, 1.0, 1.0], 1.0, 1.5, 0.5, [1.0, 1.0, 1.0], 0.0, -100.0, 45.0, -0.2, 0.8, 0.9, [1.0, 1.0, 1.0])),
    ("neon", look(21.5, [1.0, 0.35, 1.0], [0.35, 0.6, 1.0], 1.8, 1.6, 0.2, [0.8, 0.6, 1.0], -15.0, 50.0, 25.0, 0.0, 3.5, 0.55, [0.7, 0.4, 1.0])),
    ("sunset", look(18.7, [1.15, 0.8, 0.75], [1.2, 0.85, 0.6], 1.1, 1.2, 0.45, [1.0, 0.8, 0.65], 5.0, 20.0, 10.0, 0.0, 1.5, 0.8, [1.0, 0.6, 0.4])),
    ("night", look(0.5, [0.55, 0.65, 1.0], [0.45, 0.5, 0.8], 1.3, 0.6, 0.15, [0.75, 0.85, 1.0], 0.0, 10.0, 15.0, -0.2, 1.2, 0.8, [0.6, 0.7, 1.0])),
];

#[derive(Default)]
pub struct Ambiance {
    pub cfg: Mutex<Cfg>,
    pub status: Mutex<String>,
    quit: AtomicBool,
    done: AtomicBool,
}

impl Ambiance {
    pub fn shutdown(&self) {
        self.quit.store(true, Ordering::Relaxed);
        let t = Instant::now();
        while !self.done.load(Ordering::Relaxed) && t.elapsed() < Duration::from_millis(500) {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

pub fn spawn(sh: Arc<Shared>, a: Arc<Ambiance>) {
    std::thread::Builder::new().name("ambiance".into()).spawn(move || run(sh, a)).unwrap();
}

#[derive(Clone, Copy)]
struct Off {
    time: u64,
    lock: u64,
    animate: u64,
    day: u64,
    sky: u64,
    ground: u64,
    fog: u64,
    clouds: u64,
    storm: u64,
    rain: u64,
}

impl Off {
    fn resolve(m: &Mem, k: u64) -> Option<Off> {
        let f = |n: &str| il2::field(m, k, n);
        Some(Off {
            time: f("timeOfDay")?,
            lock: f("lockTimeOfDay")?,
            animate: f("animate")?,
            day: f("dayLengthSeconds")?,
            sky: f("ambientSkyTint")?,
            ground: f("ambientGroundTint")?,
            fog: f("fogDensityScale")?,
            clouds: f("cloudCoverage")?,
            storm: f("storm")?,
            rain: f("rainOverride")?,
        })
    }
}

struct Comp {
    obj: u64,
    params: HashMap<String, (u64, u64, u64)>,
}

struct Post {
    vol: u64,
    grade: Option<Comp>,
    bloom: Option<Comp>,
}

#[derive(Default)]
struct Hold {
    orig: HashMap<u64, (u64, u64, Vec<u8>)>,

    frozen: Option<(Instant, bool, u64)>,
}

struct Session {
    m: Mem,
    k_sky: u64,
    k_vol: u64,
    off: Option<Off>,
    active: u64,
    override_: u64,
    sky: u64,
    post: Option<Post>,
    looked: Instant,
}

fn run(sh: Arc<Shared>, a: Arc<Ambiance>) {
    let mut sess: Option<Session> = None;
    let mut hold = Hold::default();
    loop {
        let quit = a.quit.load(Ordering::Relaxed);
        let cfg = if quit { Cfg::default() } else { *a.cfg.lock().unwrap() };
        let pid = sh.pick(Duration::ZERO).pid;
        if sess.as_ref().is_some_and(|s| s.m.pid != pid || !s.m.alive()) {
            (sess, hold) = (None, Hold::default());
        }
        if cfg.any() && sess.is_none() && pid > 0 {
            sess = open(pid);
        }
        if let Some(s) = sess.as_mut() {
            let status = tick(s, &mut hold, &cfg);
            *a.status.lock().unwrap() = status;
        }
        if quit {
            a.done.store(true, Ordering::Relaxed);
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn open(pid: i32) -> Option<Session> {
    let m = Mem::open(pid).ok()?;
    let il = Il2::scan(&m, WANT);
    let (k_sky, k_vol) = (il.class(WANT[0]).unwrap_or(0), il.class(WANT[1]).unwrap_or(0));
    let off = (k_sky != 0).then(|| Off::resolve(&m, k_sky)).flatten();
    let active = il.class(WANT[2]).and_then(|k| il2::field(&m, k, "active")).unwrap_or(0x18);
    let override_ = il.class(WANT[3]).and_then(|k| il2::field(&m, k, "m_OverrideState")).unwrap_or(0x10);
    Some(Session { m, k_sky, k_vol, off, active, override_, sky: 0, post: None, looked: Instant::now() - Duration::from_secs(60) })
}

fn alive(m: &Mem, k: u64, o: u64) -> bool {
    let native = m.q(o + nat::CACHED_PTR);
    k != 0 && mem::ok(o) && m.q(o) == k && mem::ok(native) && m.q(m.q(native + nat::NAT_MANAGED)) == o
}

fn profile(m: &Mem, kv: u64, v: u64) -> u64 {
    let f = |n: &str| il2::field(m, kv, n).map_or(0, |o| m.q(v + o));
    Some(f("sharedProfile")).filter(|&p| mem::ok(p)).unwrap_or_else(|| f("m_InternalProfile"))
}

fn comps(m: &Mem, prof: u64) -> Vec<u64> {
    let list = m.q(prof + 0x18);
    let (items, n) = (m.q(list + 0x10), m.i32(list + 0x18).clamp(0, 64) as u64);
    (0..n).map(|i| m.q(items + 0x20 + i * 8)).filter(|&c| mem::ok(c)).collect()
}

fn comp(m: &Mem, c: u64) -> Comp {
    let k = m.q(c);
    let params = il2::fields(m, k)
        .into_iter()
        .filter(|f| !f.stat)
        .filter_map(|f| {
            let p = m.q(c + f.off);
            let value = il2::field(m, m.q(p), "m_Value")?;
            mem::ok(p).then(|| (f.name, (p, p + value, p)))
        })
        .collect();
    Comp { obj: c, params }
}

fn find_post(m: &Mem, kv: u64) -> Option<Post> {
    let pri = il2::field(m, kv, "priority")?;
    let global = il2::field(m, kv, "m_IsGlobal")?;
    let mut best: Option<(f32, u64, Vec<u64>)> = None;
    for v in mem::objects(m, kv).into_iter().filter(|&o| alive(m, kv, o) && m.u8(o + global) != 0) {
        let cs = comps(m, profile(m, kv, v));
        let has = cs.iter().any(|&c| il2::class_name(m, m.q(c)).as_deref() == Some("ColorAdjustments"));
        let p = m.rd::<f32>(v + pri).unwrap_or(0.0);
        if has && best.as_ref().is_none_or(|b| p > b.0) {
            best = Some((p, v, cs));
        }
    }
    let (_, vol, cs) = best?;
    let by = |name: &str| cs.iter().find(|&&c| il2::class_name(m, m.q(c)).as_deref() == Some(name)).map(|&c| comp(m, c));
    Some(Post { vol, grade: by("ColorAdjustments"), bloom: by("Bloom") })
}

fn f32b(v: f32) -> Vec<u8> {
    v.to_le_bytes().to_vec()
}

fn color(c: [f32; 3], b: f32) -> Vec<u8> {
    [c[0] * b, c[1] * b, c[2] * b, 1.0].iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn tick(s: &mut Session, hold: &mut Hold, cfg: &Cfg) -> String {
    let m = &s.m;
    let sky_on = cfg.time || cfg.tint || cfg.fog || cfg.weather;
    let post_on = cfg.grade || cfg.glow;

    let rescan = s.looked.elapsed() > Duration::from_secs(5);
    if !alive(m, s.k_sky, s.sky) {
        s.sky = 0;
    }
    if s.post.as_ref().is_some_and(|p| !alive(m, s.k_vol, p.vol)) {
        s.post = None;
    }
    if rescan && ((sky_on && s.sky == 0) || (post_on && s.post.is_none())) {
        s.looked = Instant::now();
        if sky_on && s.sky == 0 {
            s.sky = mem::objects(m, s.k_sky).into_iter().find(|&o| alive(m, s.k_sky, o)).unwrap_or(0);
        }
        if post_on && s.post.is_none() {
            s.post = find_post(m, s.k_vol);
        }
    }

    let mut want: Vec<(u64, u64, Vec<u8>)> = vec![];
    let mut time_at = 0;
    if let (Some(o), true) = (s.off, s.sky != 0) {
        let sk = s.sky;
        if cfg.time {
            time_at = sk + o.time;
            want.push((sk, sk + o.time, f32b((cfg.hour / 24.0).rem_euclid(1.0))));
            want.push((sk, sk + o.lock, vec![1]));
            want.push((sk, sk + o.animate, vec![0]));
        }
        if cfg.tint {
            want.push((sk, sk + o.sky, color(cfg.sky, cfg.bright)));
            want.push((sk, sk + o.ground, color(cfg.ground, cfg.bright)));
        }
        if cfg.fog {
            want.push((sk, sk + o.fog, f32b(cfg.fog_scale)));
        }
        if cfg.weather {
            want.push((sk, sk + o.clouds, f32b(cfg.clouds)));
            want.push((sk, sk + o.storm, f32b(cfg.storm)));
            want.push((sk, sk + o.rain, f32b(cfg.rain)));
        }
    }
    if let Some(p) = &s.post {
        let mut set = |c: &Comp, vals: &[(&str, Vec<u8>)]| {
            want.push((c.obj, c.obj + s.active, vec![1]));
            for (name, v) in vals {
                if let Some(&(obj, value, _)) = c.params.get(*name) {
                    want.push((obj, obj + s.override_, vec![1]));
                    want.push((obj, value, v.clone()));
                }
            }
        };
        if let (true, Some(c)) = (cfg.grade, &p.grade) {
            set(c, &[("postExposure", f32b(cfg.exposure)), ("contrast", f32b(cfg.contrast)), ("colorFilter", color(cfg.filter, 1.0)), ("hueShift", f32b(cfg.hue)), ("saturation", f32b(cfg.sat))]);
        }
        if let (true, Some(c)) = (cfg.glow, &p.bloom) {
            set(c, &[("intensity", f32b(cfg.glow_int)), ("threshold", f32b(cfg.glow_thr)), ("tint", color(cfg.glow_tint, 1.0))]);
        }
    }

    for (obj, a, v) in &want {
        if !hold.orig.contains_key(a) {
            let Some(old) = m.bytes(*a, v.len()) else { continue };
            if *a == time_at
                && let Some(o) = s.off
            {
                hold.frozen = Some((Instant::now(), m.u8(s.sky + o.animate) != 0, s.sky + o.day));
            }
            crate::game::undo::keep(m.fd(), *a, *obj, m.q(*obj), &old);
            hold.orig.insert(*a, (*obj, m.q(*obj), old));
        }
        if m.bytes(*a, v.len()).as_ref() != Some(v) {
            m.write(*a, v);
        }
    }

    let gone: Vec<u64> = hold.orig.keys().copied().filter(|a| !want.iter().any(|(_, w, _)| w == a)).collect();
    for a in gone {
        let (obj, k, mut old) = hold.orig.remove(&a).unwrap();
        if old.len() == 4
            && let Some((since, running, day_at)) = hold.frozen.filter(|f| f.2 != 0 && hold_is_time(&s.off, s.sky, a))
        {
            hold.frozen = None;
            if running {
                let t = f32::from_le_bytes(old[..4].try_into().unwrap());
                let day = m.rd::<f32>(day_at).filter(|d| *d > 1.0).unwrap_or(1800.0);
                old = ((t + since.elapsed().as_secs_f32() / day).rem_euclid(1.0)).to_le_bytes().to_vec();
            }
        }
        if m.q(obj) == k {
            m.write(a, &old);
        }
        crate::game::undo::forget(a);
    }
    let mut status = vec![];
    if sky_on && s.sky == 0 {
        status.push("no sky in this map");
    }
    if post_on && s.post.is_none() {
        status.push("no grading volume found");
    }
    if want.is_empty() && status.is_empty() { String::new() } else { format!("world: {} values held{}{}", want.len(), if status.is_empty() { "" } else { ", " }, status.join(", ")) }
}

fn hold_is_time(off: &Option<Off>, sky: u64, a: u64) -> bool {
    off.is_some_and(|o| sky != 0 && a == sky + o.time)
}

pub fn volumes_report() {
    let Some(m) = mem::find_pid().and_then(|p| Mem::open(p).ok()) else { return eprintln!("GoreBox not running") };
    let il = Il2::scan(&m, &["UnityEngine.Rendering.Volume", "UnityEngine.Rendering.VolumeParameter"]);
    let Some(kv) = il.class("UnityEngine.Rendering.Volume") else { return println!("no Volume class") };
    let f = |k: u64, n: &str| il2::field(&m, k, n).unwrap_or(0);
    for v in mem::objects(&m, kv).into_iter().filter(|&o| alive(&m, kv, o)) {
        let prof = Some(m.q(v + f(kv, "sharedProfile"))).filter(|&p| mem::ok(p)).unwrap_or_else(|| m.q(v + f(kv, "m_InternalProfile")));
        println!(
            "volume {v:#x} global {} priority {} weight {} profile {prof:#x} internal {:#x}",
            m.u8(v + f(kv, "m_IsGlobal")),
            m.rd::<f32>(v + f(kv, "priority")).unwrap_or(0.0),
            m.rd::<f32>(v + f(kv, "weight")).unwrap_or(0.0),
            m.q(v + f(kv, "m_InternalProfile"))
        );
        let list = m.q(prof + 0x18);
        let (items, n) = (m.q(list + 0x10), m.i32(list + 0x18).clamp(0, 64) as u64);
        for i in 0..n {
            let c = m.q(items + 0x20 + i * 8);
            let kc = m.q(c);
            let name = il2::class_name(&m, kc).unwrap_or_default();
            println!("  {name} active {}", m.u8(c + 0x18));
            let params = m.q(c + 0x28);
            let pn = m.i32(params + 0x18).clamp(0, 64) as u64;
            let names: std::collections::HashMap<u64, String> = il2::fields(&m, kc).into_iter().filter(|fl| !fl.stat).map(|fl| (m.q(c + fl.off), fl.name)).collect();
            for j in 0..pn {
                let p = m.q(params + 0x20 + j * 8);
                let kp = m.q(p);
                let val = il2::field(&m, kp, "m_Value").unwrap_or(0x14);
                let vals: Vec<String> = (0..4).map(|k| format!("{:.3}", m.rd::<f32>(p + val + k * 4).unwrap_or(f32::NAN))).collect();
                println!("    {:<22} {:<28} override {} value {}", names.get(&p).cloned().unwrap_or_default(), il2::class_name(&m, kp).unwrap_or_default(), m.u8(p + 0x10), vals.join(" "));
            }
        }
    }
}

pub fn grade_test() {
    let Some(m) = mem::find_pid().and_then(|p| Mem::open(p).ok()) else { return eprintln!("GoreBox not running") };
    let Some(s) = open(m.pid) else { return println!("open failed") };
    let Some(p) = find_post(&s.m, s.k_vol) else { return println!("no grading volume") };
    println!("volume {:#x}", p.vol);
    for (name, c) in [("ColorAdjustments", &p.grade), ("Bloom", &p.bloom)] {
        let Some(c) = c else { continue };
        let mut ps: Vec<&String> = c.params.keys().collect();
        ps.sort();
        println!("  {name} {:#x} active {} params {:?}", c.obj, s.m.u8(c.obj + s.active), ps);
    }
    let Some(g) = &p.grade else { return };
    let Some(&(sat, sat_v, _)) = g.params.get("saturation") else { return println!("no saturation") };
    let (a0, o0, v0) = (s.m.u8(g.obj + s.active), s.m.u8(sat + s.override_), s.m.rd::<f32>(sat_v).unwrap_or(0.0));
    s.m.write(g.obj + s.active, &[1]);
    s.m.write(sat + s.override_, &[1]);
    s.m.write(sat_v, &(-100.0f32).to_le_bytes());
    let t = Instant::now();
    let mut resets = 0;
    while t.elapsed() < Duration::from_secs(2) {
        if s.m.u8(g.obj + s.active) != 1 || s.m.rd::<f32>(sat_v).unwrap_or(0.0) != -100.0 {
            resets += 1;
            s.m.write(g.obj + s.active, &[1]);
            s.m.write(sat_v, &(-100.0f32).to_le_bytes());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    s.m.write(sat_v, &v0.to_le_bytes());
    s.m.write(sat + s.override_, &[o0]);
    s.m.write(g.obj + s.active, &[a0]);
    println!("2s black and white: the game reset it {resets} times, restored (active {a0}, saturation {v0})");
}
