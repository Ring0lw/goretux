use crate::game::V3;
use crate::game::il2::{self, Il2};
use crate::os::mem::{self, Mem};
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub const WANT: &[&str] = &["GoreBox.Weapons.Projectile", "GoreBox.Weapons.Projectile[]"];

pub const OWN: f32 = 2.5;
pub const FADE: Duration = Duration::from_millis(1500);
const MAX_TRAILS: usize = 160;
const MAX_POINTS: usize = 96;

#[derive(Clone, Copy)]
pub struct Pool {
    arr: u64,
    pub count: u64,
    len: usize,
    stride: usize,

    pub pos: usize,
    pub prev: usize,
    pub vel: usize,
    pub speed: usize,
    pub age: usize,
    pub auth: usize,
    pub anchor: usize,
    pub alive: usize,
    pub serial: usize,
}

impl Pool {
    pub fn find(m: &Mem) -> Option<Pool> {
        let il = Il2::scan(m, WANT);
        let (kp, ka) = (il.class(WANT[0])?, il.class(WANT[1])?);
        let fs = il2::fields(m, kp);
        let f = |n: &str| fs.iter().find(|f| !f.stat && f.name == n).map(|f| f.off as usize - 0x10);
        let stride = m.u32(kp + il2::K_INSTANCE_SIZE) as usize - 0x10;
        let arr = mem::objects(m, ka).into_iter().filter(|&a| m.q(a + 8) == 0).max_by_key(|&a| m.i32(a + 0x18))?;
        let len = m.i32(arr + 0x18) as usize;

        let slot = mem::find(m, arr, 8).into_iter().find(|&s| (0..=len as i32).contains(&m.i32(s + 8)))?;
        Some(Pool {
            arr,
            count: slot + 8,
            len,
            stride,
            pos: f("pos")?,
            prev: f("prevPos")?,
            vel: f("vel")?,
            speed: f("muzzleSpeed")?,
            age: f("age")?,
            auth: f("authoritative")?,
            anchor: f("trailAnchor")?,
            alive: f("alive")?,
            serial: f("serial")?,
        })
    }

    pub fn valid(&self, m: &Mem) -> bool {
        m.i32(self.arr + 0x18) as usize == self.len
    }

    pub fn live(&self, m: &Mem) -> Vec<(u64, Vec<u8>)> {
        let n = m.i32(self.count).clamp(0, self.len as i32) as usize;
        let data = m.bytes(self.arr + 0x20, n * self.stride).unwrap_or_default();
        data.chunks_exact(self.stride).enumerate().map(|(i, e)| (self.arr + 0x20 + (i * self.stride) as u64, e.to_vec())).collect()
    }
}

#[derive(Clone)]
pub struct Trail {
    pub pts: Vec<V3>,
    pub own: bool,
    pub end: Option<Instant>,
}

#[derive(Default)]
pub struct Tracker {
    trails: HashMap<u32, Trail>,
}

impl Tracker {
    pub fn tick(&mut self, m: &Mem, p: &Pool, cam: V3) -> Vec<Trail> {
        let n = m.i32(p.count).clamp(0, p.len as i32) as usize;
        let data = m.bytes(p.arr + 0x20, n * p.stride).unwrap_or_default();
        let f3 = |e: &[u8], o: usize| -> V3 { std::array::from_fn(|k| f32::from_le_bytes(e[o + k * 4..o + k * 4 + 4].try_into().unwrap())) };
        let mut live = Vec::with_capacity(n);
        for e in data.chunks_exact(p.stride) {
            if e[p.alive] == 0 {
                continue;
            }
            let serial = u32::from_le_bytes(e[p.serial..p.serial + 4].try_into().unwrap());
            let (pos, anchor) = (f3(e, p.pos), f3(e, p.anchor));
            if !pos.iter().chain(anchor.iter()).all(|v| v.is_finite()) {
                continue;
            }
            live.push(serial);
            let t = self.trails.entry(serial).or_insert_with(|| Trail { pts: vec![anchor], own: dist(anchor, cam) < OWN, end: None });
            if t.pts.last().is_none_or(|&l| dist(l, pos) > 0.25) && t.pts.len() < MAX_POINTS {
                t.pts.push(pos);
            }
        }
        let now = Instant::now();
        self.trails.retain(|s, t| {
            if t.end.is_none() && !live.contains(s) {
                t.end = Some(now);
            }
            t.end.is_none_or(|e| e.elapsed() < FADE)
        });
        if self.trails.len() > MAX_TRAILS {
            let mut old: Vec<(u32, Instant)> = self.trails.iter().filter_map(|(s, t)| Some((*s, t.end?))).collect();
            old.sort_by_key(|&(_, e)| e);
            for (s, _) in old.into_iter().take(self.trails.len() - MAX_TRAILS) {
                self.trails.remove(&s);
            }
        }
        self.trails.values().cloned().collect()
    }
}

fn dist(a: V3, b: V3) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}
