use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};

const SLOTS: usize = 64;
const FREE: u64 = 0;
const BUSY: u64 = 1;
const READY: u64 = 2;

struct Slot {
    state: AtomicU64,
    fd: AtomicI32,
    addr: AtomicU64,
    obj: AtomicU64,
    class: AtomicU64,
    len: AtomicU64,
    data: [AtomicU64; 2],
}

#[allow(clippy::declare_interior_mutable_const)]
const EMPTY: Slot = Slot {
    state: AtomicU64::new(FREE),
    fd: AtomicI32::new(-1),
    addr: AtomicU64::new(0),
    obj: AtomicU64::new(0),
    class: AtomicU64::new(0),
    len: AtomicU64::new(0),
    data: [AtomicU64::new(0), AtomicU64::new(0)],
};
static TABLE: [Slot; SLOTS] = [EMPTY; SLOTS];

unsafe extern "C" {
    fn pread(fd: i32, buf: *mut u8, n: usize, off: i64) -> isize;
    fn pwrite(fd: i32, buf: *const u8, n: usize, off: i64) -> isize;
    fn signal(sig: i32, handler: usize) -> usize;
    fn raise(sig: i32) -> i32;
}

pub fn keep(fd: i32, addr: u64, obj: u64, class: u64, orig: &[u8]) {
    if orig.len() > 16 || TABLE.iter().any(|s| s.state.load(Ordering::Acquire) == READY && s.addr.load(Ordering::Relaxed) == addr) {
        return;
    }
    let Some(s) = TABLE.iter().find(|s| s.state.compare_exchange(FREE, BUSY, Ordering::Acquire, Ordering::Relaxed).is_ok()) else { return };
    let mut b = [0u8; 16];
    b[..orig.len()].copy_from_slice(orig);
    s.fd.store(fd, Ordering::Relaxed);
    s.addr.store(addr, Ordering::Relaxed);
    s.obj.store(obj, Ordering::Relaxed);
    s.class.store(class, Ordering::Relaxed);
    s.len.store(orig.len() as u64, Ordering::Relaxed);
    s.data[0].store(u64::from_le_bytes(b[..8].try_into().unwrap()), Ordering::Relaxed);
    s.data[1].store(u64::from_le_bytes(b[8..].try_into().unwrap()), Ordering::Relaxed);
    s.state.store(READY, Ordering::Release);
}

pub fn forget(addr: u64) {
    for s in &TABLE {
        if s.state.load(Ordering::Acquire) == READY && s.addr.load(Ordering::Relaxed) == addr {
            s.state.store(FREE, Ordering::Release);
        }
    }
}

fn restore_all() {
    for s in &TABLE {
        if s.state.load(Ordering::Acquire) != READY {
            continue;
        }
        let (fd, addr, obj, class, len) =
            (s.fd.load(Ordering::Relaxed), s.addr.load(Ordering::Relaxed), s.obj.load(Ordering::Relaxed), s.class.load(Ordering::Relaxed), s.len.load(Ordering::Relaxed) as usize);
        let mut k = [0u8; 8];
        if unsafe { pread(fd, k.as_mut_ptr(), 8, obj as i64) } != 8 || u64::from_le_bytes(k) != class {
            continue;
        }
        let mut b = [0u8; 16];
        b[..8].copy_from_slice(&s.data[0].load(Ordering::Relaxed).to_le_bytes());
        b[8..].copy_from_slice(&s.data[1].load(Ordering::Relaxed).to_le_bytes());
        unsafe { pwrite(fd, b.as_ptr(), len, addr as i64) };
        s.state.store(FREE, Ordering::Release);
    }
}

extern "C" fn on_fatal(sig: i32) {
    restore_all();
    unsafe {
        signal(sig, 0);
        raise(sig);
    }
}

pub fn install() {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_all();
        hook(info);
    }));

    for sig in [4, 6, 7, 8, 11] {
        unsafe { signal(sig, on_fatal as *const () as usize) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::os::fd::AsRawFd;

    #[test]
    fn puts_back_only_matching_objects() {
        let path = std::env::temp_dir().join(format!("goretux-undo-{}", std::process::id()));
        let mut f = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(true).open(&path).unwrap();

        let mut img = vec![0u8; 0x80];
        img[..8].copy_from_slice(&0xAAu64.to_le_bytes());
        img[0x40..0x48].copy_from_slice(&0xBBu64.to_le_bytes());
        img[0x10..0x14].copy_from_slice(b"orig");
        img[0x50..0x54].copy_from_slice(b"orig");
        f.write_all(&img).unwrap();
        let fd = f.as_raw_fd();
        keep(fd, 0x10, 0x00, 0xAA, b"orig");
        keep(fd, 0x50, 0x40, 0xBB, b"orig");

        f.seek(SeekFrom::Start(0x10)).unwrap();
        f.write_all(b"ours").unwrap();
        f.seek(SeekFrom::Start(0x50)).unwrap();
        f.write_all(b"ours").unwrap();
        f.seek(SeekFrom::Start(0x40)).unwrap();
        f.write_all(&0xCCu64.to_le_bytes()).unwrap();
        restore_all();
        let mut now = vec![0u8; 0x80];
        f.seek(SeekFrom::Start(0)).unwrap();
        f.read_exact(&mut now).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(&now[0x10..0x14], b"orig");
        assert_eq!(&now[0x50..0x54], b"ours");
    }
}
