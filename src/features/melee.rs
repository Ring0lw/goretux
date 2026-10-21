use crate::features::aim::{self, Cfg};
use crate::features::silent::{self, Decoy};
use crate::game::undo;
use crate::game::vis::Bvh;
use crate::game::{CHEST, Ent, HEAD, NECK, PELVIS, Snap, V3};
use crate::os::mem::{self, Mem};
use crate::os::uinput::Mouse;
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
pub struct Fists {
    pub obj: u64,
    pub class: u64,
    pub cam: u64,

    pub attacks: [(u64, u64); 4],

    pub line: Option<(u64, u64)>,
}

const OUT: f32 = 0.4;

const SKULL: f32 = 0.09;

#[rustfmt::skip]
const BODY: [(usize, usize, f32); 17] = [
    (PELVIS, 1, 0.16), (1, 2, 0.16), (2, CHEST, 0.17), (CHEST, NECK, 0.12), (NECK, HEAD, 0.06),
    (CHEST, 7, 0.09), (7, 8, 0.07), (8, 9, 0.06), (9, 9, 0.07),
    (CHEST, 10, 0.09), (10, 11, 0.07), (11, 12, 0.06), (12, 12, 0.07),
    (PELVIS, 13, 0.1), (13, 14, 0.08), (PELVIS, 16, 0.1), (16, 17, 0.08),
];

const PICK: Duration = Duration::from_millis(5);
const KEEP: f32 = 0.03;

const THIN: f32 = 0.04;

const PRESS: Duration = Duration::from_millis(40);
const EVERY: Duration = Duration::from_millis(300);

#[derive(Clone)]
pub struct Setup {
    pub name: String,
    head: V3,
    out: V3,
    bones: [Option<V3>; 19],
    at: Instant,
}

const BODY_NAMES: [&str; 17] = [
    "pelvis-spine",
    "spine",
    "spine-chest",
    "chest-neck",
    "neck-head",
    "L shoulder",
    "L upper arm",
    "L forearm",
    "L hand",
    "R shoulder",
    "R upper arm",
    "R forearm",
    "R hand",
    "L hip",
    "L thigh",
    "R hip",
    "R thigh",
];

#[derive(Default)]
pub struct Aura {
    pub setup: Option<Setup>,

    side: Option<(u64, V3, Instant)>,
    swapped: Option<(u64, u64, u64, u64)>,
    thinned: Vec<(u64, u64, u64, [u8; 4])>,
    mouse: Option<Mouse>,
    pressed: Option<Instant>,
    last: Option<Instant>,
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn len(v: V3) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn at(p: V3, d: V3, k: f32) -> V3 {
    [p[0] + d[0] * k, p[1] + d[1] * k, p[2] + d[2] * k]
}

fn unit(v: V3) -> Option<V3> {
    let l = len(v);
    (l > 1e-4).then(|| [v[0] / l, v[1] / l, v[2] / l])
}

fn seg_dist(p1: V3, q1: V3, p2: V3, q2: V3) -> f32 {
    let (d1, d2, r) = (sub(q1, p1), sub(q2, p2), sub(p1, p2));
    let (a, e, f) = (dot(d1, d1), dot(d2, d2), dot(d2, r));
    let (s, t) = if a <= 1e-8 && e <= 1e-8 {
        (0.0, 0.0)
    } else if a <= 1e-8 {
        (0.0, (f / e).clamp(0.0, 1.0))
    } else {
        let c = dot(d1, r);
        if e <= 1e-8 {
            ((-c / a).clamp(0.0, 1.0), 0.0)
        } else {
            let b = dot(d1, d2);
            let den = a * e - b * b;
            let s = if den > 1e-8 { ((b * f - c * e) / den).clamp(0.0, 1.0) } else { 0.0 };
            let t = (b * s + f) / e;
            if t < 0.0 {
                ((-c / a).clamp(0.0, 1.0), 0.0)
            } else if t > 1.0 {
                (((b - c) / a).clamp(0.0, 1.0), 1.0)
            } else {
                (s, t)
            }
        }
    };
    len(sub(at(p1, d1, s), at(p2, d2, t)))
}

fn room(e: &Ent, head: V3, out: V3) -> f32 {
    let (from, to) = (at(head, out, OUT), at(head, out, SKULL));
    BODY.iter().filter_map(|&(a, b, r)| Some(seg_dist(from, to, e.bones[a]?, e.bones[b]?) - r)).fold(f32::INFINITY, f32::min)
}

fn sides(e: &Ent, head: V3, cam: V3) -> Vec<V3> {
    let mut v = vec![aim::head_up(e), [0.0, 1.0, 0.0]];
    v.extend(unit(sub(cam, head)));
    for x in -1..=1 {
        for y in -1..=1 {
            for z in -1..=1 {
                v.extend(unit([x as f32, y as f32, z as f32]));
            }
        }
    }
    v
}

fn ok(e: &Ent, cfg: &Cfg) -> bool {
    !e.dead && !(cfg.team_check && e.team) && if e.npc { cfg.npcs } else { cfg.players }
}

impl Aura {
    #[allow(clippy::too_many_arguments)]
    pub fn tick(&mut self, m: &Mem, s: &Snap, cfg: &Cfg, fists: Option<Fists>, out: bool, decoy: Option<Decoy>, ingame: bool, bvh: Option<&Bvh>) -> u64 {
        let (Some(f), Some(d), true) = (fists, decoy, cfg.aura && out && ingame && s.ok) else {
            self.release(m);
            return 0;
        };
        if m.q(f.obj) != f.class {
            self.release(m);
            return 0;
        }
        let near = s
            .ents
            .iter()
            .filter(|e| ok(e, cfg))
            .filter_map(|e| {
                let head = aim::spot(e, HEAD)?;
                let dist = len(sub(head, s.cam));
                (dist <= cfg.aura_range).then_some((dist, e, head))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        let Some((_, t, head)) = near else {
            self.release(m);
            return 0;
        };
        let out = self.side(t, head, s.cam, bvh);
        let from = at(head, out, OUT);
        self.setup = Some(Setup { name: t.name.clone(), head, out, bones: t.bones, at: Instant::now() });
        if !silent::point(m, &d, from, head) {
            self.release(m);
            return 0;
        }

        if let Some((flag, org)) = f.line {
            let dir = sub(head, from);
            let l = len(dir).max(1e-4);
            let line: Vec<u8> = from.iter().chain(dir.map(|c| c / l).iter()).flat_map(|v| v.to_le_bytes()).collect();
            m.write(org, &line);
            if m.u8(flag) != 0 {
                m.write(flag, &[0]);
            }
        }

        if self.swapped.is_some_and(|(o, ..)| o != f.obj) {
            self.release(m);
        }
        let cur = m.q(f.cam);
        if self.swapped.is_none() {
            let orig = if cur == d.cam { s.aim_cam } else { cur };
            if !mem::ok(orig) {
                return 0;
            }
            self.swapped = Some((f.obj, m.q(f.obj), f.cam, orig));
            undo::keep(m.fd(), f.cam, f.obj, m.q(f.obj), &orig.to_le_bytes());
        }
        if cur != d.cam {
            m.write(f.cam, &d.cam.to_le_bytes());
        }
        for &(a, r) in f.attacks.iter().filter(|x| x.1 != 0) {
            if !self.thinned.iter().any(|t| t.2 == r) {
                let Some(old) = m.rd::<f32>(r) else { continue };
                undo::keep(m.fd(), r, a, m.q(a), &old.to_le_bytes());
                self.thinned.push((a, m.q(a), r, old.to_le_bytes()));
            }
            if m.rd::<f32>(r) != Some(THIN) {
                m.write(r, &THIN.to_le_bytes());
            }
        }

        if cfg.auto_punch {
            self.punch();
        } else {
            self.lift();
        }
        t.id
    }

    pub fn report(&self, m: &Mem, f: Option<Fists>) -> String {
        let Some(l) = &self.setup else { return "no setup".into() };
        let e = Ent { id: 0, team: false, name: String::new(), npc: false, bones: l.bones, root: [0.0; 3], dist: 0.0, vis: 0, dead: false, vel: [0.0; 3] };
        let (from, to) = (at(l.head, l.out, OUT), at(l.head, l.out, SKULL));
        let mut gaps: Vec<(f32, &str)> = BODY.iter().zip(BODY_NAMES).filter_map(|(&(a, b, r), n)| Some((seg_dist(from, to, l.bones[a]?, l.bones[b]?) - r, n))).collect();
        gaps.sort_by(|a, b| a.0.total_cmp(&b.0));
        let near: Vec<String> = gaps.iter().take(4).map(|(g, n)| format!("{n} {g:+.2}")).collect();
        let up = aim::head_up(&e);
        let held = f.and_then(|f| f.line.map(|(flag, org)| (m.u8(flag), m.rd::<[f32; 6]>(org)))).map_or("no line".into(), |(flag, v)| match v {
            Some(v) => {
                let (o, d) = ([v[0], v[1], v[2]], [v[3], v[4], v[5]]);
                format!("held flag {flag} origin off {:.2}m dir dot {:.3}", len(sub(o, from)), dot(d, unit(sub(l.head, from)).unwrap_or([0.0; 3])))
            }
            None => "unreadable".into(),
        });
        format!(
            "{:.0}ms ago, from {:.0}deg off the neck axis ({:+.2} {:+.2} {:+.2}), closest: {} | {held}",
            l.at.elapsed().as_secs_f32() * 1000.0,
            dot(up, l.out).clamp(-1.0, 1.0).acos().to_degrees(),
            l.out[0],
            l.out[1],
            l.out[2],
            near.join(", ")
        )
    }

    fn side(&mut self, t: &Ent, head: V3, cam: V3, bvh: Option<&Bvh>) -> V3 {
        let kept = self.side.filter(|(id, ..)| *id == t.id);
        if let Some((_, o, when)) = kept
            && when.elapsed() < PICK
        {
            return o;
        }
        let clear = |o: V3| bvh.is_none_or(|b| !b.blocked(at(head, o, OUT), head));
        let best = sides(t, head, cam).into_iter().filter(|&o| clear(o)).map(|o| (room(t, head, o), o)).max_by(|a, b| a.0.total_cmp(&b.0));
        let o = match (kept, best) {
            (Some((_, o, _)), Some((r, _))) if clear(o) && room(t, head, o) >= r - KEEP => o,
            (_, Some((_, o))) => o,
            _ => aim::head_up(t),
        };
        self.side = Some((t.id, o, Instant::now()));
        o
    }

    fn punch(&mut self) {
        if self.mouse.is_none() {
            self.mouse = Mouse::new().ok();
        }
        let Some(mouse) = self.mouse.as_mut() else { return };
        if let Some(at) = self.pressed {
            if at.elapsed() >= PRESS && mouse.left(false).is_ok() {
                self.pressed = None;
            }
            return;
        }
        if self.last.is_none_or(|l| l.elapsed() >= EVERY) && mouse.left(true).is_ok() {
            let now = Instant::now();
            (self.pressed, self.last) = (Some(now), Some(now));
        }
    }

    fn lift(&mut self) {
        if self.pressed.take().is_some()
            && let Some(mouse) = self.mouse.as_mut()
        {
            let _ = mouse.left(false);
        }
    }

    pub fn release(&mut self, m: &Mem) {
        self.lift();
        self.side = None;
        if let Some((obj, k, at, orig)) = self.swapped.take() {
            if m.q(obj) == k {
                m.write(at, &orig.to_le_bytes());
            }
            undo::forget(at);
        }
        for (a, k, r, old) in self.thinned.drain(..) {
            if m.q(a) == k {
                m.write(r, &old);
            }
            undo::forget(r);
        }
    }

    pub fn idle(&self) -> bool {
        self.swapped.is_none() && self.pressed.is_none() && self.thinned.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_distance() {
        assert!((seg_dist([0.0, 0.0, -1.0], [0.0, 0.0, 1.0], [-1.0, 1.0, 0.0], [1.0, 1.0, 0.0]) - 1.0).abs() < 1e-5);

        assert!((seg_dist([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.5, 0.0], [1.0, 0.5, 0.0]) - 0.5).abs() < 1e-5);

        assert!((seg_dist([2.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]) - 1.0).abs() < 1e-5);
    }

    fn body(hands_up: bool) -> Ent {
        let mut e = Ent { id: 1, team: false, name: String::new(), npc: false, bones: [None; 19], root: [0.0; 3], dist: 2.0, vis: 0, dead: false, vel: [0.0; 3] };
        let b = &mut e.bones;
        b[PELVIS] = Some([0.0, 1.0, 0.0]);
        b[1] = Some([0.0, 1.15, 0.0]);
        b[2] = Some([0.0, 1.3, 0.0]);
        b[CHEST] = Some([0.0, 1.42, 0.0]);
        b[NECK] = Some([0.0, 1.55, 0.0]);
        b[HEAD] = Some([0.0, 1.65, 0.0]);
        b[7] = Some([-0.18, 1.45, 0.0]);
        b[10] = Some([0.18, 1.45, 0.0]);
        if hands_up {
            b[8] = Some([-0.15, 1.75, 0.0]);
            b[9] = Some([-0.05, 1.95, 0.0]);
            b[11] = Some([0.15, 1.75, 0.0]);
            b[12] = Some([0.05, 1.95, 0.0]);
        } else {
            b[8] = Some([-0.22, 1.2, 0.0]);
            b[9] = Some([-0.22, 1.0, 0.0]);
            b[11] = Some([0.22, 1.2, 0.0]);
            b[12] = Some([0.22, 1.0, 0.0]);
        }
        e
    }

    #[test]
    fn from_above_when_standing_from_the_side_with_hands_up() {
        let mut a = Aura::default();
        let e = body(false);
        let head = aim::spot(&e, HEAD).unwrap();
        let o = a.side(&e, head, [0.0, 1.6, -3.0], None);
        assert!(room(&e, head, o) > 0.1 && o[1] > 0.5, "standing: from above, got {o:?}");
        let e = body(true);
        let mut a = Aura::default();
        let o = a.side(&e, head, [0.0, 1.6, -3.0], None);
        assert!(o[1] < 0.8 && room(&e, head, o) > 0.0, "hands over the head: not from above, got {o:?}");

        assert!(room(&e, head, [0.0, 1.0, 0.0]) < 0.0);
    }
}
