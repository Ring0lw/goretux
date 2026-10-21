use crate::game::il2;
use crate::game::sdk::HitDefs;
use crate::os::mem::{self, Mem};
use std::collections::HashMap;
use std::time::{Duration, Instant};

const SYS_LEN: usize = 0x1c0;

const REFRESH: Duration = Duration::from_millis(10);
const LATE: Duration = Duration::from_millis(60);

#[derive(Clone)]
pub struct Hit {
    pub at: Instant,
    pub name: String,
    pub npc: bool,
    pub part: u8,
    pub kind: u8,
}

pub fn part(l: u8) -> &'static str {
    match l {
        1 => "pelvis",
        2 => "stomach",
        3 => "upper stomach",
        4 => "chest",
        5 => "neck",
        6 => "head",
        7 => "left upper arm",
        8 => "left forearm",
        9 => "left hand",
        10 => "right upper arm",
        11 => "right forearm",
        12 => "right hand",
        13 => "left thigh",
        14 => "left shin",
        15 => "left foot",
        16 => "right thigh",
        17 => "right shin",
        18 => "right foot",
        _ => "body",
    }
}

#[derive(Default)]
pub struct Watch {
    last: Option<f32>,
    pending: Option<(Instant, u8)>,
    blobs: HashMap<u64, Vec<u8>>,
    refreshed: Option<Instant>,

    slot: Option<(u64, u64)>,
}

impl Watch {
    pub fn tick(&mut self, m: &Mem, d: &HitDefs, bodies: &[(u64, u64)]) -> Option<(u64, u8, u8)> {
        let base = m.q(d.k_hm + il2::K_STATICS);
        let t: f32 = m.rd(base + d.time)?;
        if self.last.is_some_and(|l| l != t) {
            self.pending = Some((Instant::now(), m.u8(base + d.kind)));
        }
        self.last = Some(t);

        let Some((at, kind)) = self.pending else {
            if self.refreshed.is_none_or(|r| r.elapsed() >= REFRESH) {
                self.refreshed = Some(Instant::now());
                self.blobs = bodies.iter().filter_map(|&(id, sys)| Some((id, m.bytes(sys, SYS_LEN)?))).collect();
            }
            return None;
        };
        let mut found = None;
        for &(id, sys) in bodies {
            let Some(now) = m.bytes(sys, SYS_LEN) else { continue };
            if let Some(old) = self.blobs.remove(&id)
                && old != now
                && found.is_none()
            {
                found = self.limb(m, d, &old, &now).map(|l| (id, l, kind));
            }
            self.blobs.insert(id, now);
        }
        if found.is_some() || at.elapsed() > LATE {
            self.pending = None;
        }
        found
    }

    fn limb(&mut self, m: &Mem, d: &HitDefs, old: &[u8], now: &[u8]) -> Option<u8> {
        let ptr = |b: &[u8], o: usize| u64::from_le_bytes(b[o..o + 8].try_into().unwrap());
        if self.slot.is_none() {
            for o in (0..SYS_LEN - 8).step_by(8) {
                let p = ptr(now, o);
                if p == ptr(old, o) || !mem::ok(p) {
                    continue;
                }
                let first = il2::fields(m, m.q(p)).into_iter().find(|f| !f.stat);
                if let Some(f) = first.filter(|f| f.ty == d.limb_ty) {
                    self.slot = Some((o as u64, f.off));
                    break;
                }
            }
        }
        let (slot, off) = self.slot?;
        let l = m.u8(ptr(now, slot as usize) + off);
        (1..=18).contains(&l).then_some(l)
    }
}
