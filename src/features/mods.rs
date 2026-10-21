use crate::os::mem::Mem;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const SPREAD: [&str; 7] = ["hipSpreadDegrees", "adsSpreadDegrees", "movementSpreadPerMs", "bloomPerShotDegrees", "maxBloomDegrees", "visionSpreadDegrees", "armSpreadDegrees"];

#[derive(Clone, Copy)]
pub struct Cfg {
    pub no_recoil: bool,
    pub no_spread: bool,
    pub no_sway: bool,
    pub zoom: bool,
    pub zoom_mag: f32,
}

impl Default for Cfg {
    fn default() -> Cfg {
        Cfg { no_recoil: false, no_spread: false, no_sway: false, zoom: false, zoom_mag: 3.0 }
    }
}

#[derive(Default)]
pub struct Mods {
    pub cfg: Mutex<Cfg>,
    quit: AtomicBool,
    clean: AtomicBool,
}

impl Mods {
    pub fn shutdown(&self) {
        self.quit.store(true, Ordering::Relaxed);
        let t = Instant::now();
        while !self.clean.load(Ordering::Relaxed) && t.elapsed() < Duration::from_millis(300) {
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    pub fn quitting(&self) -> bool {
        self.quit.load(Ordering::Relaxed)
    }

    pub fn set_clean(&self, v: bool) {
        self.clean.store(v, Ordering::Relaxed);
    }
}

#[derive(Clone, Copy, Default)]
pub struct Off {
    pub gun_recoil: u64,
    pub gun_cam: u64,
    pub gun_spread: [u64; 7],
    pub vm_recoil: u64,
    pub vm_sway: u64,
    pub vm_mag: u64,
    pub vm_fov: u64,
    pub ml_drift: u64,
    pub ml_drift_deg: u64,
}

#[derive(Clone, Copy, Default)]
pub struct Live {
    pub gun: u64,
    pub vm: u64,
    pub ml: u64,
}

#[derive(Default)]
pub struct Writer {
    orig: HashMap<u64, (u64, u64, Vec<u8>)>,
}

impl Writer {
    pub fn tick(&mut self, m: &Mem, cfg: Cfg, off: Option<Off>, l: Live, quit: bool) {
        let mut want: Vec<(u64, u64, Vec<u8>)> = vec![];
        if let (Some(o), false) = (off, quit) {
            let f = |v: f32| v.to_le_bytes().to_vec();
            let mut put = |obj: u64, at: u64, v: Vec<u8>| {
                if obj != 0 && at != 0 {
                    want.push((obj, obj + at, v));
                }
            };
            if cfg.no_recoil {
                put(l.gun, o.gun_recoil, f(0.0));
                put(l.vm, o.vm_recoil, vec![0]);
            }
            if cfg.no_spread {
                for at in o.gun_spread {
                    put(l.gun, at, f(0.0));
                }
            }
            if cfg.no_sway {
                put(l.ml, o.ml_drift, vec![0]);
                put(l.ml, o.ml_drift_deg, f(0.0));
                put(l.vm, o.vm_sway, vec![0]);
            }
            if cfg.zoom {
                put(l.vm, o.vm_mag, f(cfg.zoom_mag));
                put(l.vm, o.vm_fov, f(0.0));
            }
        }
        for (obj, a, v) in &want {
            if !self.orig.contains_key(a) {
                let Some(old) = m.bytes(*a, v.len()) else { continue };
                crate::game::undo::keep(m.fd(), *a, *obj, m.q(*obj), &old);
                self.orig.insert(*a, (*obj, m.q(*obj), old));
            }
            if m.bytes(*a, v.len()).as_ref() != Some(v) {
                m.write(*a, v);
            }
        }
        self.orig.retain(|a, (obj, k, old)| {
            if want.iter().any(|(_, w, _)| w == a) {
                return true;
            }
            if m.q(*obj) == *k {
                m.write(*a, old);
            }
            crate::game::undo::forget(*a);
            false
        });
    }

    pub fn is_clean(&self) -> bool {
        self.orig.is_empty()
    }
}
