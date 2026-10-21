use crate::game::il2::{self, Il2};
use crate::os::mem::{self, Mem};
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub const WANT: &[&str] = &["UnityEngine.EventSystems.EventSystem", "System.String"];

const ST_IDLE: u64 = 0;
const ST_PENDING: u64 = 1;
const ST_DONE: u64 = 2;
const MB_STATE: u64 = 0x00;
const MB_COUNT: u64 = 0x08;
const MB_MAGIC: u64 = 0x10;
const MB_HITS: u64 = 0x18;
const MB_ORIG: u64 = 0x20;
const MB_RECS: u64 = 0x40;
const MAGIC: u64 = u64::from_le_bytes(*b"gbremote");

const REC: u64 = 0x58;
const REC_ARGS: u64 = 0x08;
const REC_RESULT: u64 = 0x38;
pub const MAX_CALLS: usize = 14;
const MB_SCRATCH: u64 = MB_RECS + REC * MAX_CALLS as u64;
const MB_SIZE: u64 = 0x700;

pub const SKIPPED: u64 = u64::MAX;

const STR_LEN: u64 = 0x10;
const STR_CHARS: u64 = 0x14;

pub const CACHED_PTR: u64 = 0x10;

const OWN: u64 = 0xffff_0000_0000_0000;
pub const fn own(j: usize) -> u64 {
    OWN | j as u64
}

#[derive(Clone, Copy, Default)]
pub struct Call {
    pub f: u64,
    pub a: [u64; 6],
    pub guard: [u64; 2],

    pub fwd: Option<(usize, usize)>,
}

impl Call {
    pub fn method(m: &il2::Method, args: &[u64]) -> Call {
        let mut a = [0u64; 6];
        a[..args.len()].copy_from_slice(args);
        a[args.len()] = m.info;
        Call { f: m.ptr, a, ..Default::default() }
    }

    fn guard(mut self, addr: u64) -> Call {
        let g = if self.guard[0] == 0 { 0 } else { 1 };
        self.guard[g] = addr;
        self
    }

    pub fn alive(self, obj: u64) -> Call {
        self.guard(obj + CACHED_PTR)
    }

    pub fn needs(self, j: usize) -> Call {
        self.guard(own(j))
    }

    pub fn to(mut self, call: usize, arg: usize) -> Call {
        self.fwd = Some((call, arg));
        self
    }
}

pub fn f32a(v: f32) -> u64 {
    v.to_bits() as u64
}

pub struct Remote {
    func: u64,
    orig: [u8; 8],
    mbox: u64,
    scratch: u64,
    used: u64,
    room: u64,
    strings: HashMap<String, u64>,
    string_class: u64,
    stale: bool,
    _lock: std::fs::File,
}

struct Section {
    end: u64,
    exec: bool,
    write: bool,
}

fn sections(m: &Mem, base: u64) -> Vec<Section> {
    let pe = base + m.u32(base + 0x3c) as u64;
    let n = m.rd::<u16>(pe + 6).unwrap_or(0) as u64;
    let opt = m.rd::<u16>(pe + 20).unwrap_or(0) as u64;
    (0..n)
        .map(|i| {
            let s = pe + 24 + opt + i * 40;
            let (vsize, va, ch) = (m.u32(s + 8) as u64, m.u32(s + 12) as u64, m.u32(s + 36));
            Section { end: base + va + vsize, exec: ch & 0x2000_0000 != 0, write: ch & 0x8000_0000 != 0 }
        })
        .collect()
}

fn tails(m: &Mem, near: u64) -> Result<((u64, u64), (u64, u64)), String> {
    let base = mem::maps(m.pid).into_iter().filter(|r| r.path.ends_with("GameAssembly.dll")).map(|r| r.lo).min().ok_or("GameAssembly not mapped")?;
    let secs = sections(m, base);

    let regions = mem::maps(m.pid);
    let mapped = |at: u64, room: u64, w: bool, x: bool| regions.iter().any(|r| r.lo <= at && at + room <= r.hi && r.r && r.w == w && r.x == x);
    let code = secs
        .iter()
        .filter(|s| s.exec)
        .map(|s| tail(s, 16))
        .filter(|&(at, room)| room >= 0x200 && at.abs_diff(near) < 0x7000_0000 && mapped(at, room, false, true))
        .max_by_key(|t| t.1)
        .ok_or("no room for the stub in GameAssembly's code")?;
    let data = secs.iter().filter(|s| s.write).map(|s| tail(s, 0x40)).filter(|&(at, room)| mapped(at, room, true, false)).max_by_key(|t| t.1).ok_or("no room for the mailbox")?;
    Ok((code, data))
}

pub const SPARE: u64 = 0x60;
pub fn spare(m: &Mem, near: u64) -> Result<u64, String> {
    let ((code, room), _) = tails(m, near)?;
    if room < STUB_MAX + SPARE {
        return Err(format!("no spare room in GameAssembly's code tail ({room:#x})"));
    }
    Ok(code + room - SPARE)
}

const STUB_MAX: u64 = 0x180;

fn tail(s: &Section, align: u64) -> (u64, u64) {
    let at = (s.end + align - 1) & !(align - 1);
    let page = (s.end + 0xfff) & !0xfff;
    (at, page.saturating_sub(at))
}

fn prologue(b: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i < 5 {
        let rest = &b[i..];
        let n = match rest {
            [0x50..=0x57, ..] => 1,
            [0x40..=0x41, 0x50..=0x57, ..] => 2,
            [0x48, 0x83, 0xec, _, ..] => 4,
            [0x48, 0x81, 0xec, _, _, _, _, ..] => 7,
            [0x48, 0x89, 0x5c | 0x6c | 0x74 | 0x7c, 0x24, _, ..] => 5,
            [0x48, 0x8b, 0xc4, ..] => 3,
            _ => return None,
        };
        i += n;
    }
    (i <= 8).then_some(i)
}

#[derive(Default)]
struct Asm {
    b: Vec<u8>,
    labels: HashMap<&'static str, usize>,
    fix: Vec<(usize, &'static str, bool)>,
}

impl Asm {
    fn e(&mut self, bytes: &[u8]) {
        self.b.extend_from_slice(bytes);
    }
    fn q(&mut self, v: u64) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    fn label(&mut self, l: &'static str) {
        self.labels.insert(l, self.b.len());
    }
    fn j8(&mut self, op: u8, l: &'static str) {
        self.e(&[op, 0]);
        self.fix.push((self.b.len() - 1, l, false));
    }
    fn j32(&mut self, op: &[u8], l: &'static str) {
        self.e(op);
        self.e(&[0; 4]);
        self.fix.push((self.b.len() - 4, l, true));
    }
    fn done(mut self) -> Vec<u8> {
        for (at, l, wide) in std::mem::take(&mut self.fix) {
            let to = self.labels[l] as i64;
            if wide {
                let rel = (to - (at as i64 + 4)) as i32;
                self.b[at..at + 4].copy_from_slice(&rel.to_le_bytes());
            } else {
                let rel = to - (at as i64 + 1);
                assert!((-128..128).contains(&rel), "rel8 out of range to {l}");
                self.b[at] = rel as i8 as u8;
            }
        }
        self.b
    }
}

fn stub(at: u64, mbox: u64, stolen: &[u8], back: u64) -> Vec<u8> {
    let mut a = Asm::default();

    a.e(&[0x51, 0x52, 0x41, 0x50, 0x41, 0x51, 0x53, 0x56]);
    a.e(&[0x48, 0x83, 0xec, 0x68]);
    a.e(&[0x48, 0xb8]);
    a.q(mbox);
    a.e(&[0xf0, 0x48, 0xff, 0x40, MB_HITS as u8]);
    a.e(&[0x48, 0x83, 0x38, ST_PENDING as u8]);
    a.j32(&[0x0f, 0x85], "out");
    a.e(&[0x48, 0x8b, 0x70, MB_COUNT as u8]);
    a.e(&[0x48, 0x8d, 0x58, MB_RECS as u8]);
    a.label("loop");
    a.e(&[0x48, 0x85, 0xf6]);
    a.j32(&[0x0f, 0x84], "finish");
    for g in [0x40u8, 0x48] {
        a.e(&[0x48, 0x8b, 0x43, g]);
        a.e(&[0x48, 0x85, 0xc0]);
        a.j8(0x74, if g == 0x40 { "g1" } else { "call" });
        a.e(&[0x48, 0x83, 0x38, 0x00]);
        a.j32(&[0x0f, 0x84], "skip");
        a.label(if g == 0x40 { "g1" } else { "call" });
    }
    a.e(&[0x48, 0x8b, 0x4b, 0x08]);
    a.e(&[0x48, 0x8b, 0x53, 0x10]);
    a.e(&[0x4c, 0x8b, 0x43, 0x18]);
    a.e(&[0x4c, 0x8b, 0x4b, 0x20]);

    a.e(&[0x66, 0x48, 0x0f, 0x6e, 0xc1]);
    a.e(&[0x66, 0x48, 0x0f, 0x6e, 0xca]);
    a.e(&[0x66, 0x49, 0x0f, 0x6e, 0xd0]);
    a.e(&[0x66, 0x49, 0x0f, 0x6e, 0xd9]);
    a.e(&[0x48, 0x8b, 0x43, 0x28]);
    a.e(&[0x48, 0x89, 0x44, 0x24, 0x20]);
    a.e(&[0x48, 0x8b, 0x43, 0x30]);
    a.e(&[0x48, 0x89, 0x44, 0x24, 0x28]);
    a.e(&[0xff, 0x13]);
    a.e(&[0x48, 0x89, 0x43, 0x38]);
    a.e(&[0x48, 0x8b, 0x4b, 0x50]);
    a.e(&[0x48, 0x85, 0xc9]);
    a.j8(0x74, "next");
    a.e(&[0x48, 0x89, 0x01]);
    a.j8(0xeb, "next");
    a.label("skip");
    a.e(&[0x48, 0xc7, 0x43, 0x38, 0xff, 0xff, 0xff, 0xff]);
    a.label("next");
    a.e(&[0x48, 0x83, 0xc3, REC as u8]);
    a.e(&[0x48, 0xff, 0xce]);
    a.j32(&[0xe9], "loop");
    a.label("finish");
    a.e(&[0x48, 0xb8]);
    a.q(mbox);
    a.e(&[0x48, 0xc7, 0x00, ST_DONE as u8, 0, 0, 0]);
    a.label("out");
    a.e(&[0x48, 0x83, 0xc4, 0x68]);
    a.e(&[0x5e, 0x5b, 0x41, 0x59, 0x41, 0x58, 0x5a, 0x59]);
    a.e(stolen);
    let mut b = a.done();
    let rel = back as i64 - (at as i64 + b.len() as i64 + 5);
    b.push(0xe9);
    b.extend_from_slice(&(rel as i32).to_le_bytes());
    b
}

impl Remote {
    pub fn install(m: &Mem, il: &Il2) -> Result<Remote, String> {
        let lock = crate::settings::lock("remote.lock").ok_or("another goretux (the overlay or a tool) is using the game hook")?;
        let k = il.class(WANT[0]).ok_or("no EventSystem class")?;
        let string_class = il.class(WANT[1]).ok_or("no String class")?;
        let func = il2::method(m, k, "Update", &[]).ok_or("no EventSystem.Update")?.ptr;
        let ((stub_at, stub_room), (mbox, room)) = tails(m, func)?;
        if room < MB_SIZE {
            return Err(format!("mailbox room {room:#x} < {MB_SIZE:#x}"));
        }

        let mut cur = [0u8; 8];
        if !m.read(func, &mut cur) {
            return Err("can't read EventSystem.Update".into());
        }

        let ours = m.q(mbox + MB_MAGIC) == MAGIC;
        let hooked = cur[0] == 0xe9;
        if hooked && !ours {
            return Err("EventSystem.Update is hooked by something else".into());
        }
        let orig: [u8; 8] = if ours { m.q(mbox + MB_ORIG).to_le_bytes() } else { cur };
        let n = prologue(&orig).ok_or_else(|| format!("unknown prologue {:02x?}", orig))?;
        let code = stub(stub_at, mbox, &orig[..n], func + n as u64);
        if code.len() as u64 > stub_room {
            return Err("stub too big".into());
        }
        let r = Remote { func, orig, mbox, scratch: mbox + MB_SCRATCH, used: 0, room: MB_SIZE - MB_SCRATCH, strings: HashMap::new(), string_class, stale: false, _lock: lock };
        if !ours && (m.bytes(stub_at, code.len()).is_none_or(|b| b.iter().any(|&c| c != 0)) || m.bytes(mbox, MB_SIZE as usize).is_none_or(|b| b.iter().any(|&c| c != 0))) {
            return Err("section tails are not empty".into());
        }
        if hooked {
            if m.bytes(stub_at, code.len()).as_deref() == Some(&code[..]) {
                let _ = m.write(mbox + MB_STATE, &ST_IDLE.to_le_bytes());
                return Ok(r);
            }

            r.uninstall(m);
            std::thread::sleep(Duration::from_millis(200));
        }
        let mut head = vec![0u8; MB_RECS as usize];
        head[MB_MAGIC as usize..][..8].copy_from_slice(&MAGIC.to_le_bytes());
        head[MB_HITS as usize..][..8].copy_from_slice(&m.q(mbox + MB_HITS).to_le_bytes());
        head[MB_ORIG as usize..][..8].copy_from_slice(&orig);
        if !m.write(mbox, &head) || !m.write(stub_at, &code) {
            return Err("writing the stub failed, the kernel may refuse forced writes to /proc/pid/mem (see compatibility.md)".into());
        }

        let mut patch = orig;
        patch[0] = 0xe9;
        patch[1..5].copy_from_slice(&((stub_at as i64 - (func as i64 + 5)) as i32).to_le_bytes());
        patch[5..n].fill(0x90);
        if func % 8 != 0 || !m.write(func, &patch) {
            return Err("writing the hook failed, the kernel may refuse forced writes to /proc/pid/mem (see compatibility.md)".into());
        }
        Ok(r)
    }

    pub fn hits(&self, m: &Mem) -> u64 {
        m.q(self.mbox + MB_HITS)
    }

    pub fn uninstall(&self, m: &Mem) {
        let _ = m.write(self.func, &self.orig);
    }

    pub fn batch(&mut self, m: &Mem, calls: &[Call]) -> Result<Vec<Option<u64>>, String> {
        assert!(calls.len() <= MAX_CALLS);
        if calls.is_empty() {
            return Ok(vec![]);
        }
        if self.stale || m.q(self.mbox + MB_STATE) == ST_PENDING {
            return Err("the previous batch never ran".into());
        }
        let rec = |i: usize| self.mbox + MB_RECS + i as u64 * REC;
        let arg = |i: usize, j: u64| rec(i) + REC_ARGS + j * 8;
        let fix = |i: usize, v: u64| if v & OWN == OWN { arg(i, v & 7) } else { v };
        let mut buf = vec![0u8; calls.len() * REC as usize];
        for (i, (c, r)) in calls.iter().zip(buf.chunks_exact_mut(REC as usize)).enumerate() {
            let fwd = c.fwd.map_or(0, |(k, j)| {
                assert!(k > i && k < calls.len() && j < 6, "forward to a later call");
                arg(k, j as u64)
            });
            let a = c.a.map(|v| fix(i, v));
            let words = [c.f, a[0], a[1], a[2], a[3], a[4], a[5], 0, fix(i, c.guard[0]), fix(i, c.guard[1]), fwd];
            for (w, o) in words.iter().zip(r.chunks_exact_mut(8)) {
                o.copy_from_slice(&w.to_le_bytes());
            }
        }
        let ok = m.write(self.mbox + MB_RECS, &buf) && m.write(self.mbox + MB_COUNT, &(calls.len() as u64).to_le_bytes()) && m.write(self.mbox + MB_STATE, &ST_PENDING.to_le_bytes());
        if !ok {
            return Err("mailbox write failed".into());
        }
        let t = Instant::now();
        while m.q(self.mbox + MB_STATE) != ST_DONE {
            if t.elapsed() > Duration::from_secs(3) {
                if m.write(self.mbox + MB_STATE, &ST_IDLE.to_le_bytes()) {
                    return Err("timed out waiting for the game's main thread".into());
                }
                self.stale = true;
                return Err("game gone".into());
            }
            std::thread::sleep(Duration::from_micros(500));
        }
        let mut out = Vec::with_capacity(calls.len());
        for i in 0..calls.len() as u64 {
            let r = m.q(self.mbox + MB_RECS + i * REC + REC_RESULT);
            out.push((r != SKIPPED).then_some(r));
        }
        let _ = m.write(self.mbox + MB_STATE, &ST_IDLE.to_le_bytes());
        Ok(out)
    }

    pub fn call(&mut self, m: &Mem, c: Call) -> Result<Option<u64>, String> {
        Ok(self.batch(m, &[c])?[0])
    }

    pub fn each(&mut self, m: &Mem, calls: &[Call]) -> Result<Vec<Option<u64>>, String> {
        let mut out = Vec::with_capacity(calls.len());
        for c in calls.chunks(MAX_CALLS) {
            out.extend(self.batch(m, c)?);
        }
        Ok(out)
    }

    pub fn groups(&mut self, m: &Mem, groups: &[Vec<Call>]) -> Result<Vec<Vec<Option<u64>>>, String> {
        let mut out = Vec::with_capacity(groups.len());
        let mut i = 0;
        while i < groups.len() {
            let mut batch: Vec<Call> = vec![];
            let mut sizes = vec![];
            while i < groups.len() && batch.len() + groups[i].len() <= MAX_CALLS {
                let base = batch.len();
                batch.extend(groups[i].iter().map(|c| Call { fwd: c.fwd.map(|(k, j)| (base + k, j)), ..*c }));
                sizes.push(groups[i].len());
                i += 1;
            }
            assert!(!sizes.is_empty(), "a call group bigger than a batch");
            let mut res = self.batch(m, &batch)?.into_iter();
            for n in sizes {
                out.push(res.by_ref().take(n).collect());
            }
        }
        Ok(out)
    }

    pub fn grow(&mut self, at: u64, size: u64) {
        (self.scratch, self.used, self.room) = (at, 0, size);
    }

    pub fn alloc(&mut self, m: &Mem, data: &[u8]) -> Option<u64> {
        let n = (data.len() as u64 + 15) & !15;
        if self.used + n > self.room {
            return None;
        }
        let at = self.scratch + self.used;
        m.write(at, data).then(|| self.used += n)?;
        Some(at)
    }

    pub fn string(&mut self, m: &Mem, s: &str) -> Option<u64> {
        if let Some(&a) = self.strings.get(s) {
            return Some(a);
        }
        let units: Vec<u16> = s.encode_utf16().collect();
        let mut b = vec![0u8; STR_CHARS as usize + units.len() * 2 + 2];
        b[..8].copy_from_slice(&self.string_class.to_le_bytes());
        b[STR_LEN as usize..][..4].copy_from_slice(&(units.len() as i32).to_le_bytes());
        for (i, u) in units.iter().enumerate() {
            b[STR_CHARS as usize + i * 2..][..2].copy_from_slice(&u.to_le_bytes());
        }
        let a = self.alloc(m, &b)?;
        self.strings.insert(s.to_string(), a);
        Some(a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prologues() {
        assert_eq!(prologue(&[0x40, 0x56, 0x48, 0x83, 0xec, 0x20, 0x80, 0x3d]), Some(6));
        assert_eq!(prologue(&[0x48, 0x89, 0x5c, 0x24, 0x08, 0x57, 0x48, 0x83]), Some(5));
        assert_eq!(prologue(&[0x48, 0x8d, 0x05, 0, 0, 0, 0, 0]), None);
    }

    #[test]
    fn stub_shape() {
        let (at, mbox, func) = (0x7000_1000u64, 0x7100_0000u64, 0x7000_8000u64);
        let b = stub(at, mbox, &[0x40, 0x56, 0x48, 0x83, 0xec, 0x20], func + 6);

        let n = b.len();
        assert_eq!(&b[n - 11..n - 5], &[0x40, 0x56, 0x48, 0x83, 0xec, 0x20]);
        let rel = i32::from_le_bytes(b[n - 4..].try_into().unwrap()) as i64;
        assert_eq!(at as i64 + n as i64 + rel, (func + 6) as i64);

        assert!(n as u64 <= STUB_MAX);
    }
}
