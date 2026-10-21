use crate::game::sdk::nat;
use crate::game::vis::Tri;
use crate::os::mem::{self, Mem};
use std::collections::HashMap;

pub type V3 = [f32; 3];

const T_DYNAMIC: u16 = 5;
const T_STATIC: u16 = 6;
const T_SHAPE: u16 = 7;
const MAX_TRIS: usize = 4 << 20;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct Scene {
    pub np: u64,
    pub actors: u64,
}

impl Scene {
    pub fn find(m: &Mem, rb: u64) -> Option<Scene> {
        let actor = m.q(rb + nat::RB_ACTOR);
        let np = m.q(rb + nat::RB_SCENE);
        if !mem::ok(actor) || !mem::ok(np) || ty(m, actor) != T_DYNAMIC || m.q(actor + 0x10) != rb {
            return None;
        }
        let blob = m.bytes(np, 0x20000)?;

        let mut best: Option<(usize, u64)> = None;
        for o in (0..blob.len() - 16).step_by(8) {
            let p = u64::from_le_bytes(blob[o..o + 8].try_into().unwrap());
            let n = u32::from_le_bytes(blob[o + 8..o + 12].try_into().unwrap()) as usize;
            let cap = (u32::from_le_bytes(blob[o + 12..o + 16].try_into().unwrap()) & 0x7fff_ffff) as usize;
            if !mem::ok(p) || !(1..=1 << 18).contains(&n) || cap < n {
                continue;
            }
            let Some(a) = m.bytes(p, n * 8) else { continue };
            let ptrs: Vec<u64> = a.chunks_exact(8).map(|c| u64::from_le_bytes(c.try_into().unwrap())).collect();
            if !ptrs.contains(&actor) {
                continue;
            }
            let statics = ptrs.iter().filter(|&&p| ty(m, p) == T_STATIC && pose(m, p + 0x90).is_some()).count();
            if statics > 0 && best.is_none_or(|(bs, _)| statics > bs) {
                best = Some((statics, o as u64));
            }
        }
        best.map(|(_, o)| Scene { np, actors: o })
    }

    pub fn actors(&self, m: &Mem) -> Vec<u64> {
        let p = m.q(self.np + self.actors);
        let n = m.u32(self.np + self.actors + 8) as usize;
        m.bytes(p, n.min(1 << 20) * 8).map(|b| b.chunks_exact(8).map(|c| u64::from_le_bytes(c.try_into().unwrap())).collect()).unwrap_or_default()
    }

    pub fn statics(&self, m: &Mem) -> usize {
        self.actors(m).into_iter().filter(|&a| ty(m, a) == T_STATIC).count()
    }

    pub fn triangles(&self, m: &Mem) -> Vec<Tri> {
        let mut out = vec![];
        let mut meshes: HashMap<u64, Option<(Vec<V3>, Vec<[u32; 3]>)>> = HashMap::new();
        for a in self.actors(m) {
            if ty(m, a) != T_STATIC {
                continue;
            }
            let Some(ap) = pose(m, a + 0x90) else { continue };
            for s in shapes(m, a) {
                if ty(m, s) != T_SHAPE {
                    continue;
                }
                let flags = m.u8(s + 0x90);
                if flags & 4 != 0 || flags & 2 == 0 {
                    continue;
                }
                let Some(local) = pose(m, s + 0x70) else { continue };
                let g = s + 0x98;
                let world = |v: V3| ap.apply(local.apply(v));
                match m.u32(g) {
                    0 => {
                        let r = f(m, g + 4) * 0.7;
                        boxed(&mut out, [0.0; 3], [r; 3], world);
                    }
                    2 => {
                        let (r, hh) = (f(m, g + 4), f(m, g + 8));
                        boxed(&mut out, [0.0; 3], [hh + r * 0.7, r * 0.7, r * 0.7], world);
                    }
                    3 => {
                        let Some(h) = m.rd::<[f32; 3]>(g + 4) else { continue };
                        boxed(&mut out, [0.0; 3], h, world);
                    }
                    4 => {
                        let Some(sc) = scale(m, g) else { continue };
                        let cm = m.q(g + 0x20);
                        let (Some(c), Some(e)) = (m.rd::<[f32; 3]>(cm + 0x20), m.rd::<[f32; 3]>(cm + 0x2c)) else { continue };
                        boxed(&mut out, c, e, |v| world(sc.apply(v)));
                    }
                    5 => {
                        let Some(sc) = scale(m, g) else { continue };
                        let tm = m.q(g + 0x28);
                        let Some((verts, tris)) = meshes.entry(tm).or_insert_with(|| mesh(m, tm)).as_ref() else { continue };
                        let wv: Vec<V3> = verts.iter().map(|&v| world(sc.apply(v))).collect();
                        out.extend(tris.iter().map(|t| [wv[t[0] as usize], wv[t[1] as usize], wv[t[2] as usize]]));
                    }
                    _ => {}
                }
                if out.len() > MAX_TRIS {
                    return out;
                }
            }
        }
        out
    }
}

impl Scene {
    pub fn report(&self, m: &Mem) -> String {
        let mut c: HashMap<&str, usize> = HashMap::new();
        for a in self.actors(m).into_iter().filter(|&a| ty(m, a) == T_STATIC) {
            if pose(m, a + 0x90).is_none() {
                *c.entry("actor pose rejected").or_default() += 1;
                continue;
            }
            for s in shapes(m, a) {
                let flags = m.u8(s + 0x90);
                let k = if ty(m, s) != T_SHAPE {
                    "not a shape"
                } else if flags & 4 != 0 || flags & 2 == 0 {
                    "trigger / no query"
                } else if pose(m, s + 0x70).is_none() {
                    "shape pose rejected"
                } else {
                    match m.u32(s + 0x98) {
                        0 => "sphere",
                        2 => "capsule",
                        3 => "box",
                        4 => "convex",
                        5 if mesh(m, m.q(s + 0x98 + 0x28)).is_none() => "mesh unreadable",
                        5 => "mesh",
                        _ => "other",
                    }
                };
                *c.entry(k).or_default() += 1;
            }
        }
        let mut v: Vec<_> = c.into_iter().collect();
        v.sort();
        v.iter().map(|(k, n)| format!("  {k}: {n}")).collect::<Vec<_>>().join("\n")
    }
}

fn ty(m: &Mem, o: u64) -> u16 {
    m.rd::<u16>(o + 8).unwrap_or(0)
}

fn f(m: &Mem, a: u64) -> f32 {
    m.rd::<f32>(a).unwrap_or(0.0)
}

fn shapes(m: &Mem, a: u64) -> Vec<u64> {
    let p = m.q(a + 0x28);
    let n = m.rd::<u16>(a + 0x30).unwrap_or(0) as usize;
    match n {
        0 => vec![],
        1 => vec![p],
        _ => m.bytes(p, n * 8).map(|b| b.chunks_exact(8).map(|c| u64::from_le_bytes(c.try_into().unwrap())).collect()).unwrap_or_default(),
    }
}

fn mesh(m: &Mem, tm: u64) -> Option<(Vec<V3>, Vec<[u32; 3]>)> {
    let (nv, nt) = (m.u32(tm + 0x20) as usize, m.u32(tm + 0x24) as usize);
    if nv == 0 || nt == 0 || nv > 1 << 22 || nt > 1 << 22 {
        return None;
    }
    let vb = m.bytes(m.q(tm + 0x28), nv * 12)?;
    let verts: Vec<V3> = vb.chunks_exact(12).map(|c| std::array::from_fn(|i| f32::from_le_bytes(c[i * 4..i * 4 + 4].try_into().unwrap()))).collect();
    let wide = m.u8(tm + 0x5c) & 2 == 0;
    let ib = m.bytes(m.q(tm + 0x30), nt * 3 * if wide { 4 } else { 2 })?;
    let idx: Vec<u32> =
        if wide { ib.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect() } else { ib.chunks_exact(2).map(|c| u16::from_le_bytes(c.try_into().unwrap()) as u32).collect() };
    if idx.iter().any(|&i| i as usize >= nv) {
        return None;
    }
    Some((verts, idx.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect()))
}

#[derive(Clone, Copy)]
struct Pose {
    q: [f32; 4],
    p: V3,
}

impl Pose {
    fn apply(&self, v: V3) -> V3 {
        let r = rot(self.q, v);
        [r[0] + self.p[0], r[1] + self.p[1], r[2] + self.p[2]]
    }
}

fn pose(m: &Mem, a: u64) -> Option<Pose> {
    let t: [f32; 7] = m.rd(a)?;
    let q = [t[0], t[1], t[2], t[3]];
    let n = q.iter().map(|x| x * x).sum::<f32>();
    ((0.98..1.02).contains(&n) && t.iter().all(|v| v.is_finite())).then_some(Pose { q, p: [t[4], t[5], t[6]] })
}

#[derive(Clone, Copy)]
struct Scale {
    s: V3,
    q: [f32; 4],
}

impl Scale {
    fn apply(&self, v: V3) -> V3 {
        let r = rot(self.q, v);
        let r = [r[0] * self.s[0], r[1] * self.s[1], r[2] * self.s[2]];
        rot([-self.q[0], -self.q[1], -self.q[2], self.q[3]], r)
    }
}

fn scale(m: &Mem, g: u64) -> Option<Scale> {
    let t: [f32; 7] = m.rd(g + 4)?;
    t.iter().all(|v| v.is_finite()).then_some(Scale { s: [t[0], t[1], t[2]], q: [t[3], t[4], t[5], t[6]] })
}

fn boxed(out: &mut Vec<Tri>, c: V3, h: V3, to: impl Fn(V3) -> V3) {
    let v: Vec<V3> = (0..8)
        .map(|i| {
            let s = |b: usize, k: usize| if i >> b & 1 == 1 { c[k] + h[k] } else { c[k] - h[k] };
            to([s(0, 0), s(1, 1), s(2, 2)])
        })
        .collect();
    const F: [[usize; 4]; 6] = [[0, 1, 3, 2], [4, 5, 7, 6], [0, 1, 5, 4], [2, 3, 7, 6], [0, 2, 6, 4], [1, 3, 7, 5]];
    for q in F {
        out.push([v[q[0]], v[q[1]], v[q[2]]]);
        out.push([v[q[0]], v[q[2]], v[q[3]]]);
    }
}

pub fn rot(q: [f32; 4], v: V3) -> V3 {
    let [x, y, z, w] = q;
    let t = [2.0 * (y * v[2] - z * v[1]), 2.0 * (z * v[0] - x * v[2]), 2.0 * (x * v[1] - y * v[0])];
    [v[0] + w * t[0] + (y * t[2] - z * t[1]), v[1] + w * t[1] + (z * t[0] - x * t[2]), v[2] + w * t[2] + (x * t[1] - y * t[0])]
}
