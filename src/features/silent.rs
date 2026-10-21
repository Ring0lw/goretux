use crate::features::aim::{self, Cfg};
use crate::game::sdk::{Sdk, nat};
use crate::game::undo;
use crate::game::vis::Bvh;
use crate::game::{CHEST, Ent, HEAD, NECK, PELVIS, STOMACH, Snap, V3, mul};
use crate::os::mem::{self, Mem};
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
pub struct Decoy {
    pub cam: u64,
    pub native: u64,
    pub trs: u64,
}

pub fn decoy_from(m: &Mem, s: &Sdk, cam_vt: u64) -> Option<Decoy> {
    let mut best: Option<(bool, Decoy)> = None;
    for o in mem::objects(m, s.k_cam) {
        let native = m.q(o + nat::CACHED_PTR);
        if m.q(o + 8) != 0 || !mem::ok(native) || m.q(native) != cam_vt {
            continue;
        }
        let go = m.q(native + nat::COMP_GO);
        let name = m.cstr(m.q(go + nat::GO_NAME), 64).unwrap_or_default();
        let tf = m.q(m.q(go + nat::GO_COMPS) + 8);
        let hier = m.q(tf + nat::TF_HIER);
        if m.i32(tf + nat::TF_INDEX) != 0 || m.u32(hier + nat::H_CAP) != 1 || name.contains("Reflection") {
            continue;
        }
        let d = Decoy { cam: o, native, trs: m.q(hier + nat::H_LOCAL) };
        let boot = name == "BootCamera";
        if best.is_none_or(|(b, _)| boot && !b) {
            best = Some((boot, d));
        }
    }
    best.map(|(_, d)| d)
}

pub const PARTS: [(usize, &str); 5] = [(HEAD, "head"), (NECK, "neck"), (CHEST, "chest"), (STOMACH, "stomach"), (PELVIS, "pelvis")];

const REROLL: Duration = Duration::from_millis(120);

pub const MAGIC_BACK: f32 = 0.5;

const REACH_MARGIN: f32 = 1.6;
const REACH_LAG: f32 = 0.15;

pub struct Reach<'a> {
    pub limit: f32,
    pub host: bool,
    pub bvh: Option<&'a Bvh>,

    pub delay: f32,
}

pub fn reach(s: &Snap, limit: f32) -> f32 {
    let speed = (s.cam_vel[0] * s.cam_vel[0] + s.cam_vel[1] * s.cam_vel[1] + s.cam_vel[2] * s.cam_vel[2]).sqrt();
    (limit - REACH_MARGIN - speed * REACH_LAG).max(0.0)
}

#[derive(Default)]
pub struct Silent {
    swapped: Option<(u64, u64, u64, u64)>,
    decoy_orig: Option<(Decoy, Vec<u8>)>,
    part: Option<(u64, usize, Instant)>,
    rng: u64,

    pub sure: bool,
}

impl Silent {
    #[allow(clippy::too_many_arguments)]
    pub fn tick(&mut self, m: &Mem, snap: &Snap, cfg: &Cfg, gun: u64, cam_off: u64, decoy: Option<Decoy>, r: Reach) -> (u64, usize) {
        let (Some(d), true) = (decoy, cfg.silent && snap.ok && mem::ok(gun) && cam_off != 0) else {
            self.release(m);
            return (0, cfg.bone);
        };

        let cfg = &if cfg.magic() { Cfg { fov: cfg.magic_fov, vis_only: false, ..*cfg } } else { *cfg };

        let parts: Vec<usize> = PARTS.iter().zip(cfg.hit).filter(|(_, w)| *w > 0).map(|((b, _), _)| *b).collect();
        let parts = if parts.is_empty() { vec![cfg.bone] } else { parts };
        let cands = if cfg.magic() { aim::candidates(snap, cfg, &parts) } else { aim::pick(snap, cfg, 0, &parts).into_iter().collect() };
        let mut chosen = None;

        let mut fallback = None;
        'targets: for t in cands {
            let rolled = match self.part {
                _ if (cfg.magic() || cfg.rage) && cfg.head_vis && t.seen(HEAD) => HEAD,
                Some((id, b, at)) if id == t.id && at.elapsed() < REROLL => b,
                _ => {
                    let seen = cfg.magic() && PARTS.iter().zip(cfg.hit).any(|((b, _), w)| w > 0 && t.seen(*b));
                    self.roll(&Cfg { vis_only: cfg.vis_only || seen, ..*cfg }, t)
                }
            };

            let others = parts.iter().copied().filter(|&b| cfg.magic() && b != rolled && (!cfg.vis_only || t.seen(b)));
            for bone in std::iter::once(rolled).chain(others) {
                if let Some((from, p)) = plan(snap, cfg, t, bone, &r) {
                    chosen = Some((t, bone, from, p));
                    break 'targets;
                }
            }
            if fallback.is_none() {
                fallback = camera_shot(snap, t, rolled, &r).map(|(from, p)| (t, rolled, from, p));
            }
        }

        self.sure = chosen.is_some_and(|(t, bone, ..)| cfg.magic() || t.seen(bone));
        let Some((t, bone, from, p)) = chosen.or(fallback) else {
            self.release(m);
            return (0, cfg.bone);
        };
        if self.part.is_none_or(|(id, b, _)| id != t.id || b != bone) {
            self.part = Some((t.id, bone, Instant::now()));
        }

        if self.decoy_orig.is_none() {
            self.decoy_orig = m.bytes(d.trs, 0x20).map(|b| (d, b));
        }
        if !point(m, &d, from, p) {
            self.release(m);
            return (0, bone);
        }

        let at = gun + cam_off;
        if self.swapped.is_some_and(|(g, ..)| g != gun) {
            self.release(m);
        }
        let cur = m.q(at);
        if cur != d.cam || self.swapped.is_none() {
            if self.swapped.is_none() {
                let orig = if cur == d.cam { snap.aim_cam } else { cur };
                if !mem::ok(orig) {
                    self.sure = false;
                    return (0, bone);
                }
                self.swapped = Some((gun, m.q(gun), at, orig));
                undo::keep(m.fd(), at, gun, m.q(gun), &orig.to_le_bytes());
            }
            m.write(at, &d.cam.to_le_bytes());
        }
        (t.id, bone)
    }

    fn roll(&mut self, cfg: &Cfg, e: &Ent) -> usize {
        roll(cfg, e, next(&mut self.rng))
    }

    pub fn release(&mut self, m: &Mem) {
        (self.part, self.sure) = (None, false);
        if let Some((gun, k, at, orig)) = self.swapped.take() {
            if m.q(gun) == k {
                m.write(at, &orig.to_le_bytes());
            }
            undo::forget(at);
        }
    }

    pub fn restore(&mut self, m: &Mem) {
        self.release(m);

        if let Some((d, b)) = self.decoy_orig.take()
            && live(m, &d)
        {
            m.write(d.trs, &b);
        }
    }

    pub fn idle(&self) -> bool {
        self.swapped.is_none()
    }
}

pub fn live(m: &Mem, d: &Decoy) -> bool {
    if m.q(d.cam + nat::CACHED_PTR) != d.native || m.q(m.q(d.native + nat::NAT_MANAGED)) != d.cam {
        return false;
    }
    let go = m.q(d.native + nat::COMP_GO);
    let tf = m.q(m.q(go + nat::GO_COMPS) + 8);
    m.q(m.q(tf + nat::TF_HIER) + nat::H_LOCAL) == d.trs
}

pub fn point(m: &Mem, d: &Decoy, from: V3, to: V3) -> bool {
    if !live(m, d) {
        return false;
    }
    let (rot, axes) = look(sub(to, from));
    let mut trs = [0u8; 0x20];
    for (i, v) in from.iter().chain([0.0f32].iter()).chain(rot.iter()).enumerate() {
        trs[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    m.write(d.trs, &trs);
    let view = view(axes, from);
    if let Some(proj) = m.rd::<[f32; 16]>(d.native + nat::CAM_PROJ) {
        m.write(d.native + nat::CAM_VIEW, &bytes(&view));
        m.write(d.native + nat::CAM_VIEW + 0x80, &bytes(&mul(&proj, &view)));
    }
    true
}

fn plan(snap: &Snap, cfg: &Cfg, t: &Ent, bone: usize, r: &Reach) -> Option<(V3, V3)> {
    if !cfg.magic() || t.seen(bone) {
        return camera_shot(snap, t, bone, r);
    }
    let p = aim::spot(t, bone)?;
    let p: V3 = std::array::from_fn(|k| p[k] + t.vel[k] * r.delay);
    let d = sub(p, snap.cam);
    let dist = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let dir = norm(d);
    let at = |k: f32| -> V3 { std::array::from_fn(|i| snap.cam[i] + dir[i] * k) };

    if r.host {
        return Some((at(dist - MAGIC_BACK), p));
    }

    let k = reach(snap, r.limit).min(dist - MAGIC_BACK);
    let from = at(k);
    if k <= 0.0 || r.bvh?.blocked(from, p) {
        return None;
    }
    Some((from, aim::lead_from(snap, t, p, from)))
}

fn camera_shot(snap: &Snap, t: &Ent, bone: usize, r: &Reach) -> Option<(V3, V3)> {
    let p = aim::spot(t, bone)?;
    let p: V3 = std::array::from_fn(|k| p[k] + t.vel[k] * r.delay);
    Some((snap.cam, aim::lead(snap, t, p)))
}

pub fn roll(cfg: &Cfg, e: &Ent, r: u64) -> usize {
    let parts: Vec<(usize, u32)> = PARTS.iter().zip(cfg.hit).filter(|((b, _), w)| *w > 0 && e.bones[*b].is_some() && (!cfg.vis_only || e.seen(*b))).map(|((b, _), w)| (*b, w as u32)).collect();
    let sum: u32 = parts.iter().map(|p| p.1).sum();
    if sum == 0 {
        return cfg.bone;
    }
    let mut r = (r % sum as u64) as u32;
    for (b, w) in parts {
        if r < w {
            return b;
        }
        r -= w;
    }
    cfg.bone
}

pub fn next(rng: &mut u64) -> u64 {
    if *rng == 0 {
        *rng = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64) | 1;
    }
    *rng ^= *rng << 13;
    *rng ^= *rng >> 7;
    *rng ^= *rng << 17;
    *rng
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn norm(v: V3) -> V3 {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
    [v[0] / l, v[1] / l, v[2] / l]
}

fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn look(f: V3) -> ([f32; 4], [V3; 3]) {
    let f = norm(f);
    let up = if f[1].abs() > 0.999 { [0.0, 0.0, 1.0] } else { [0.0, 1.0, 0.0] };
    let r = norm(cross(up, f));
    let u = cross(f, r);
    let (m00, m11, m22) = (r[0], u[1], f[2]);
    let tr = m00 + m11 + m22;

    let q = if tr > 0.0 {
        let s = (tr + 1.0).sqrt() * 2.0;
        [(u[2] - f[1]) / s, (f[0] - r[2]) / s, (r[1] - u[0]) / s, 0.25 * s]
    } else if m00 > m11 && m00 > m22 {
        let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
        [0.25 * s, (u[0] + r[1]) / s, (f[0] + r[2]) / s, (u[2] - f[1]) / s]
    } else if m11 > m22 {
        let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
        [(u[0] + r[1]) / s, 0.25 * s, (f[1] + u[2]) / s, (f[0] - r[2]) / s]
    } else {
        let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        [(f[0] + r[2]) / s, (f[1] + u[2]) / s, 0.25 * s, (r[1] - u[0]) / s]
    };
    (q, [r, u, f])
}

fn view([r, u, f]: [V3; 3], p: V3) -> [f32; 16] {
    [r[0], u[0], -f[0], 0.0, r[1], u[1], -f[1], 0.0, r[2], u[2], -f[2], 0.0, -dot(r, p), -dot(u, p), dot(f, p), 1.0]
}

fn bytes(v: &[f32; 16]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roll_follows_weights() {
        let mut si = Silent::default();
        let cfg = Cfg { hit: [60, 0, 30, 10, 0], vis_only: false, ..Cfg::default() };
        let mut e = Ent::default();
        for (b, _) in PARTS {
            e.bones[b] = Some([0.0; 3]);
        }
        let mut n = std::collections::HashMap::new();
        for _ in 0..20000 {
            *n.entry(si.roll(&cfg, &e)).or_insert(0u32) += 1;
        }
        let pct = |b| n.get(&b).copied().unwrap_or(0) as f32 / 200.0;
        assert!((pct(HEAD) - 60.0).abs() < 2.0 && (pct(CHEST) - 30.0).abs() < 2.0 && (pct(STOMACH) - 10.0).abs() < 2.0, "{n:?}");
        assert_eq!(pct(NECK) + pct(PELVIS), 0.0);
    }

    #[test]
    fn decoy_faces_target() {
        for f in [[1.0, 0.2, 0.3], [-0.5, -0.8, 0.1], [0.0, 0.0, -1.0], [0.3, 0.99, 0.0], [-2.0, 0.1, -5.0]] {
            let want = norm(f);
            let (q, axes) = look(f);
            let got = crate::game::px::rot(q, [0.0, 0.0, 1.0]);
            let v = view(axes, [3.0, 1.0, -2.0]);
            let fv = [-v[2], -v[6], -v[10]];
            for k in 0..3 {
                assert!((got[k] - want[k]).abs() < 1e-4, "quat {f:?} {got:?}");
                assert!((fv[k] - want[k]).abs() < 1e-4, "view {f:?} {fv:?}");
            }

            let t = [v[12], v[13], v[14]];
            let pos: Vec<f32> = (0..3).map(|j| -(0..3).map(|i| v[j * 4 + i] * t[i]).sum::<f32>()).collect();
            assert!((pos[0] - 3.0).abs() < 1e-4 && (pos[1] - 1.0).abs() < 1e-4 && (pos[2] + 2.0).abs() < 1e-4);
        }
    }

    #[test]
    fn magic_reach_past_close_walls_only() {
        let wall = |z: f32| Bvh::build(vec![[[-50.0, -50.0, z], [50.0, -50.0, z], [-50.0, 50.0, z]], [[50.0, -50.0, z], [50.0, 50.0, z], [-50.0, 50.0, z]]]);
        let snap = Snap { ok: true, cam: [0.0; 3], ..Snap::default() };
        let mut t = Ent::default();
        t.bones[CHEST] = Some([0.0, 0.0, 10.0]);
        let cfg = Cfg { silent: true, method: crate::features::aim::Method::Magic, ..Cfg::default() };
        let near = wall(2.0);
        let far = wall(6.0);
        let r = |host, bvh| Reach { limit: 5.0, host, bvh, delay: 0.0 };

        let (from, _) = plan(&snap, &cfg, &t, CHEST, &r(false, Some(&near))).expect("past a close wall");
        assert!((from[2] - 3.4).abs() < 1e-3 && from[0].abs() < 1e-3);

        assert!(plan(&snap, &cfg, &t, CHEST, &r(false, Some(&far))).is_none());

        assert!(plan(&snap, &cfg, &t, CHEST, &r(false, None)).is_none());

        let (from, p) = plan(&snap, &cfg, &t, CHEST, &r(true, Some(&far))).unwrap();
        assert!((from[2] - (10.0 - MAGIC_BACK)).abs() < 1e-3 && p == [0.0, 0.0, 10.0]);

        t.vis = 1 << CHEST;
        assert_eq!(plan(&snap, &cfg, &t, CHEST, &r(false, Some(&far))).unwrap().0, [0.0; 3]);
    }
}
