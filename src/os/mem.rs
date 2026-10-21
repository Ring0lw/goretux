use std::fs::{self, File, OpenOptions};
use std::os::fd::AsRawFd;
use std::os::unix::fs::FileExt;

pub trait Pod: Copy + Default {}
impl Pod for u8 {}
impl Pod for u16 {}
impl Pod for u32 {}
impl Pod for i32 {}
impl Pod for u64 {}
impl Pod for f32 {}
impl<const N: usize> Pod for [f32; N] where [f32; N]: Default {}

pub fn ok(a: u64) -> bool {
    (0x10000..0x7fff_ffff_ffff).contains(&a)
}

pub struct Mem {
    f: File,
    pub pid: i32,
}

impl Mem {
    pub fn open(pid: i32) -> std::io::Result<Mem> {
        Ok(Mem { f: OpenOptions::new().read(true).write(true).open(format!("/proc/{pid}/mem"))?, pid })
    }

    pub fn fd(&self) -> i32 {
        self.f.as_raw_fd()
    }

    pub fn alive(&self) -> bool {
        fs::metadata(format!("/proc/{}/mem", self.pid)).is_ok()
    }

    pub fn read(&self, a: u64, buf: &mut [u8]) -> bool {
        ok(a) && ok(a + buf.len() as u64) && self.f.read_exact_at(buf, a).is_ok()
    }

    pub fn write(&self, a: u64, buf: &[u8]) -> bool {
        ok(a) && ok(a + buf.len() as u64) && self.f.write_all_at(buf, a).is_ok()
    }

    pub fn bytes(&self, a: u64, n: usize) -> Option<Vec<u8>> {
        let mut v = vec![0u8; n];
        self.read(a, &mut v).then_some(v)
    }

    pub fn rd<T: Pod>(&self, a: u64) -> Option<T> {
        let mut v = T::default();
        let buf = unsafe { std::slice::from_raw_parts_mut(&mut v as *mut T as *mut u8, size_of::<T>()) };
        self.read(a, buf).then_some(v)
    }

    pub fn q(&self, a: u64) -> u64 {
        self.rd::<u64>(a).unwrap_or(0)
    }

    pub fn i32(&self, a: u64) -> i32 {
        self.rd::<i32>(a).unwrap_or(0)
    }

    pub fn u32(&self, a: u64) -> u32 {
        self.rd::<u32>(a).unwrap_or(0)
    }

    pub fn u8(&self, a: u64) -> u8 {
        self.rd::<u8>(a).unwrap_or(0)
    }

    pub fn cstr(&self, a: u64, max: usize) -> Option<String> {
        let mut b = vec![0u8; max];

        if !self.read(a, &mut b) {
            b.truncate(32);
            if !self.read(a, &mut b) {
                return None;
            }
        }
        let z = b.iter().position(|&c| c == 0)?;
        String::from_utf8(b[..z].to_vec()).ok()
    }

    pub fn mstr(&self, s: u64) -> Option<String> {
        if !ok(s) {
            return None;
        }
        let n = self.i32(s + 0x10);
        if !(0..=256).contains(&n) {
            return None;
        }
        let b = self.bytes(s + 0x14, n as usize * 2)?;
        let w: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        Some(String::from_utf16_lossy(&w))
    }
}

pub struct Region {
    pub lo: u64,
    pub hi: u64,
    pub r: bool,
    pub w: bool,
    pub x: bool,
    pub path: String,
}

pub fn maps(pid: i32) -> Vec<Region> {
    let Ok(s) = fs::read_to_string(format!("/proc/{pid}/maps")) else { return vec![] };
    s.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let (lo, hi) = it.next()?.split_once('-')?;
            let perm = it.next()?.as_bytes();
            let path = it.nth(3).unwrap_or("").to_string();
            Some(Region { lo: u64::from_str_radix(lo, 16).ok()?, hi: u64::from_str_radix(hi, 16).ok()?, r: perm[0] == b'r', w: perm[1] == b'w', x: perm[2] == b'x', path })
        })
        .collect()
}

pub fn objects(m: &Mem, k: u64) -> Vec<u64> {
    find(m, k, 16)
}

pub fn find(m: &Mem, v: u64, align: usize) -> Vec<u64> {
    const CHUNK: usize = 16 << 20;
    let mut out = vec![];
    let mut buf = vec![0u8; CHUNK];
    for r in maps(m.pid) {
        if !r.r || !r.w || !(r.path.is_empty() || r.path.starts_with("[anon")) || r.hi - r.lo > 1 << 30 {
            continue;
        }
        let mut at = r.lo;
        while at < r.hi {
            let n = ((r.hi - at) as usize).min(CHUNK);
            if !m.read(at, &mut buf[..n]) {
                break;
            }
            for (i, c) in buf[..n].chunks_exact(align).enumerate() {
                if u64::from_le_bytes(c[..8].try_into().unwrap()) == v {
                    out.push(at + (i * align) as u64);
                }
            }
            at += n as u64;
        }
    }
    out
}

pub fn find_pid() -> Option<i32> {
    for e in fs::read_dir("/proc").ok()?.flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<i32>() else { continue };
        let comm = fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
        let named = comm.trim() == "GoreBox.exe" || fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|c| c.split(|&b| b == 0).any(|a| a.to_ascii_lowercase().ends_with(b"gorebox.exe")));
        if named && maps(pid).iter().any(|r| r.path.ends_with("GameAssembly.dll")) {
            return Some(pid);
        }
    }
    None
}

pub fn open_error(pid: i32, e: &std::io::Error) -> String {
    let scope = fs::read_to_string("/proc/sys/kernel/yama/ptrace_scope").ok().and_then(|s| s.trim().parse::<u8>().ok());
    match (e.kind(), scope) {
        (std::io::ErrorKind::PermissionDenied, Some(3)) => {
            "can't read the game: kernel.yama.ptrace_scope is 3, nothing may read another process until a reboot with a lower value (see compatibility.md)".into()
        }
        (std::io::ErrorKind::PermissionDenied, Some(s)) if s > 0 => format!("can't read the game: kernel.yama.ptrace_scope is {s}, run setup.sh (see compatibility.md)"),
        _ => format!("cannot open /proc/{pid}/mem: {e}"),
    }
}
