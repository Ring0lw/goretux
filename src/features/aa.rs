use crate::game::Snap;
use crate::game::il2;
use crate::game::remote;
use crate::game::undo;
use crate::os::mem::Mem;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pitch {
    Off,
    Down,
    Up,
    Jitter,
}

impl Pitch {
    pub const ALL: [Pitch; 4] = [Pitch::Off, Pitch::Down, Pitch::Up, Pitch::Jitter];

    pub fn label(self) -> &'static str {
        match self {
            Pitch::Off => "off",
            Pitch::Down => "down",
            Pitch::Up => "up",
            Pitch::Jitter => "jitter",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Yaw {
    Off,
    Back,
    Spin,
    Jitter,
}

impl Yaw {
    pub const ALL: [Yaw; 4] = [Yaw::Off, Yaw::Back, Yaw::Spin, Yaw::Jitter];

    pub fn label(self) -> &'static str {
        match self {
            Yaw::Off => "off",
            Yaw::Back => "backwards",
            Yaw::Spin => "spin",
            Yaw::Jitter => "jitter",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Cfg {
    pub pitch: Pitch,
    pub yaw: Yaw,
    pub spin: f32,
}

impl Default for Cfg {
    fn default() -> Cfg {
        Cfg { pitch: Pitch::Off, yaw: Yaw::Off, spin: 720.0 }
    }
}

const VALS: u64 = 0x48;

const EVERY: Duration = Duration::from_millis(10);

const FLIP: f32 = 0.07;

const SWING: f32 = 60.0;

const HEAD: &[u8] = &[0xf3, 0x41, 0x0f, 0x58, 0xc1, 0xf3, 0x0f, 0x5e, 0x05];
const HEAD_CALL: u64 = 21;

const YAW: &[u8] = &[0x41, 0x0f, 0x28, 0xcc, 0x0f, 0x28, 0xc7, 0x41, 0xbe, 0x11, 0x00, 0x00, 0x00, 0xe8];
const YAW_CALL: u64 = 13;

const AIM: &[u8] = &[0xf3, 0x41, 0x0f, 0x58, 0xf1, 0xbd, 0x12, 0x00, 0x00, 0x00, 0xf3, 0x0f, 0x5e, 0x35];
const AIM_TAIL: &[u8] = &[0xf3, 0x41, 0x0f, 0x59, 0xf4, 0x0f, 0x28, 0xc6, 0xe8];
const AIM_CALL: u64 = 26;

#[derive(Clone, Copy)]
struct Patch {
    at: u64,
    orig: [u8; 8],
    ours: [u8; 8],
    on: bool,
}

impl Patch {
    fn new(m: &Mem, addr: u64, bytes: &[u8]) -> Result<Patch, String> {
        let at = addr & !7;
        let off = (addr - at) as usize;
        if off + bytes.len() > 8 {
            return Err("a patch spot straddles a qword".into());
        }
        let orig: [u8; 8] = m.bytes(at, 8).ok_or("can't read the game's code")?.try_into().unwrap();
        let mut ours = orig;
        ours[off..off + bytes.len()].copy_from_slice(bytes);
        Ok(Patch { at, orig, ours, on: false })
    }

    fn set(&mut self, m: &Mem, on: bool) {
        if self.on == on {
            return;
        }
        let (from, to) = if on { (self.orig, self.ours) } else { (self.ours, self.orig) };

        if m.bytes(self.at, 8).as_deref() != Some(&from[..]) || !m.write(self.at, &to) {
            return;
        }
        if on {
            undo::keep(m.fd(), self.at, self.at, u64::from_le_bytes(self.ours), &self.orig);
        } else {
            undo::forget(self.at);
        }
        self.on = on;
    }
}

struct Sites {
    head: Patch,
    yaw: Patch,
    aim: Patch,
    vals: u64,
}

fn rel(from_end: u64, to: u64) -> [u8; 4] {
    ((to as i64 - from_end as i64) as i32).to_le_bytes()
}

fn find(code: &[u8], pat: &[u8]) -> Option<usize> {
    code.windows(pat.len()).position(|w| w == pat)
}

impl Sites {
    fn resolve(m: &Mem, k_pns: u64) -> Result<Sites, String> {
        let (head, yaw, aim) = il2::methods(m, k_pns)
            .into_iter()
            .filter(|f| f.ptr != 0)
            .find_map(|f| {
                let code = m.bytes(f.ptr, 0x2000)?;
                let (h, y, a) = (find(&code, HEAD)?, find(&code, YAW)?, find(&code, AIM)?);
                let call = |o: usize| code.get(o) == Some(&0xe8);
                (call(h + HEAD_CALL as usize) && code.get(a + 18..a + 27) == Some(AIM_TAIL) && call(a + AIM_CALL as usize)).then_some((f.ptr + h as u64, f.ptr + y as u64, f.ptr + a as u64))
            })
            .ok_or("the snapshot code changed in this game version")?;
        let code = remote::spare(m, head)?;
        let vals = code + VALS;

        let (s_head, s_yaw, s_aim) = (code, code + 0x10, code + 0x20);
        let mut stubs = vec![0u8; VALS as usize];
        let mut put = |at: u64, b: &[u8]| stubs[(at - code) as usize..][..b.len()].copy_from_slice(b);
        let mut b = vec![0xf3, 0x0f, 0x10, 0x05];
        b.extend(rel(s_head + 8, vals));
        b.push(0xe9);
        b.extend(rel(s_head + 13, head + HEAD_CALL));
        put(s_head, &b);
        let mut b = vec![0xf3, 0x0f, 0x10, 0x05];
        b.extend(rel(s_yaw + 8, vals + 4));
        b.push(0xc3);
        put(s_yaw, &b);
        let mut b = vec![0xf3, 0x0f, 0x10, 0x35];
        b.extend(rel(s_aim + 8, vals + 8));
        b.extend([0xbd, 0x12, 0x00, 0x00, 0x00, 0x0f, 0x28, 0xc6, 0xe9]);
        b.extend(rel(s_aim + 21, aim + AIM_CALL));
        put(s_aim, &b);

        let cur = m.bytes(code, remote::SPARE as usize).ok_or("can't read the code tail")?;
        if cur[..stubs.len()] != stubs[..] {
            if cur.iter().any(|&c| c != 0) {
                return Err("the code tail is in use".into());
            }
            if !m.write(code, &stubs) {
                return Err("writing the stubs failed".into());
            }
        }
        let jmp = |at: u64, to: u64| -> Result<Patch, String> {
            let mut b = vec![0xe9];
            b.extend(rel(at + 5, to));
            Patch::new(m, at, &b)
        };
        let head_p = jmp(head, s_head)?;
        let aim_p = jmp(aim, s_aim)?;
        let call = yaw + YAW_CALL;
        let yaw_p = Patch::new(m, call + 1, &rel(call + 5, s_yaw))?;
        Ok(Sites { head: head_p, yaw: yaw_p, aim: aim_p, vals })
    }
}

#[derive(Default)]
pub struct AntiAim {
    sites: Option<Sites>,
    failed: bool,
    born: Option<Instant>,
    wrote: Option<(Instant, [u8; 12])>,
    pub status: String,
}

fn byte(pitch: f32) -> f32 {
    (pitch.clamp(-90.0, 90.0) + 90.0) / 180.0 * 255.0
}

impl AntiAim {
    pub fn tick(&mut self, m: &Mem, k_pns: u64, s: &Snap, cfg: &Cfg) {
        let want = cfg.pitch != Pitch::Off || cfg.yaw != Yaw::Off;
        if !want {
            self.off(m);
            self.status.clear();
            return;
        }
        if self.sites.is_none() {
            if self.failed {
                return;
            }
            match Sites::resolve(m, k_pns) {
                Ok(x) => self.sites = Some(x),
                Err(e) => {
                    (self.failed, self.status) = (true, format!("anti-aim: {e}"));
                    return;
                }
            }
        }
        let t = self.born.get_or_insert_with(Instant::now).elapsed().as_secs_f32();
        let flip = (t / FLIP) as u64 % 2 == 0;
        let (real_yaw, real_pitch) = crate::features::aim::facing(&s.view);
        let pitch = match cfg.pitch {
            Pitch::Off => -real_pitch,
            Pitch::Down => 90.0,
            Pitch::Up => -90.0,
            Pitch::Jitter => {
                if flip {
                    90.0
                } else {
                    -90.0
                }
            }
        };
        let yaw = match cfg.yaw {
            Yaw::Off => real_yaw,
            Yaw::Back => real_yaw + 180.0,
            Yaw::Spin => t * cfg.spin,
            Yaw::Jitter => real_yaw + 180.0 + if flip { SWING } else { -SWING },
        }
        .rem_euclid(360.0);
        let x = self.sites.as_mut().unwrap();
        let mut vals = [0u8; 12];
        for (i, v) in [byte(pitch), yaw, byte(pitch)].iter().enumerate() {
            vals[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }

        if self.wrote.is_none_or(|(at, v)| v != vals && at.elapsed() >= EVERY) {
            m.write(x.vals, &vals);
            self.wrote = Some((Instant::now(), vals));
        }
        x.head.set(m, cfg.pitch != Pitch::Off);
        x.aim.set(m, cfg.pitch != Pitch::Off);
        x.yaw.set(m, cfg.yaw != Yaw::Off);
        self.status = format!("anti-aim: pitch {} yaw {}", cfg.pitch.label(), cfg.yaw.label());
    }

    pub fn off(&mut self, m: &Mem) {
        if let Some(x) = self.sites.as_mut() {
            for p in [&mut x.head, &mut x.aim, &mut x.yaw] {
                p.set(m, false);
            }
        }
    }

    pub fn idle(&self) -> bool {
        self.sites.as_ref().is_none_or(|x| !x.head.on && !x.aim.on && !x.yaw.on)
    }
}

pub fn dry(m: &Mem, k_pns: u64) -> Result<String, String> {
    let x = Sites::resolve(m, k_pns)?;
    let code = remote::spare(m, x.head.at)?;
    Ok(format!("stubs {code:#x} values {:#x}  patch qwords head {:#x} yaw {:#x} aim {:#x}", x.vals, x.head.at, x.yaw.at, x.aim.at))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pitch_bytes() {
        assert_eq!(byte(-90.0), 0.0);
        assert_eq!(byte(90.0), 255.0);
        assert!((byte(0.0) - 127.5).abs() < 1e-4);
    }

    #[test]
    fn patterns_hold_the_calls_where_expected() {
        assert_eq!(YAW[YAW_CALL as usize], 0xe8);
        assert_eq!(AIM.len() + 4 + AIM_TAIL.len(), AIM_CALL as usize + 1);

        assert!(0x35 <= VALS && VALS + 12 <= remote::SPARE);
    }
}
