use crate::features::aim::{self, Cfg};
use crate::features::silent;
use crate::features::tracers::{self, Pool};
use crate::game::{Snap, V3};
use crate::os::mem::Mem;
use std::collections::HashMap;
use std::time::{Duration, Instant};

const BACK: f32 = 0.5;

const FRESH: f32 = 0.25;

const RETRY: Duration = Duration::from_millis(40);

const MIN_SPEED: f32 = 120.0;

const NEAR: f32 = 3.0;

#[derive(Default)]
pub struct Magic {
    seen: HashMap<u32, Option<(Instant, V3, V3)>>,
    rng: u64,
    pub moved: u64,
}

fn f3(e: &[u8], o: usize) -> V3 {
    std::array::from_fn(|k| f32::from_le_bytes(e[o + k * 4..o + k * 4 + 4].try_into().unwrap()))
}

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn len(v: V3) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

impl Magic {
    pub fn tick(&mut self, m: &Mem, p: &Pool, s: &Snap, cfg: &Cfg) -> u64 {
        let live = p.live(m);
        let alive: Vec<u32> = live.iter().map(|(_, e)| u32::from_le_bytes(e[p.serial..p.serial + 4].try_into().unwrap())).collect();
        self.seen.retain(|serial, _| alive.contains(serial));
        if !s.ok {
            return 0;
        }

        let mc = Cfg { fov: cfg.magic_fov, vis_only: false, ..*cfg };
        let parts: Vec<usize> = silent::PARTS.iter().zip(cfg.hit).filter(|(_, w)| *w > 0).map(|((b, _), _)| *b).collect();
        let parts = if parts.is_empty() { vec![cfg.bone] } else { parts };
        let mut hit = 0;
        for ((at, e), serial) in live.iter().zip(alive) {
            if e[p.alive] == 0 {
                continue;
            }
            let pos = f3(e, p.pos);
            let redo = match self.seen.get(&serial) {
                None => None,

                Some(Some((when, start, dir))) if when.elapsed() < RETRY && len(sub(pos, s.cam)) + 1.0 < len(sub(*start, s.cam)) => Some((*start, *dir)),
                Some(_) => continue,
            };
            let (start, dir) = match redo {
                Some(r) => r,
                None => {
                    let age = f32::from_le_bytes(e[p.age..p.age + 4].try_into().unwrap());
                    let ours = e[p.auth] != 0 && len(sub(f3(e, p.anchor), s.cam)) < tracers::OWN && age < FRESH;
                    let mine = ours && len(sub(pos, s.cam)) < NEAR;
                    let target = if mine { aim::pick(s, &mc, 0, &parts) } else { None };
                    let Some(t) = target else {
                        self.seen.insert(serial, None);
                        continue;
                    };
                    let bone = silent::roll(&mc, t, silent::next(&mut self.rng));
                    let Some(goal) = aim::spot(t, bone) else {
                        self.seen.insert(serial, None);
                        continue;
                    };

                    let d = sub(goal, s.cam);
                    let dir = d.map(|v| v / len(d).max(1e-3));
                    hit = t.id;
                    let part = silent::PARTS.iter().find(|p| p.0 == bone).map_or("body", |p| p.1);
                    eprintln!("magic: bullet {serial} -> {} {part}, {:.0}m", t.name, t.dist);
                    (std::array::from_fn(|k| goal[k] - dir[k] * BACK), dir)
                }
            };
            let speed = len(f3(e, p.vel)).max(f32::from_le_bytes(e[p.speed..p.speed + 4].try_into().unwrap())).max(MIN_SPEED);
            let vel: V3 = dir.map(|v| v * speed);
            let put = |o: usize, v: V3| m.write(at + o as u64, &v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>());

            if m.u32(at + p.serial as u64) != serial {
                continue;
            }
            put(p.pos, start);
            put(p.prev, start);
            put(p.vel, vel);
            if redo.is_none() {
                self.moved += 1;
                self.seen.insert(serial, Some((Instant::now(), start, dir)));
            }
        }
        hit
    }
}

pub fn dump() {
    let Some(m) = crate::os::mem::find_pid().and_then(|p| Mem::open(p).ok()) else { return eprintln!("GoreBox not running") };
    let t = Instant::now();
    let Some(p) = Pool::find(&m) else { return println!("no projectile pool ({:.1}s)", t.elapsed().as_secs_f32()) };
    println!("pool found in {:.1}s", t.elapsed().as_secs_f32());
    let mut seen = std::collections::HashSet::new();
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(8) {
        for (at, e) in p.live(&m) {
            let serial = u32::from_le_bytes(e[p.serial..p.serial + 4].try_into().unwrap());
            if seen.insert(serial) {
                let age = f32::from_le_bytes(e[p.age..p.age + 4].try_into().unwrap());
                println!(
                    "{:.3}s bullet {serial} at {at:#x} alive {} auth {} age {age:.3} pos {:?} anchor {:?} vel {:?}",
                    t.elapsed().as_secs_f32(),
                    e[p.alive],
                    e[p.auth],
                    f3(&e, p.pos),
                    f3(&e, p.anchor),
                    f3(&e, p.vel)
                );
            }
        }
        std::thread::sleep(Duration::from_micros(500));
    }
}

pub fn pools() {
    use crate::game::il2::{self, Il2};
    let Some(m) = crate::os::mem::find_pid().and_then(|p| Mem::open(p).ok()) else { return eprintln!("GoreBox not running") };
    let il = Il2::scan(&m, tracers::WANT);
    let (Some(kp), Some(ka)) = (il.class(tracers::WANT[0]), il.class(tracers::WANT[1])) else { return println!("classes missing") };
    let stride = m.u32(kp + il2::K_INSTANCE_SIZE) as usize - 0x10;
    let arrs: Vec<(u64, usize)> = crate::os::mem::objects(&m, ka).into_iter().filter(|&a| m.q(a + 8) == 0).map(|a| (a, m.i32(a + 0x18) as usize)).filter(|&(_, n)| n > 0 && n < 1 << 20).collect();
    println!("{} Projectile[] arrays: {:x?}", arrs.len(), arrs);
    let snap = |a: u64, n: usize| m.bytes(a + 0x20, (n * stride).min(1 << 22)).unwrap_or_default();
    let before: Vec<Vec<u8>> = arrs.iter().map(|&(a, n)| snap(a, n)).collect();
    println!("ready");
    let t = Instant::now();
    let mut changed = std::collections::BTreeMap::new();
    while t.elapsed() < Duration::from_secs(6) {
        for (i, &(a, n)) in arrs.iter().enumerate() {
            let now = snap(a, n);
            let diff = now.chunks(stride).zip(before[i].chunks(stride)).filter(|(x, y)| x != y).count();
            if diff > 0 {
                let e = changed.entry(a).or_insert(0);
                *e = (*e).max(diff);
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    println!("changed elements per array: {changed:x?}");
}

pub fn find() {
    let Some(m) = crate::os::mem::find_pid().and_then(|p| Mem::open(p).ok()) else { return eprintln!("GoreBox not running") };
    let b = match crate::game::ballistics(&m) {
        Ok(Some(b)) => b,
        other => return println!("no ballistics: {:?}", other.err()),
    };
    println!("gun: speed {} drag {} gravity {}", b.speed, b.drag, b.grav);
    let pat: Vec<u8> = [b.drag, b.grav, b.speed].iter().flat_map(|v| v.to_le_bytes()).collect();
    println!("ready");
    std::thread::sleep(Duration::from_millis(2000));
    let t = Instant::now();
    let mut hits = vec![];
    for r in crate::os::mem::maps(m.pid) {
        if !r.r || !r.w || r.hi - r.lo > 1 << 31 {
            continue;
        }
        let mut at = r.lo;
        while at < r.hi {
            let n = ((r.hi - at) as usize).min(16 << 20);
            if let Some(buf) = m.bytes(at, n) {
                for i in (0..n.saturating_sub(12)).step_by(4) {
                    if buf[i..i + 12] == pat[..] {
                        hits.push((at + i as u64, r.path.clone()));
                    }
                }
            }
            at += n as u64;
        }
    }
    println!("{} matches in {:.1}s", hits.len(), t.elapsed().as_secs_f32());

    for (h, path) in hits.iter().take(40) {
        let s = h - 0x2c;
        let Some(e) = m.bytes(s, 0x78) else { continue };
        println!(
            "{s:#x} {path} alive {} auth {} age {:.3} serial {} pos {:?} anchor {:?} vel {:?}",
            e[0x69],
            e[0x49],
            f32::from_le_bytes(e[0x3c..0x40].try_into().unwrap()),
            u32::from_le_bytes(e[0x6c..0x70].try_into().unwrap()),
            f3(&e, 0),
            f3(&e, 0x4c),
            f3(&e, 0x18)
        );
    }
}

pub fn owner() {
    use crate::game::il2;
    let Some(m) = crate::os::mem::find_pid().and_then(|p| Mem::open(p).ok()) else { return eprintln!("GoreBox not running") };
    let Some(p) = Pool::find(&m) else { return println!("no pool") };
    let slot = p.count - 8;
    for back in (0..0x800u64).step_by(8) {
        let o = slot - back;
        let k = m.q(o);
        if crate::os::mem::ok(k) && m.q(k + il2::K_SELF) == k {
            let name = il2::class_name(&m, k).unwrap_or_default();
            let ns = m.cstr(m.q(k + il2::K_NS), 96).unwrap_or_default();
            println!("owner {o:#x} class {ns}.{name} (field at +{back:#x}), parent {}", il2::class_name(&m, m.q(k + il2::K_PARENT)).unwrap_or_default());
            for f in il2::fields(&m, k) {
                let at = if f.stat { m.q(k + il2::K_STATICS) + f.off } else { o + f.off };
                println!("  {}{:#06x} te {:#04x} {} = {:#x}", if f.stat { "static " } else { "" }, f.off, f.te, f.name, m.q(at));
            }
            return;
        }
    }
    println!("no object header found before the pool field");
}

pub fn shots(secs: u64) {
    use crate::game::il2::{self, Il2};
    let Some(m) = crate::os::mem::find_pid().and_then(|p| Mem::open(p).ok()) else { return eprintln!("GoreBox not running") };
    let Some(p) = Pool::find(&m) else { return println!("no projectile pool") };
    let il = Il2::scan(&m, &tracers::WANT[..1]);
    let Some(kp) = il.class(tracers::WANT[0]) else { return println!("no Projectile class") };
    let off = |n: &str| il2::fields(&m, kp).into_iter().find(|f| !f.stat && f.name == n).map(|f| f.off as usize - 0x10);
    let (Some(replay), Some(dist), Some(source), Some(tracer)) = (off("replayTracer"), off("distance"), off("source"), off("tracer")) else { return println!("fields missing") };
    println!("logging bullets for {secs}s, fire now");
    let mut seen = std::collections::HashSet::new();
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(secs) {
        for (_, e) in p.live(&m) {
            let serial = u32::from_le_bytes(e[p.serial..p.serial + 4].try_into().unwrap());
            if !seen.insert(serial) {
                continue;
            }
            let (pos, anchor) = (f3(&e, p.pos), f3(&e, p.anchor));
            let src = u64::from_le_bytes(e[source..source + 8].try_into().unwrap());
            let class = if crate::os::mem::ok(src) { il2::class_name(&m, m.q(src)).unwrap_or_default() } else { "-".into() };
            println!(
                "{:6.2}s #{serial} auth {} replay {} tracer {} alive {} age {:.3} flown {:.1}m spawn {:.1}m from muzzle, source {class}",
                t.elapsed().as_secs_f32(),
                e[p.auth],
                e[replay],
                e[tracer],
                e[p.alive],
                f32::from_le_bytes(e[p.age..p.age + 4].try_into().unwrap()),
                f32::from_le_bytes(e[dist..dist + 4].try_into().unwrap()),
                len(sub(pos, anchor)),
            );
        }
        std::thread::sleep(Duration::from_micros(300));
    }
    println!("done, {} bullets", seen.len());
}
