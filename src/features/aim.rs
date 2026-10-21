use crate::game::{Ent, LookNow, Shared, Snap, V3};
use crate::os::uinput::Mouse;
use crate::ui::x11::X;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Rmb,
    Lmb,
    Mmb,
    LAlt,
    LShift,
    Caps,
}

impl Key {
    pub const ALL: [Key; 6] = [Key::Rmb, Key::Lmb, Key::Mmb, Key::LAlt, Key::LShift, Key::Caps];

    pub fn label(self) -> &'static str {
        match self {
            Key::Rmb => "right mouse",
            Key::Lmb => "left mouse",
            Key::Mmb => "middle mouse",
            Key::LAlt => "left alt",
            Key::LShift => "left shift",
            Key::Caps => "caps lock",
        }
    }

    fn held(self, x: &X) -> bool {
        let sym = match self {
            Key::Rmb => return x.buttons() & 0x400 != 0,
            Key::Lmb => return x.buttons() & 0x100 != 0,
            Key::Mmb => return x.buttons() & 0x200 != 0,
            Key::LAlt => 0xffe9,
            Key::LShift => 0xffe1,
            Key::Caps => 0xffe5,
        };
        X::down(&x.keys(), x.kc(sym))
    }
}

#[derive(Clone, Copy)]
pub struct Cfg {
    pub on: bool,
    pub key: Key,
    pub fov: f32,
    pub smooth: f32,
    pub bone: usize,
    pub players: bool,
    pub npcs: bool,
    pub vis_only: bool,
    pub max_dist: f32,
    pub draw_fov: bool,
    pub silent: bool,

    pub team_check: bool,

    pub method: Method,

    pub hit: [u8; 5],
    pub magic_fov: f32,

    pub head_vis: bool,

    pub aa: crate::features::aa::Cfg,

    pub rage: bool,

    pub predict: f32,

    pub aura: bool,
    pub aura_range: f32,
    pub auto_punch: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Method {
    Decoy,
    Magic,
}

impl Method {
    pub const ALL: [Method; 2] = [Method::Decoy, Method::Magic];

    pub fn label(self) -> &'static str {
        match self {
            Method::Decoy => "camera",
            Method::Magic => "magic bullet",
        }
    }
}

impl Cfg {
    pub fn magic(&self) -> bool {
        self.silent && self.method == Method::Magic
    }

    pub fn effective(&self) -> Cfg {
        if !self.rage {
            return *self;
        }
        Cfg { silent: true, method: Method::Decoy, fov: RAGE_FOV, vis_only: true, head_vis: true, ..*self }
    }
}

pub const RAGE_FOV: f32 = 360.0;

impl Default for Cfg {
    fn default() -> Cfg {
        Cfg {
            on: false,
            key: Key::Rmb,
            fov: 8.0,
            smooth: 4.0,
            bone: crate::game::HEAD,
            players: true,
            npcs: true,
            vis_only: true,
            max_dist: 300.0,
            draw_fov: true,
            silent: false,
            team_check: true,
            method: Method::Decoy,
            hit: [100, 0, 0, 0, 0],
            magic_fov: 20.0,
            head_vis: true,
            aa: crate::features::aa::Cfg::default(),
            rage: false,
            predict: 0.0,
            aura: false,
            aura_range: 3.5,
            auto_punch: true,
        }
    }
}

#[derive(Default)]
pub struct State {
    pub target: u64,
    pub silent: u64,
    pub silent_bone: usize,
    pub decoy: String,
    pub status: String,
    pub calib: Option<(f32, f32)>,

    pub magic: u64,
    pub moved: u64,

    pub reach: f32,
    pub host: bool,

    pub ping: f32,
    pub lead_ms: f32,

    pub rage_shots: u64,
    pub aa: String,
}

#[derive(Default)]
pub struct Aim {
    pub cfg: Mutex<Cfg>,
    pub state: Mutex<State>,
    pub calibrate: AtomicBool,
    pub menu: AtomicBool,

    pub focused: AtomicBool,
}

impl Aim {
    fn say(&self, s: impl Into<String>) {
        self.state.lock().unwrap().status = s.into();
    }
}

pub fn spawn(sh: Arc<Shared>, aim: Arc<Aim>) {
    std::thread::Builder::new().name("aim".into()).spawn(move || run(sh, aim)).unwrap();
}

fn calib_path() -> PathBuf {
    crate::settings::dir().join("aim.txt")
}

fn load() -> Option<(f32, f32)> {
    let s = std::fs::read_to_string(calib_path()).ok()?;
    let mut it = s.split_whitespace().map(|v| v.parse::<f32>().ok());
    Some((it.next()??, it.next()??))
}

fn save(k: (f32, f32)) {
    let p = calib_path();
    let _ = std::fs::create_dir_all(p.parent().unwrap());
    let _ = std::fs::write(p, format!("{} {}\n", k.0, k.1));
}

fn run(sh: Arc<Shared>, aim: Arc<Aim>) {
    let Some(x) = X::new() else { return aim.say("no X11 display") };
    let mut mouse = match Mouse::new() {
        Ok(m) => m,
        Err(e) => return aim.say(format!("uinput: {e}")),
    };
    let mut k = load();
    aim.state.lock().unwrap().calib = k;
    aim.say(if k.is_some() { "ready" } else { "calibrate first" });

    let (mut focus, mut focus_at) = (false, Instant::now() - Duration::from_secs(1));
    let mut last_view = [0f32; 16];
    let mut acted = Instant::now();
    let mut acc = (0f32, 0f32);
    let mut lock = 0u64;
    let mut failed = Instant::now() - Duration::from_secs(60);
    loop {
        sleep(Duration::from_millis(1));
        let cfg = *aim.cfg.lock().unwrap();
        let snap = sh.pick(Duration::ZERO);

        if focus_at.elapsed() >= Duration::from_millis(100) {
            focus_at = Instant::now();
            focus = x.game_win(snap.pid).is_some_and(|w| x.active() == Some(w));
        }
        let ingame = focus && snap.ok && !aim.menu.load(Ordering::Relaxed);

        let auto = k.is_none() && cfg.on && failed.elapsed() > Duration::from_secs(3) && cfg.key.held(&x);
        let wanted = aim.calibrate.load(Ordering::Relaxed) || auto;
        if wanted && !ingame {
            aim.say(if k.is_none() { "calibrate first: hold the aim key in game" } else { "close the menu to calibrate" });
        }
        if wanted && ingame {
            aim.calibrate.store(false, Ordering::Relaxed);
            aim.say("calibrating, keep the mouse still");
            sleep(Duration::from_millis(400));
            match calibrate(&mut mouse, &sh) {
                Some(c) => {
                    k = Some(c);
                    save(c);
                    aim.state.lock().unwrap().calib = k;
                    aim.say("calibrated");
                }
                None => {
                    failed = Instant::now();
                    aim.say("calibration failed: look straight ahead, stand still, try again");
                }
            }
            continue;
        }

        if !cfg.on || !ingame {
            release(&aim, &mut lock);
            continue;
        }
        let (Some(look), Some((kx, ky))) = (snap.look, k) else { continue };
        if !cfg.key.held(&x) {
            release(&aim, &mut lock);
            acc = (0.0, 0.0);
            continue;
        }

        let fresh = snap.view != last_view;
        if !fresh && acted.elapsed() < Duration::from_millis(50) {
            continue;
        }

        let Some(t) = pick(&snap, &cfg, lock, &[cfg.bone]) else {
            release(&aim, &mut lock);
            continue;
        };
        lock = t.id;
        aim.state.lock().unwrap().target = t.id;
        let Some(p) = spot(t, cfg.bone) else { continue };

        let (ey, ep) = error(&snap, lead(&snap, t, p));
        let (py, pp) = pending(&look);
        let step = 1.0 / cfg.smooth.max(1.0);
        let sens = look.sens.max(1e-3);
        acc.0 += (ey - py) * step / (kx * sens);
        acc.1 += (ep - pp) * step / (ky * sens);
        let (dx, dy) = (acc.0.trunc() as i32, acc.1.trunc() as i32);
        acc.0 -= dx as f32;
        acc.1 -= dy as f32;
        let _ = mouse.rel(dx.clamp(-400, 400), dy.clamp(-400, 400));
        last_view = snap.view;
        acted = Instant::now();
    }
}

fn release(aim: &Aim, lock: &mut u64) {
    *lock = 0;
    aim.state.lock().unwrap().target = 0;
}

pub fn facing(view: &[f32; 16]) -> (f32, f32) {
    let f = [-view[2], -view[6], -view[10]];
    (f[0].atan2(f[2]).to_degrees(), f[1].clamp(-1.0, 1.0).asin().to_degrees())
}

fn wrap(a: f32) -> f32 {
    (a + 540.0).rem_euclid(360.0) - 180.0
}

pub fn error(s: &Snap, p: V3) -> (f32, f32) {
    let (cy, cp) = facing(&s.view);
    let d = [p[0] - s.cam[0], p[1] - s.cam[1], p[2] - s.cam[2]];
    let yaw = d[0].atan2(d[2]).to_degrees();
    let pitch = d[1].atan2((d[0] * d[0] + d[2] * d[2]).sqrt()).to_degrees();
    (wrap(yaw - cy), pitch - cp)
}

fn pending(l: &LookNow) -> (f32, f32) {
    (wrap(l.yaw_t - l.yaw_a), l.pitch_t - l.pitch_a)
}

pub const BRAIN: f32 = 0.07;
pub const SKULL_TOP: f32 = 0.2;

pub fn spot(e: &Ent, bone: usize) -> Option<V3> {
    use crate::game::{HEAD, NECK};
    let (n, h) = (e.bones[NECK], e.bones[HEAD]);
    match bone {
        HEAD => h.map(|h| add(h, mul(head_up(e), BRAIN))),
        NECK => match (n, h) {
            (Some(n), Some(h)) => Some(mul(add(n, h), 0.5)),
            _ => n.or(h),
        },
        _ => e.bones[bone].or(h),
    }
}

pub fn head_up(e: &Ent) -> V3 {
    use crate::game::{HEAD, NECK};
    match (e.bones[NECK], e.bones[HEAD]) {
        (Some(n), Some(h)) if len(sub(h, n)) > 1e-3 => mul(sub(h, n), 1.0 / len(sub(h, n))),
        _ => [0.0, 1.0, 0.0],
    }
}

pub fn lead(s: &Snap, e: &Ent, p: V3) -> V3 {
    lead_from(s, e, p, s.cam)
}

pub fn lead_from(s: &Snap, e: &Ent, p: V3, o: V3) -> V3 {
    let Some(b) = s.ball else { return p };
    let (g, k) = ([0.0, -9.81 * b.grav, 0.0], b.drag.max(0.0));
    let d0 = len(sub(p, o));
    let (mut t, mut u) = (d0 / b.speed, sub(p, o));
    for _ in 0..6 {
        let f = add(p, mul(e.vel, t));

        let v0 = if k > 1e-4 {
            let a = (1.0 - (-k * t).exp()) / k;
            add(mul(g, 1.0 / k), mul(sub(sub(f, o), mul(g, t / k)), 1.0 / a))
        } else {
            sub(mul(sub(f, o), 1.0 / t), mul(g, t * 0.5))
        };
        u = sub(v0, mul(s.cam_vel, b.inherit));

        t = (t * len(u) / b.speed).clamp(1e-4, 5.0);
    }
    add(o, mul(u, d0 / len(u).max(1e-6)))
}

fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn mul(a: V3, s: f32) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn len(a: V3) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

pub fn pick<'a>(s: &'a Snap, cfg: &Cfg, lock: u64, parts: &[usize]) -> Option<&'a Ent> {
    let off = |e: &Ent| {
        parts
            .iter()
            .filter(|&&b| !cfg.vis_only || e.seen(b))
            .filter_map(|&b| spot(e, b))
            .map(|p| {
                let (y, p) = error(s, p);
                (y * y + p * p).sqrt()
            })
            .min_by(f32::total_cmp)
    };
    let ok = |e: &Ent| !e.dead && !(cfg.team_check && e.team) && (if e.npc { cfg.npcs } else { cfg.players }) && e.dist <= cfg.max_dist;

    if let Some(e) = s.ents.iter().find(|e| lock != 0 && e.id == lock && ok(e) && off(e).is_some_and(|o| o <= cfg.fov * 1.5)) {
        return Some(e);
    }
    candidates(s, cfg, parts).into_iter().next()
}

pub fn candidates<'a>(s: &'a Snap, cfg: &Cfg, parts: &[usize]) -> Vec<&'a Ent> {
    let off = |e: &Ent| {
        parts
            .iter()
            .filter(|&&b| !cfg.vis_only || e.seen(b))
            .filter_map(|&b| spot(e, b))
            .map(|p| {
                let (y, p) = error(s, p);
                (y * y + p * p).sqrt()
            })
            .min_by(f32::total_cmp)
    };
    let ok = |e: &Ent| !e.dead && !(cfg.team_check && e.team) && (if e.npc { cfg.npcs } else { cfg.players }) && e.dist <= cfg.max_dist;
    let mut c: Vec<(f32, &Ent)> = s.ents.iter().filter(|e| ok(e)).filter_map(|e| Some((off(e)?, e))).filter(|(o, _)| *o <= cfg.fov).collect();
    c.sort_by(|a, b| a.0.total_cmp(&b.0));
    c.into_iter().map(|(_, e)| e).collect()
}

fn calibrate(mouse: &mut Mouse, sh: &Shared) -> Option<(f32, f32)> {
    let read = || {
        sleep(Duration::from_millis(120));
        sh.pick(Duration::ZERO).look
    };
    let push = |mouse: &mut Mouse, dx: i32, dy: i32| {
        for _ in 0..10 {
            let _ = mouse.rel(dx / 10, dy / 10);
            sleep(Duration::from_millis(3));
        }
    };
    let a = read()?;
    push(mouse, 200, 0);
    let b = read()?;
    push(mouse, -200, 0);
    let c = read()?;
    push(mouse, 0, 100);
    let d = read()?;
    push(mouse, 0, -100);
    let sens = a.sens.max(1e-3);
    let kx = wrap(b.yaw_t - a.yaw_t) / 200.0 / sens;
    let ky = (d.pitch_t - c.pitch_t) / 100.0 / sens;
    let sane = |v: f32| v.is_finite() && (1e-5..1.0).contains(&v.abs());
    (sane(kx) && sane(ky)).then_some((kx, ky))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Ballistic;

    fn fly(s: &Snap, b: Ballistic, aim: V3, until: f32) -> V3 {
        let dir = mul(sub(aim, s.cam), 1.0 / len(sub(aim, s.cam)));
        let (mut x, mut v) = (s.cam, add(mul(dir, b.speed), mul(s.cam_vel, b.inherit)));
        let dt = 1e-4;
        let mut t = 0.0;
        while t < until {
            v = add(v, mul(sub([0.0, -9.81 * b.grav, 0.0], mul(v, b.drag)), dt));
            x = add(x, mul(v, dt));
            t += dt;
        }
        x
    }

    #[test]
    fn lead_hits_moving_target() {
        let b = Ballistic { speed: 380.0, drag: 0.05, grav: 1.0, inherit: 1.0 };
        let s = Snap { cam: [0.0, 1.6, 0.0], cam_vel: [2.0, 0.0, 1.0], ball: Some(b), ..Default::default() };
        let e = Ent { vel: [5.0, 0.0, -1.0], ..Default::default() };
        let p = [10.0, 1.5, 120.0];
        let aim = lead(&s, &e, p);

        let miss = |t: f32| len(sub(fly(&s, b, aim, t), add(p, mul(e.vel, t))));
        let (mut best, mut at) = (f32::MAX, 0.0);
        for i in 0..200 {
            let t = i as f32 * 0.005;
            if miss(t) < best {
                (best, at) = (miss(t), t);
            }
        }
        for i in -100..=100 {
            let t = at + i as f32 * 0.00005;
            if t > 0.0 && miss(t) < best {
                best = miss(t);
            }
        }
        assert!(best < 0.05, "missed by {best} m at t {at}");

        let none = len(sub(fly(&s, b, p, at), add(p, mul(e.vel, at))));
        assert!(none > 1.0, "no-lead miss {none}");
    }
}
