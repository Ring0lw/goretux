use crate::features::third::Third;
use crate::game::il2::{self, Il2, K_TYPE, Method, TE_BOOL, TE_CLASS, TE_I4, TE_OBJECT, TE_R4, TE_STRING, TE_SZARRAY, TE_VALUETYPE};
use crate::game::remote::{self, Call, Remote, own};
use crate::game::{Ent, Shared};
use crate::os::mem::{self, Mem};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const WANT: &[&str] = &[
    "UnityEngine.Shader",
    "UnityEngine.Material",
    "UnityEngine.Renderer",
    "UnityEngine.GameObject",
    "UnityEngine.Component",
    "UnityEngine.Object",
    "UnityEngine.Transform",
    "System.Type",
    "System.Array",
    "System.Runtime.InteropServices.GCHandle",
    "System.Byte",
];

const ARR_LEN: u64 = 0x18;
const ARR_DATA: u64 = 0x20;

pub struct Api {
    find: Method,
    type_of: Method,
    gc_alloc: Method,
    gc_free: Method,
    game_object: Method,
    transform: Method,
    root: Method,
    children: Method,
    get_mats: Method,
    set_mats: Method,
    get_mat: Method,
    instantiate: Method,
    set_shader: Method,
    get_shader: Method,
    set_float: Method,
    set_color: Method,
    new_array: Method,
    name: Method,
    hide: Method,
    destroy: Method,
    queue: Method,
    set_tex: Method,
    keyword_on: Method,
    keyword_off: Method,
    renderer_ty: u64,
    material_ty: u64,
    byte_ty: u64,
}

impl Api {
    pub fn resolve(m: &Mem, il: &Il2) -> Result<Api, String> {
        let k = |n: &str| il.class(n).ok_or_else(|| format!("no class {n}"));
        let f = |c: &str, n: &str, sig: &[(u8, u64)]| il2::method(m, k(c)?, n, sig).ok_or_else(|| format!("no method {c}.{n}"));
        let any = |t: u8| (t, 0u64);
        let object = m.q(k("UnityEngine.Object")? + K_TYPE);
        Ok(Api {
            find: f("UnityEngine.Shader", "Find", &[any(TE_STRING)])?,
            type_of: f("System.Type", "GetTypeFromHandle", &[any(TE_VALUETYPE)])?,
            gc_alloc: f("System.Runtime.InteropServices.GCHandle", "Alloc", &[any(TE_OBJECT)])?,
            gc_free: f("System.Runtime.InteropServices.GCHandle", "Free", &[])?,
            game_object: f("UnityEngine.Component", "get_gameObject", &[])?,
            transform: f("UnityEngine.GameObject", "get_transform", &[])?,
            root: f("UnityEngine.Transform", "get_root", &[])?,
            children: f("UnityEngine.GameObject", "GetComponentsInChildren", &[any(TE_CLASS), any(TE_BOOL)])?,
            get_mats: f("UnityEngine.Renderer", "get_sharedMaterials", &[])?,
            set_mats: f("UnityEngine.Renderer", "set_sharedMaterials", &[any(TE_SZARRAY)])?,
            get_mat: f("UnityEngine.Renderer", "get_sharedMaterial", &[])?,
            instantiate: f("UnityEngine.Object", "Instantiate", &[(TE_CLASS, object)])?,
            set_shader: f("UnityEngine.Material", "set_shader", &[any(TE_CLASS)])?,
            get_shader: f("UnityEngine.Material", "get_shader", &[])?,
            set_float: f("UnityEngine.Material", "SetFloat", &[any(TE_STRING), any(TE_R4)])?,
            set_color: f("UnityEngine.Material", "SetColor", &[any(TE_STRING), any(TE_VALUETYPE)])?,
            new_array: f("System.Array", "CreateInstance", &[any(TE_CLASS), any(TE_I4)])?,
            name: f("UnityEngine.Object", "get_name", &[])?,
            hide: f("UnityEngine.Object", "set_hideFlags", &[any(TE_VALUETYPE)])?,
            destroy: f("UnityEngine.Object", "Destroy", &[(TE_CLASS, object)])?,
            queue: f("UnityEngine.Material", "set_renderQueue", &[any(TE_I4)])?,
            set_tex: f("UnityEngine.Material", "SetTexture", &[any(TE_STRING), any(TE_CLASS)])?,
            keyword_on: f("UnityEngine.Material", "EnableKeyword", &[any(TE_STRING)])?,
            keyword_off: f("UnityEngine.Material", "DisableKeyword", &[any(TE_STRING)])?,
            renderer_ty: k("UnityEngine.Renderer")? + K_TYPE,
            material_ty: k("UnityEngine.Material")? + K_TYPE,
            byte_ty: k("System.Byte")? + K_TYPE,
        })
    }
}

#[derive(Clone, Copy)]
pub struct Cfg {
    pub on: bool,
    pub npcs: bool,
    pub dead: bool,
    pub vis: [f32; 4],
    pub hid: [f32; 4],

    pub weapon: bool,
    pub local: bool,
    pub glow_col: [f32; 3],
    pub glow: f32,
    pub rainbow: bool,
    pub speed: f32,
}

impl Default for Cfg {
    fn default() -> Cfg {
        Cfg {
            on: false,
            npcs: true,
            dead: false,
            vis: [0.3, 1.0, 0.47, 0.9],
            hid: [1.0, 0.27, 0.27, 0.9],
            weapon: false,
            local: false,
            glow_col: [0.6, 0.35, 1.0],
            glow: 2.5,
            rainbow: true,
            speed: 0.12,
        }
    }
}

impl Cfg {
    fn any(&self) -> bool {
        self.on || self.weapon || self.local
    }

    fn glow_at(&self, t: f32) -> [f32; 4] {
        let [r, g, b] = if self.rainbow { hsv((t * self.speed).fract(), 0.75, 1.0) } else { self.glow_col };
        [r, g, b, self.glow]
    }
}

fn hsv(h: f32, s: f32, v: f32) -> [f32; 3] {
    let k = |n: f32| {
        let k = (n + h * 6.0) % 6.0;
        v - v * s * k.min(4.0 - k).clamp(0.0, 1.0)
    };
    [k(5.0), k(3.0), k(1.0)]
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Enemy,
    Body,
    Weapon,
}

const DEAD: [f32; 4] = [0.5, 0.5, 0.5, 0.6];

const GONE: Duration = Duration::from_secs(2);

const RESCAN: Duration = Duration::from_secs(3);

const NEW_PER_PASS: usize = 4;

const ZTEST_ALWAYS: f32 = 8.0;
const BLEND_SRC_ALPHA: f32 = 5.0;
const BLEND_ONE_MINUS_SRC_ALPHA: f32 = 10.0;
const QUEUE_OVERLAY: u64 = 4000;
const DONT_UNLOAD: u64 = 32;

const NAT_ID: u64 = 0x8;
const LIST_SIZE: u64 = 0x10;

#[derive(Default)]
pub struct Chams {
    pub cfg: Mutex<Cfg>,
    pub status: Mutex<String>,

    pub third: AtomicBool,
    pub third_status: Mutex<String>,
    quit: AtomicBool,
    done: AtomicBool,
}

impl Chams {
    fn say(&self, s: impl Into<String>) {
        let s = s.into();
        let mut cur = self.status.lock().unwrap();
        if *cur != s {
            eprintln!("{s}");
            *cur = s;
        }
    }

    pub fn shutdown(&self) {
        self.quit.store(true, Ordering::Relaxed);
        let t = Instant::now();
        while !self.done.load(Ordering::Relaxed) && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

pub fn spawn(sh: Arc<Shared>, ch: Arc<Chams>) {
    std::thread::Builder::new().name("chams".into()).spawn(move || run(sh, ch)).unwrap();
}

fn run(sh: Arc<Shared>, ch: Arc<Chams>) {
    let mut sess: Option<Session> = None;
    let mut third = Third::default();
    let mut retry = Instant::now();
    loop {
        if ch.quit.load(Ordering::Relaxed) {
            if let Some(mut s) = sess.take() {
                third.off(&s.m, &mut s.r);
                s.close();
            }
            ch.done.store(true, Ordering::Relaxed);
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
        let cfg = *ch.cfg.lock().unwrap();
        let snap = sh.pick(Duration::ZERO);

        if sess.as_ref().is_some_and(|s| s.m.pid != snap.pid || !s.m.alive()) {
            (sess, third) = (None, Third::default());
        }
        let want_third = ch.third.load(Ordering::Relaxed);
        if !cfg.any() && !want_third && !third.active() && sess.as_ref().is_none_or(|s| s.players.is_empty()) {
            ch.say("chams off");
            ch.third_status.lock().unwrap().clear();
            continue;
        }
        if !snap.ok {
            continue;
        }
        if sess.is_none() {
            if retry > Instant::now() {
                continue;
            }
            ch.say("chams: hooking the game");
            match Session::open(snap.pid) {
                Ok(s) => sess = Some(s),
                Err(e) => {
                    ch.say(format!("chams: {e}"));
                    retry = Instant::now() + Duration::from_secs(5);
                    continue;
                }
            }
        }
        let s = sess.as_mut().unwrap();
        match s.sync(&cfg, &snap.ents, snap.aim_cam) {
            Ok(()) => ch.say(format!("chams: {} players, {} meshes", s.players.len(), s.players.values().map(|p| p.rends.len()).sum::<usize>())),
            Err(e) => ch.say(format!("chams: {e}")),
        }
        third.tick(&s.m, &mut s.r, want_third, snap.local);
        *ch.third_status.lock().unwrap() = third.status.clone();
    }
}

struct Strs {
    color: u64,
    ztest: u64,
    zwrite: u64,
    cull: u64,
    src: u64,
    dst: u64,
    base: u64,
    emission: u64,
}

struct Rend {
    rd: u64,
    orig: u64,
    orig_h: u64,
    n: usize,
}

struct Player {
    kind: Kind,

    src: u64,

    shader: u64,
    mat: u64,
    mat_h: u64,

    arr_h: u64,
    rends: Vec<Rend>,

    fills: HashMap<usize, (u64, u64)>,

    color: [f32; 4],
    seen: Instant,
    scanned: Instant,
}

struct Session {
    m: Mem,
    r: Remote,
    api: Api,
    s: Strs,
    rty: u64,
    mty: u64,
    shader: u64,
    lit: u64,

    tpl: u64,
    glow_tpl: u64,
    keep: Vec<u64>,
    players: HashMap<u64, Player>,

    list: Option<u64>,

    weapons: HashMap<u64, bool>,
    born: Instant,
}

fn free(api: &Api, h: u64) -> Call {
    let mut c = Call::method(&api.gc_free, &[own(2)]);
    c.a[2] = h;
    c
}

fn set_color(api: &Api, prop: u64, mat: u64, c: [f32; 4]) -> Call {
    let mut call = Call::method(&api.set_color, &[mat, prop, own(4)]).alive(mat);
    call.a[4] = c[0].to_bits() as u64 | (c[1].to_bits() as u64) << 32;
    call.a[5] = c[2].to_bits() as u64 | (c[3].to_bits() as u64) << 32;
    call
}

impl Session {
    fn open(pid: i32) -> Result<Session, String> {
        let m = Mem::open(pid).map_err(|e| e.to_string())?;
        let want: Vec<&str> = remote::WANT.iter().chain(WANT).copied().collect();
        let il = Il2::scan(&m, &want);
        let api = Api::resolve(&m, &il)?;
        let mut r = Remote::install(&m, &il)?;
        let h = r.hits(&m);
        std::thread::sleep(Duration::from_millis(300));
        if r.hits(&m) == h {
            r.uninstall(&m);
            return Err("the game isn't running EventSystem.Update".into());
        }
        let mut st = |t: &str| r.string(&m, t).ok_or("mailbox scratch full");
        let s = Strs { color: st("_Color")?, ztest: st("_ZTest")?, zwrite: st("_ZWrite")?, cull: st("_Cull")?, src: st("_SrcBlend")?, dst: st("_DstBlend")?, base: 0, emission: 0 };
        let shader_name = r.string(&m, "Hidden/Internal-Colored").ok_or("mailbox scratch full")?;
        let lit_name = r.string(&m, "Universal Render Pipeline/Lit").ok_or("mailbox scratch full")?;

        let rooted = |c: Call| vec![c.to(1, 0), Call::method(&api.gc_alloc, &[0]).needs(0)];
        let res = r.groups(
            &m,
            &[
                rooted(Call::method(&api.find, &[shader_name])),
                rooted(Call::method(&api.type_of, &[api.renderer_ty])),
                rooted(Call::method(&api.type_of, &[api.material_ty])),
                rooted(Call::method(&api.find, &[lit_name])),
                rooted(Call::method(&api.type_of, &[api.byte_ty])),
            ],
        )?;
        let got = |i: usize| res[i][0].filter(|&v| v != 0);
        let keep: Vec<u64> = res.iter().filter_map(|g| g[1]).collect();
        let mut sess = Session { m, r, api, s, rty: 0, mty: 0, shader: 0, lit: 0, tpl: 0, glow_tpl: 0, keep, players: HashMap::new(), list: None, weapons: HashMap::new(), born: Instant::now() };
        let (Some(shader), Some(rty), Some(mty), Some(lit), Some(bty)) = (got(0), got(1), got(2), got(3), got(4)) else {
            sess.close();
            return Err("a shader or type is missing".into());
        };
        (sess.rty, sess.mty, sess.shader, sess.lit) = (rty, mty, shader, lit);

        const HEAP: u64 = 8192;
        let g = vec![Call::method(&sess.api.new_array, &[bty, HEAP]).to(1, 0), Call::method(&sess.api.gc_alloc, &[0]).needs(0)];
        let res = sess.r.groups(&sess.m, &[g])?;
        let (Some(heap), Some(h)) = (res[0][0].filter(|&v| v != 0), res[0][1]) else {
            sess.close();
            return Err("couldn't make scratch space".into());
        };
        sess.keep.push(h);
        sess.r.grow(heap + ARR_DATA, HEAP);
        sess.s.base = sess.r.string(&sess.m, "_BaseColor").ok_or("scratch full")?;
        sess.s.emission = sess.r.string(&sess.m, "_EmissionColor").ok_or("scratch full")?;
        Ok(sess)
    }

    fn template(&mut self, from: u64) -> Result<(), String> {
        let (api, s, shader) = (&self.api, &self.s, self.shader);
        let g = vec![Call::method(&api.get_mat, &[from]).alive(from).to(1, 0), Call::method(&api.instantiate, &[0]).needs(0).to(2, 0), Call::method(&api.gc_alloc, &[0]).needs(0)];
        let res = self.r.groups(&self.m, &[g])?;
        let (Some(tpl), Some(h)) = (res[0][1].filter(|&v| v != 0), res[0][2]) else { return Err("couldn't clone a material".into()) };
        self.keep.push(h);
        let f = |name: u64, v: f32| Call::method(&api.set_float, &[tpl, name, remote::f32a(v)]).alive(tpl);
        self.r.each(
            &self.m,
            &[
                Call::method(&api.set_shader, &[tpl, shader]).alive(tpl).alive(shader),
                f(s.ztest, ZTEST_ALWAYS),
                f(s.zwrite, 0.0),
                f(s.cull, 0.0),
                f(s.src, BLEND_SRC_ALPHA),
                f(s.dst, BLEND_ONE_MINUS_SRC_ALPHA),
                Call::method(&api.queue, &[tpl, QUEUE_OVERLAY]).alive(tpl),
                Call::method(&api.hide, &[tpl, DONT_UNLOAD]).alive(tpl),
            ],
        )?;
        self.tpl = tpl;
        Ok(())
    }

    fn glow_template(&mut self, from: u64) -> Result<(), String> {
        let api = &self.api;
        let g = vec![Call::method(&api.get_mat, &[from]).alive(from).to(1, 0), Call::method(&api.instantiate, &[0]).needs(0).to(2, 0), Call::method(&api.gc_alloc, &[0]).needs(0)];
        let res = self.r.groups(&self.m, &[g])?;
        let (Some(t), Some(h)) = (res[0][1].filter(|&v| v != 0), res[0][2]) else { return Err("couldn't clone a material".into()) };
        self.keep.push(h);
        let mut calls = vec![Call::method(&self.api.set_shader, &[t, self.lit]).alive(t).alive(self.lit)];
        let textures =
            ["_BaseMap", "_MainTex", "_BumpMap", "_MetallicGlossMap", "_SpecGlossMap", "_OcclusionMap", "_EmissionMap", "_ParallaxMap", "_DetailMask", "_DetailAlbedoMap", "_DetailNormalMap"];
        let off = [
            "_NORMALMAP",
            "_METALLICSPECGLOSSMAP",
            "_SPECGLOSSMAP",
            "_OCCLUSIONMAP",
            "_PARALLAXMAP",
            "_DETAIL_MULX2",
            "_DETAIL_SCALED",
            "_ALPHATEST_ON",
            "_SURFACE_TYPE_TRANSPARENT",
            "_SPECULARHIGHLIGHTS_OFF",
            "_ENVIRONMENTREFLECTIONS_OFF",
            "_RECEIVE_SHADOWS_OFF",
        ];
        let floats = [("_Metallic", 1.0), ("_Smoothness", 0.92), ("_Glossiness", 0.92), ("_Surface", 0.0), ("_SpecularHighlights", 1.0), ("_EnvironmentReflections", 1.0)];
        for n in textures {
            let s = self.r.string(&self.m, n).ok_or("scratch full")?;
            calls.push(Call::method(&self.api.set_tex, &[t, s, 0]).alive(t));
        }
        for n in off {
            let s = self.r.string(&self.m, n).ok_or("scratch full")?;
            calls.push(Call::method(&self.api.keyword_off, &[t, s]).alive(t));
        }
        let on = self.r.string(&self.m, "_EMISSION").ok_or("scratch full")?;
        calls.push(Call::method(&self.api.keyword_on, &[t, on]).alive(t));
        for (n, v) in floats {
            let s = self.r.string(&self.m, n).ok_or("scratch full")?;
            calls.push(Call::method(&self.api.set_float, &[t, s, remote::f32a(v)]).alive(t));
        }
        calls.push(Call::method(&self.api.hide, &[t, DONT_UNLOAD]).alive(t));
        self.r.each(&self.m, &calls)?;
        self.glow_tpl = t;
        Ok(())
    }

    fn paint(&self, kind: Kind, mat: u64, c: [f32; 4]) -> Vec<Call> {
        match kind {
            Kind::Enemy => vec![set_color(&self.api, self.s.color, mat, c)],
            Kind::Body | Kind::Weapon => {
                vec![set_color(&self.api, self.s.base, mat, [c[0], c[1], c[2], 1.0]), set_color(&self.api, self.s.emission, mat, [c[0] * c[3], c[1] * c[3], c[2] * c[3], 1.0])]
            }
        }
    }

    fn wanted(cfg: &Cfg, e: &Ent) -> bool {
        cfg.on && !e.team && (!e.npc || cfg.npcs) && (!e.dead || cfg.dead)
    }

    fn color(cfg: &Cfg, e: &Ent) -> [f32; 4] {
        if e.dead {
            DEAD
        } else if e.vis != 0 {
            cfg.vis
        } else {
            cfg.hid
        }
    }

    fn sync(&mut self, cfg: &Cfg, ents: &[Ent], local: u64) -> Result<(), String> {
        let now = Instant::now();
        let by_id: HashMap<u64, &Ent> = ents.iter().map(|e| (e.id, e)).collect();

        let own = [(Kind::Body, cfg.local, local | 1), (Kind::Weapon, cfg.weapon, local | 2)];
        for (id, p) in self.players.iter_mut() {
            if by_id.contains_key(id) || (p.kind != Kind::Enemy && local != 0 && p.src == local) {
                p.seen = now;
            }
        }

        let drop: Vec<u64> = self
            .players
            .iter()
            .filter(|(id, p)| {
                p.seen.elapsed() > GONE
                    || match p.kind {
                        Kind::Enemy => !cfg.on || by_id.get(id).is_some_and(|e| !Self::wanted(cfg, e)),
                        k => !own.iter().any(|o| o.0 == k && o.1 && o.2 == **id),
                    }
            })
            .map(|(id, _)| *id)
            .collect();
        if !drop.is_empty() {
            self.restore(&drop)?;
        }
        let look = cfg.glow_at(self.born.elapsed().as_secs_f32());
        if local != 0 {
            for (kind, on, key) in own {
                if on && !self.players.contains_key(&key) {
                    self.add(key, local, kind, look)?;
                }
            }
        }
        if cfg.on {
            let new: Vec<&Ent> = ents.iter().filter(|e| !self.players.contains_key(&e.id) && Self::wanted(cfg, e)).take(NEW_PER_PASS).collect();
            for e in new {
                self.add(e.id, e.id, Kind::Enemy, Self::color(cfg, e))?;
            }
        }

        let stale: Vec<u64> = self.players.iter().filter(|(_, p)| p.scanned.elapsed() > RESCAN && p.seen == now).map(|(id, _)| *id).collect();
        for id in stale {
            self.rescan(id)?;
        }
        let recolor: Vec<(u64, [f32; 4])> = self
            .players
            .iter()
            .filter_map(|(id, p)| {
                let c = if p.kind == Kind::Enemy { Self::color(cfg, by_id.get(id)?) } else { look };
                (c != p.color).then_some((*id, c))
            })
            .collect();
        if !recolor.is_empty() {
            let calls: Vec<Call> = recolor.iter().flat_map(|(id, c)| self.paint(self.players[id].kind, self.players[id].mat, *c)).collect();
            self.r.each(&self.m, &calls)?;
            for &(id, c) in &recolor {
                self.players.get_mut(&id).unwrap().color = c;
            }
            let ids: Vec<u64> = recolor.iter().map(|r| r.0).collect();
            self.reclaim(&ids)?;
        }
        Ok(())
    }

    fn strays(&self, p: &Player) -> Vec<(u64, usize)> {
        let Some(off) = self.list else { return vec![] };
        let id = self.m.u32(self.m.q(p.mat + remote::CACHED_PTR) + NAT_ID);
        p.rends
            .iter()
            .filter(|r| {
                let rn = self.m.q(r.rd + remote::CACHED_PTR);
                let (data, size) = (self.m.q(rn + off), self.m.q(rn + off + LIST_SIZE));
                mem::ok(rn) && (size != r.n as u64 || (0..r.n as u64).any(|i| self.m.u32(data + i * 4) != id))
            })
            .map(|r| (r.rd, r.n))
            .collect()
    }

    fn reclaim(&mut self, ids: &[u64]) -> Result<(), String> {
        let api = &self.api;
        let mut strays = vec![];
        for id in ids {
            let Some(p) = self.players.get(id) else { continue };
            strays.extend(self.strays(p).into_iter().map(|(rd, n)| (*id, rd, n)));
        }

        let groups: Vec<Vec<Call>> = strays
            .iter()
            .map(|&(_, rd, _)| {
                vec![
                    Call::method(&api.get_mat, &[rd]).alive(rd).to(1, 0),
                    Call::method(&api.get_shader, &[0]).needs(0),
                    Call::method(&api.get_mat, &[rd]).alive(rd).to(3, 0),
                    Call::method(&api.gc_alloc, &[0]).needs(0),
                ]
            })
            .collect();
        let res = self.r.groups(&self.m, &groups)?;
        let mut calls = vec![];
        let mut frees = vec![];
        for (&(id, rd, n), g) in strays.iter().zip(&res) {
            let p = &self.players[&id];
            frees.extend(g[3].map(|h| free(api, h)));
            match (g[0].filter(|&v| v != 0), g[1], g[3]) {
                (Some(copy), Some(sh), Some(_)) if sh == p.shader && n == 1 && g[2] == Some(copy) => calls.extend(self.paint(p.kind, copy, p.color)),
                _ => calls.extend(p.fills.get(&n).map(|f| Call::method(&api.set_mats, &[rd, f.0]).alive(rd).alive(p.mat))),
            }
        }
        self.r.each(&self.m, &calls)?;
        self.r.each(&self.m, &frees)?;
        Ok(())
    }

    fn find_list(&mut self, mat: u64, rends: &[Rend]) {
        let id = self.m.u32(self.m.q(mat + remote::CACHED_PTR) + NAT_ID);
        let hit = |r: &Rend, off: u64| {
            let rn = self.m.q(r.rd + remote::CACHED_PTR);
            let data = self.m.q(rn + off);
            mem::ok(rn) && mem::ok(data) && r.n > 0 && self.m.q(rn + off + LIST_SIZE) == r.n as u64 && (0..r.n as u64).all(|i| self.m.u32(data + i * 4) == id)
        };
        let sample: Vec<&Rend> = rends.iter().take(8).collect();
        self.list = (0..0x400u64).step_by(8).find(|&off| sample.len() >= 2 && sample.iter().all(|r| hit(r, off)));
        match self.list {
            Some(off) => eprintln!("chams: native material list at renderer+{off:#x}"),
            None => eprintln!("chams: native material list not found, copies the game makes keep their color"),
        }
    }

    fn renderers(&mut self, ctrl: u64, kind: Kind) -> Result<Option<(u64, Vec<u64>)>, String> {
        let api = &self.api;
        let g = vec![Call::method(&api.game_object, &[ctrl]).alive(ctrl).to(1, 0), Call::method(&api.children, &[0, self.rty, 1]).needs(0).to(2, 0), Call::method(&api.gc_alloc, &[0]).needs(0)];
        let res = self.r.groups(&self.m, &[g])?;
        let (Some(arr), Some(h)) = (res[0][1].filter(|&v| v != 0), res[0][2]) else { return Ok(None) };
        let n = self.m.i32(arr + ARR_LEN).clamp(0, 512) as u64;
        let rends: Vec<u64> = (0..n)
            .map(|i| self.m.q(arr + ARR_DATA + i * 8))
            .filter(|&rd| mem::ok(rd) && matches!(il2::class_name(&self.m, self.m.q(rd)).as_deref(), Some("MeshRenderer" | "SkinnedMeshRenderer")))
            .collect();
        if kind == Kind::Enemy {
            return Ok(Some((h, rends)));
        }

        let unknown: Vec<u64> = rends.iter().copied().filter(|rd| !self.weapons.contains_key(rd)).collect();
        let calls: Vec<Call> = unknown.iter().map(|&rd| Call::method(&self.api.name, &[rd]).alive(rd)).collect();
        let names = self.r.each(&self.m, &calls)?;
        for (rd, n) in unknown.into_iter().zip(names) {
            self.weapons.insert(rd, n.and_then(|n| self.m.mstr(n)).is_some_and(|n| n.starts_with("Wpn_")));
        }
        let rends = rends.into_iter().filter(|rd| self.weapons.get(rd).copied().unwrap_or(false) == (kind == Kind::Weapon)).collect();
        Ok(Some((h, rends)))
    }

    fn add(&mut self, key: u64, src: u64, kind: Kind, color: [f32; 4]) -> Result<(), String> {
        let Some((arr_h, rds)) = self.renderers(src, kind)? else { return Ok(()) };
        let Some(&first) = rds.first() else {
            self.r.each(&self.m, &[free(&self.api, arr_h)])?;
            return Ok(());
        };
        let glow = kind != Kind::Enemy;
        if glow && self.glow_tpl == 0 {
            self.glow_template(first)?;
        } else if !glow && self.tpl == 0 {
            self.template(first)?;
        }
        let (tpl, shader) = if glow { (self.glow_tpl, self.lit) } else { (self.tpl, self.shader) };
        let api = &self.api;
        let g = vec![Call::method(&api.instantiate, &[tpl]).alive(tpl).to(1, 0), Call::method(&api.gc_alloc, &[0]).needs(0)];
        let res = self.r.groups(&self.m, &[g])?;
        let (Some(mat), Some(mat_h)) = (res[0][0].filter(|&v| v != 0), res[0][1]) else {
            self.r.each(&self.m, &[free(api, arr_h)])?;
            return Err("couldn't make a material".into());
        };
        let mut calls = self.paint(kind, mat, color);
        calls.push(Call::method(&self.api.hide, &[mat, DONT_UNLOAD]).alive(mat));
        self.r.each(&self.m, &calls)?;
        let now = Instant::now();
        let mut p = Player { kind, src, shader, mat, mat_h, arr_h, rends: vec![], fills: HashMap::new(), color, seen: now, scanned: now };
        p.rends = self.cover(mat, &mut p.fills, &rds)?;
        if self.list.is_none() && p.rends.len() >= 2 {
            self.find_list(mat, &p.rends);
        }
        self.players.insert(key, p);
        Ok(())
    }

    fn cover(&mut self, mat: u64, fills: &mut HashMap<usize, (u64, u64)>, rds: &[u64]) -> Result<Vec<Rend>, String> {
        let api = &self.api;
        let groups: Vec<Vec<Call>> = rds.iter().map(|&rd| vec![Call::method(&api.get_mats, &[rd]).alive(rd).to(1, 0), Call::method(&api.gc_alloc, &[0]).needs(0)]).collect();
        let res = self.r.groups(&self.m, &groups)?;
        let mut rends = vec![];
        let mut frees = vec![];
        for (&rd, g) in rds.iter().zip(&res) {
            match (g[0].filter(|&v| v != 0), g[1]) {
                (Some(orig), Some(orig_h)) => rends.push(Rend { rd, orig, orig_h, n: self.m.i32(orig + ARR_LEN).clamp(0, 64) as usize }),
                (_, Some(h)) => frees.push(free(api, h)),
                _ => {}
            }
        }
        self.r.each(&self.m, &frees)?;

        let mut need: Vec<usize> = rends.iter().map(|r| r.n).filter(|n| *n > 0 && !fills.contains_key(n)).collect();
        need.sort_unstable();
        need.dedup();
        let groups: Vec<Vec<Call>> = need.iter().map(|&n| vec![Call::method(&api.new_array, &[self.mty, n as u64]).to(1, 0), Call::method(&api.gc_alloc, &[0]).needs(0)]).collect();
        let res = self.r.groups(&self.m, &groups)?;
        for (&n, g) in need.iter().zip(&res) {
            let (Some(arr), Some(h)) = (g[0].filter(|&v| v != 0), g[1]) else { continue };
            let fill: Vec<u8> = std::iter::repeat_n(mat.to_le_bytes(), n).flatten().collect();
            if self.m.i32(arr + ARR_LEN) as usize == n && self.m.write(arr + ARR_DATA, &fill) {
                fills.insert(n, (arr, h));
            } else {
                self.r.each(&self.m, &[free(api, h)])?;
            }
        }
        let sets: Vec<Call> = rends.iter().filter_map(|r| Some(Call::method(&api.set_mats, &[r.rd, fills.get(&r.n)?.0]).alive(r.rd).alive(mat))).collect();
        self.r.each(&self.m, &sets)?;
        Ok(rends)
    }

    fn rescan(&mut self, id: u64) -> Result<(), String> {
        let (src, kind) = (self.players[&id].src, self.players[&id].kind);
        let Some((arr_h, rds)) = self.renderers(src, kind)? else {
            self.players.get_mut(&id).unwrap().scanned = Instant::now();
            return Ok(());
        };
        let p = self.players.get_mut(&id).unwrap();
        let (mat, old_h) = (p.mat, p.arr_h);

        let (keep, left): (Vec<Rend>, Vec<Rend>) = std::mem::take(&mut p.rends).into_iter().partition(|r| rds.contains(&r.rd));
        let fresh: Vec<u64> = rds.into_iter().filter(|rd| !keep.iter().any(|r| r.rd == *rd)).collect();
        p.rends = keep;
        let mut calls: Vec<Call> = left.iter().map(|r| Call::method(&self.api.set_mats, &[r.rd, r.orig]).alive(r.rd)).collect();
        calls.extend(left.iter().map(|r| free(&self.api, r.orig_h)));
        calls.push(free(&self.api, old_h));
        for r in &left {
            self.weapons.remove(&r.rd);
        }
        let mut fills = std::mem::take(&mut self.players.get_mut(&id).unwrap().fills);
        let more = if fresh.is_empty() { Ok(vec![]) } else { self.cover(mat, &mut fills, &fresh) };
        let p = self.players.get_mut(&id).unwrap();
        p.fills = fills;
        p.rends.extend(more?);
        (p.arr_h, p.scanned) = (arr_h, Instant::now());
        self.r.each(&self.m, &calls)?;

        self.reclaim(&[id])
    }

    fn restore(&mut self, ids: &[u64]) -> Result<(), String> {
        let api = &self.api;
        let mut sets = vec![];
        let mut after = vec![];
        for id in ids {
            let Some(p) = self.players.remove(id) else { continue };
            for r in &p.rends {
                sets.push(Call::method(&api.set_mats, &[r.rd, r.orig]).alive(r.rd));
                after.push(free(api, r.orig_h));
            }
            after.push(free(api, p.arr_h));
            after.extend(p.fills.values().map(|&(_, h)| free(api, h)));
            after.push(Call::method(&api.destroy, &[p.mat]).alive(p.mat));
            after.push(free(api, p.mat_h));
        }
        self.r.each(&self.m, &sets)?;
        self.r.each(&self.m, &after)?;
        Ok(())
    }

    fn close(&mut self) {
        let ids: Vec<u64> = self.players.keys().copied().collect();
        let _ = self.restore(&ids);
        let mut last = vec![];
        for t in [self.tpl, self.glow_tpl] {
            if t != 0 {
                last.push(Call::method(&self.api.destroy, &[t]).alive(t));
            }
        }
        last.extend(self.keep.iter().map(|&h| free(&self.api, h)));
        let _ = self.r.each(&self.m, &last);
        self.r.uninstall(&self.m);
    }
}

pub fn test() {
    let Some(m) = mem::find_pid().and_then(|p| Mem::open(p).ok()) else { return eprintln!("GoreBox not running") };
    let want: Vec<&str> = remote::WANT.iter().chain(WANT).copied().collect();
    let il = Il2::scan(&m, &want);
    let api = match Api::resolve(&m, &il) {
        Ok(a) => a,
        Err(e) => return eprintln!("api: {e}"),
    };
    let mut r = match Remote::install(&m, &il) {
        Ok(r) => r,
        Err(e) => return eprintln!("install: {e}"),
    };
    let h0 = r.hits(&m);
    std::thread::sleep(std::time::Duration::from_millis(300));
    println!("hook in, Update ran {} times in 300ms", r.hits(&m) - h0);
    if let Err(e) = run_test(&m, &api, &mut r) {
        println!("test: {e}");
    }
    r.uninstall(&m);
    println!("hook out");
}

fn run_test(m: &Mem, api: &Api, r: &mut Remote) -> Result<(), String> {
    let name = |r: &mut Remote, o: u64| -> Result<String, String> { Ok(r.call(m, Call::method(&api.name, &[o]).alive(o))?.and_then(|s| m.mstr(s)).unwrap_or_else(|| "?".into())) };
    let shader_name = r.string(m, "Hidden/Internal-Colored").ok_or("scratch full")?;
    let res = r.batch(m, &[Call::method(&api.find, &[shader_name]), Call::method(&api.type_of, &[api.renderer_ty]), Call::method(&api.type_of, &[api.material_ty])])?;
    let (shader, rty, mty) = (res[0].unwrap_or(0), res[1].unwrap_or(0), res[2].unwrap_or(0));
    println!("Shader.Find -> {shader:#x} (native {:#x})  typeof(Renderer) {rty:#x}  typeof(Material) {mty:#x}", m.q(shader + remote::CACHED_PTR));
    if shader != 0 {
        println!("  shader name: {}", name(r, shader)?);
    }
    let ts = crate::game::targets(m)?;
    println!("{} targets", ts.len());
    let Some((ctrl, who, npc)) = ts.iter().find(|t| !t.2).or(ts.first()).cloned() else { return Ok(()) };
    let go = r.call(m, Call::method(&api.game_object, &[ctrl]).alive(ctrl))?.ok_or("controller gone")?;
    let root_tf = r.call(m, Call::method(&api.transform, &[go]).alive(go))?.ok_or("go gone")?;
    let root_tf = r.call(m, Call::method(&api.root, &[root_tf]).alive(root_tf))?.ok_or("tf gone")?;
    let root = r.call(m, Call::method(&api.game_object, &[root_tf]).alive(root_tf))?.ok_or("root gone")?;
    println!("{who} (npc {npc}): controller object '{}', root '{}'", name(r, go)?, name(r, root)?);
    for (label, obj) in [("controller", go), ("root", root)] {
        let arr = r.call(m, Call::method(&api.children, &[obj, rty, 1]).alive(obj))?.unwrap_or(0);
        let n = m.i32(arr + ARR_LEN).max(0) as u64;
        println!("  renderers under {label}: {n}");
        for i in 0..n.min(24) {
            let rd = m.q(arr + ARR_DATA + i * 8);
            let mats = r.call(m, Call::method(&api.get_mats, &[rd]).alive(rd))?.unwrap_or(0);
            let cls = il2::class_name(m, m.q(rd)).unwrap_or_default();
            println!("    {cls} '{}' {} materials", name(r, rd)?, m.i32(mats + ARR_LEN));
        }
    }
    Ok(())
}

pub fn debug() {
    let Some(m) = mem::find_pid().and_then(|p| Mem::open(p).ok()) else { return eprintln!("GoreBox not running") };
    let mut s = match Session::open(m.pid) {
        Ok(s) => s,
        Err(e) => return eprintln!("open: {e}"),
    };
    if let Err(e) = debug_run(&m, &mut s) {
        println!("debug: {e}");
    }
    s.close();
    println!("restored, unhooked");
}

fn debug_run(m: &Mem, s: &mut Session) -> Result<(), String> {
    let cfg = Cfg::default();
    let ts = crate::game::targets(m)?;
    for t in ts.iter().take(6) {
        s.add(t.0, t.0, Kind::Enemy, cfg.vis)?;
    }
    let name = |s: &mut Session, o: u64| -> String { s.r.call(m, Call::method(&s.api.name, &[o]).alive(o)).ok().flatten().and_then(|v| m.mstr(v)).unwrap_or_else(|| "?".into()) };
    let ids: Vec<u64> = s.players.keys().copied().collect();
    for &id in &ids {
        let mat = s.players[&id].mat;
        println!("player {id:#x}: {} renderers, material {}", s.players[&id].rends.len(), name(s, mat));
    }
    for round in 0..10 {
        std::thread::sleep(Duration::from_secs(2));
        let c = if round % 2 == 0 { cfg.hid } else { cfg.vis };
        let strays = |s: &Session| ids.iter().map(|id| s.strays(&s.players[id]).len()).sum::<usize>();
        let before = strays(s);
        let calls: Vec<Call> = ids.iter().map(|id| set_color(&s.api, s.s.color, s.players[id].mat, c)).collect();
        s.r.each(m, &calls)?;
        for id in &ids {
            s.players.get_mut(id).unwrap().color = c;
        }
        s.reclaim(&ids)?;

        println!("round {round}: {before} renderers on a copy before, {} after recoloring", strays(s));
    }
    Ok(())
}

pub fn explore() {
    let Some(m) = mem::find_pid().and_then(|p| Mem::open(p).ok()) else { return eprintln!("GoreBox not running") };
    let mut s = match Session::open(m.pid) {
        Ok(s) => s,
        Err(e) => return eprintln!("open: {e}"),
    };
    if let Err(e) = explore_run(&m, &mut s) {
        println!("explore: {e}");
    }
    s.close();
}

fn explore_run(m: &Mem, s: &mut Session) -> Result<(), String> {
    let il = Il2::scan(m, &["UnityEngine.Resources", "UnityEngine.Shader"]);
    let k = |n: &str| il.class(n).ok_or(format!("no {n}"));
    let all = il2::method(m, k("UnityEngine.Resources")?, "FindObjectsOfTypeAll", &[(TE_CLASS, 0)]).ok_or("no FindObjectsOfTypeAll")?;
    let names = |s: &mut Session, objs: &[u64]| -> Result<Vec<String>, String> {
        let calls: Vec<Call> = objs.iter().map(|&o| Call::method(&s.api.name, &[o]).alive(o)).collect();
        Ok(s.r.each(m, &calls)?.into_iter().map(|v| v.and_then(|v| m.mstr(v)).unwrap_or_else(|| "?".into())).collect())
    };
    let arr = |a: u64| -> Vec<u64> { (0..m.i32(a + ARR_LEN).clamp(0, 4096) as u64).map(|i| m.q(a + ARR_DATA + i * 8)).collect() };
    let sty = s.r.call(m, Call::method(&s.api.type_of, &[k("UnityEngine.Shader")? + K_TYPE]))?.unwrap_or(0);
    let shaders = s.r.call(m, Call::method(&all, &[sty]))?.unwrap_or(0);
    let mut list = names(s, &arr(shaders))?;
    list.sort();
    list.dedup();
    println!("{} shaders:", list.len());
    for n in &list {
        println!("  {n}");
    }
    let (local, item, cam) = crate::game::local(m)?;
    println!("local {local:#x} item switcher {item:#x} aim camera {cam:#x}");
    for (label, comp) in [("local player", local), ("aim camera", cam)] {
        if comp == 0 {
            continue;
        }
        let go = s.r.call(m, Call::method(&s.api.game_object, &[comp]).alive(comp))?.unwrap_or(0);
        let tf = s.r.call(m, Call::method(&s.api.transform, &[go]).alive(go))?.unwrap_or(0);
        let root_tf = s.r.call(m, Call::method(&s.api.root, &[tf]).alive(tf))?.unwrap_or(0);
        let root = s.r.call(m, Call::method(&s.api.game_object, &[root_tf]).alive(root_tf))?.unwrap_or(0);
        for (what, obj) in [("own object", go), ("root", root)] {
            let rs = s.r.call(m, Call::method(&s.api.children, &[obj, s.rty, 1]).alive(obj))?.unwrap_or(0);
            let rds = arr(rs);
            let rn = names(s, &rds)?;
            println!("{label} {what} '{}': {} renderers", names(s, &[obj])?[0], rds.len());
            let mats: Vec<u64> = s.r.each(m, &rds.iter().map(|&rd| Call::method(&s.api.get_mat, &[rd]).alive(rd)).collect::<Vec<_>>())?.into_iter().map(|v| v.unwrap_or(0)).collect();
            let shs: Vec<u64> = s.r.each(m, &mats.iter().map(|&mt| Call::method(&s.api.get_shader, &[mt]).alive(mt)).collect::<Vec<_>>())?.into_iter().map(|v| v.unwrap_or(0)).collect();
            let (mn, sn) = (names(s, &mats)?, names(s, &shs)?);
            for i in 0..rds.len().min(60) {
                let cls = il2::class_name(m, m.q(rds[i])).unwrap_or_default();
                println!("    {cls} '{}'  material '{}'  shader '{}'", rn[i], mn[i], sn[i]);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::hsv;

    #[test]
    fn hue_wheel() {
        let near = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5);
        assert!(near(hsv(0.0, 1.0, 1.0), [1.0, 0.0, 0.0]));
        assert!(near(hsv(1.0 / 3.0, 1.0, 1.0), [0.0, 1.0, 0.0]));
        assert!(near(hsv(2.0 / 3.0, 1.0, 1.0), [0.0, 0.0, 1.0]));
        assert!(near(hsv(0.5, 0.0, 0.8), [0.8, 0.8, 0.8]));
    }
}
