use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;

const EVIOCGRAB: u64 = 0x4004_4590;
const O_NONBLOCK: i32 = 0o4000;
const EV_KEY: u16 = 1;
const EV_REL: u16 = 2;
const REL_X: u16 = 0;
const REL_Y: u16 = 1;
const REL_HWHEEL: u16 = 6;
const REL_WHEEL: u16 = 8;
const BTN_LEFT: u16 = 0x110;
const BTN_RIGHT: u16 = 0x111;
const BTN_MIDDLE: u16 = 0x112;

const EV_SIZE: usize = 24;

unsafe extern "C" {
    fn ioctl(fd: i32, req: u64, ...) -> i32;
}

#[derive(Default)]
pub struct Grab {
    devs: Vec<File>,
    pub on: bool,
    pub pos: [f32; 2],
    pub btn: [bool; 3],
    pub wheel: [f32; 2],
}

fn bit(caps: &str, n: u16) -> bool {
    let n = n as usize;
    caps.split_whitespace().rev().nth(n / 64).and_then(|w| u64::from_str_radix(w, 16).ok()).is_some_and(|w| w >> (n % 64) & 1 != 0)
}

impl Grab {
    pub fn open() -> Grab {
        let mut devs = vec![];
        let mut names = vec![];
        for e in fs::read_dir("/sys/class/input").into_iter().flatten().flatten() {
            let ev = e.file_name().to_string_lossy().into_owned();
            if !ev.starts_with("event") {
                continue;
            }
            let rd = |f: &str| fs::read_to_string(e.path().join("device").join(f)).unwrap_or_default();
            let (name, rel, key) = (rd("name").trim().to_string(), rd("capabilities/rel"), rd("capabilities/key"));

            if name == "goretux-aim" || !bit(&rel, REL_X) || !bit(&rel, REL_Y) || !bit(&key, BTN_LEFT) {
                continue;
            }
            match OpenOptions::new().read(true).custom_flags(O_NONBLOCK).open(format!("/dev/input/{ev}")) {
                Ok(f) => {
                    devs.push(f);
                    names.push(format!("{ev} {name}"));
                }
                Err(err) => eprintln!("menu mouse: /dev/input/{ev} ({name}): {err}"),
            }
        }
        eprintln!("menu mouse: {}", if names.is_empty() { "no mice readable, falling back to the X pointer".into() } else { names.join(", ") });
        Grab { devs, ..Default::default() }
    }

    pub fn set(&mut self, on: bool, pos: [f32; 2]) -> bool {
        let mut any = false;
        for f in &self.devs {
            any |= unsafe { ioctl(f.as_raw_fd(), EVIOCGRAB, on as i64) } == 0;
        }
        (self.on, self.pos, self.btn, self.wheel) = (on && any, pos, [false; 3], [0.0; 2]);
        self.on
    }

    pub fn poll(&mut self, size: [f32; 2], speed: f32) {
        let mut buf = [0u8; EV_SIZE * 64];
        for f in &mut self.devs {
            while let Ok(n @ 1..) = f.read(&mut buf) {
                if !self.on {
                    continue;
                }
                for e in buf[..n].chunks_exact(EV_SIZE) {
                    let (ty, code) = (u16::from_le_bytes([e[16], e[17]]), u16::from_le_bytes([e[18], e[19]]));
                    let v = i32::from_le_bytes([e[20], e[21], e[22], e[23]]);
                    match (ty, code) {
                        (EV_REL, REL_X) => self.pos[0] += v as f32 * speed,
                        (EV_REL, REL_Y) => self.pos[1] += v as f32 * speed,
                        (EV_REL, REL_WHEEL) => self.wheel[1] += v as f32,
                        (EV_REL, REL_HWHEEL) => self.wheel[0] += v as f32,
                        (EV_KEY, BTN_LEFT) => self.btn[0] = v != 0,
                        (EV_KEY, BTN_RIGHT) => self.btn[1] = v != 0,
                        (EV_KEY, BTN_MIDDLE) => self.btn[2] = v != 0,
                        _ => {}
                    }
                }
            }
        }
        self.pos = [self.pos[0].clamp(0.0, size[0] - 1.0), self.pos[1].clamp(0.0, size[1] - 1.0)];
    }
}

#[cfg(test)]
mod tests {
    use super::bit;

    #[test]
    fn caps_bits() {
        let key = "0 0 0 0 1f0000 0 0 0 0";
        assert!(bit(key, 0x110) && bit(key, 0x114) && !bit(key, 0x115) && !bit(key, 0));
        assert!(bit("903", 0) && bit("903", 1) && bit("903", 8) && !bit("903", 6));
    }
}
