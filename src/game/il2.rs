use crate::os::mem::{self, Mem};
use std::collections::HashMap;

pub const K_NAME: u64 = 0x10;
pub const K_NS: u64 = 0x18;
pub const K_TYPE: u64 = 0x20;
pub const K_PARENT: u64 = 0x58;
pub const K_SELF: u64 = 0x78;
pub const K_FIELDS: u64 = 0x80;
pub const K_STATICS: u64 = 0xb8;
pub const K_INSTANCE_SIZE: u64 = 0xf8;
pub const TE_R4: u8 = 0x0c;

const CHUNK: usize = 16 << 20;

pub struct Il2 {
    classes: HashMap<String, u64>,
}

impl Il2 {
    pub fn scan(m: &Mem, want: &[&str]) -> Il2 {
        let mut classes = HashMap::new();
        let mut buf = vec![0u8; CHUNK + 0x80];
        'regions: for r in mem::maps(m.pid) {
            if !r.r || !(r.path.is_empty() || r.path.starts_with("[anon")) || r.hi - r.lo > 1 << 30 {
                continue;
            }
            let mut at = r.lo;
            while at < r.hi {
                let n = ((r.hi - at) as usize).min(CHUNK + 0x80);
                if !m.read(at, &mut buf[..n]) {
                    break;
                }
                let words = n / 8;
                for i in 0..words.saturating_sub(16) {
                    let w = |k: usize| u64::from_le_bytes(buf[(i + k) * 8..(i + k) * 8 + 8].try_into().unwrap());
                    let a = at + i as u64 * 8;
                    let (name, ns) = (w(K_NAME as usize / 8), w(K_NS as usize / 8));
                    if w(K_SELF as usize / 8) != a || !mem::ok(name) || !mem::ok(ns) {
                        continue;
                    }
                    let (Some(name), Some(ns)) = (m.cstr(name, 96), m.cstr(ns, 96)) else { continue };
                    let fqn = if ns.is_empty() { name } else { format!("{ns}.{name}") };
                    if want.contains(&fqn.as_str()) {
                        classes.entry(fqn).or_insert(a);
                        if classes.len() == want.len() {
                            break 'regions;
                        }
                    }
                }
                at += CHUNK as u64;
            }
        }
        Il2 { classes }
    }

    pub fn class(&self, n: &str) -> Option<u64> {
        self.classes.get(n).copied()
    }
}

pub struct Field {
    pub name: String,
    pub off: u64,
    pub stat: bool,
    pub ty: u64,
    pub te: u8,
}

pub fn fields(m: &Mem, k: u64) -> Vec<Field> {
    let mut out = vec![];
    let f = m.q(k + K_FIELDS);
    if !mem::ok(f) {
        return out;
    }
    for i in 0..4096u64 {
        let Some(e) = m.bytes(f + i * 0x20, 0x20) else { break };
        let g = |o: usize| u64::from_le_bytes(e[o..o + 8].try_into().unwrap());
        if g(0x10) != k {
            break;
        }
        let bits = m.u32(g(0x08) + 8);
        let (ty, attrs, te) = (m.q(g(0x08)), bits & 0xffff, (bits >> 16) as u8);
        if attrs & 0x40 != 0 {
            continue;
        }
        let Some(name) = m.cstr(g(0), 96) else { continue };
        let off = i32::from_le_bytes(e[0x18..0x1c].try_into().unwrap());
        if off < 0 {
            continue;
        }
        out.push(Field { name, off: off as u64, stat: attrs & 0x10 != 0, ty, te });
    }
    out
}

pub fn field(m: &Mem, mut k: u64, name: &str) -> Option<u64> {
    for _ in 0..16 {
        if !mem::ok(k) {
            break;
        }
        if let Some(f) = fields(m, k).into_iter().find(|f| !f.stat && f.name == name) {
            return Some(f.off);
        }
        k = m.q(k + K_PARENT);
    }
    None
}

pub fn static_field(m: &Mem, k: u64, name: &str) -> Option<u64> {
    fields(m, k).into_iter().find(|f| f.stat && f.name == name).map(|f| f.off)
}

pub fn class_name(m: &Mem, k: u64) -> Option<String> {
    m.cstr(m.q(k + K_NAME), 96)
}

pub const K_METHODS: u64 = 0x98;
pub const K_METHOD_COUNT: u64 = 0x120;
pub const K_FIELD_COUNT: u64 = 0x124;
pub const TE_BOOL: u8 = 0x02;
pub const TE_I4: u8 = 0x08;
pub const TE_STRING: u8 = 0x0e;
pub const TE_VALUETYPE: u8 = 0x11;
pub const TE_CLASS: u8 = 0x12;
pub const TE_SZARRAY: u8 = 0x1d;
pub const TE_OBJECT: u8 = 0x1c;

pub struct Method {
    pub info: u64,
    pub ptr: u64,
    pub name: String,
    pub stat: bool,

    pub params: Vec<(u8, u64)>,
}

pub fn methods(m: &Mem, k: u64) -> Vec<Method> {
    let (arr, n) = (m.q(k + K_METHODS), m.rd::<u16>(k + K_METHOD_COUNT).unwrap_or(0));
    if !mem::ok(arr) {
        return vec![];
    }
    (0..n as u64)
        .filter_map(|i| {
            let mi = m.q(arr + i * 8);
            let name = m.cstr(m.q(mi + 0x18), 128)?;
            let pc = m.u8(mi + 0x52) as u64;
            let ps = m.q(mi + 0x30);
            let params = (0..pc).map(|j| m.q(ps + j * 8)).map(|t| (m.u8(t + 0xa), m.q(t))).collect();
            let stat = m.rd::<u16>(mi + 0x4c).unwrap_or(0) & 0x10 != 0;
            Some(Method { info: mi, ptr: m.q(mi), name, stat, params })
        })
        .collect()
}

pub fn method(m: &Mem, k: u64, name: &str, sig: &[(u8, u64)]) -> Option<Method> {
    methods(m, k).into_iter().find(|f| f.name == name && f.ptr != 0 && f.params.len() == sig.len() && f.params.iter().zip(sig).all(|(p, s)| p.0 == s.0 && (s.1 == 0 || p.1 == s.1)))
}

pub fn methods_report(args: &[String]) {
    let Some(m) = mem::find_pid().and_then(|p| Mem::open(p).ok()) else { return eprintln!("GoreBox not running") };
    let specs: Vec<(&str, Vec<&str>)> = args
        .iter()
        .map(|a| match a.split_once(':') {
            Some((c, n)) => (c, n.split(',').collect()),
            None => (a.as_str(), vec![]),
        })
        .collect();
    let want: Vec<&str> = specs.iter().map(|s| s.0).collect();
    let il = Il2::scan(&m, &want);
    for (c, names) in &specs {
        let Some(k) = il.class(c) else {
            println!("{c}: not found");
            continue;
        };
        let (mc, fc) = (m.rd::<u16>(k + K_METHOD_COUNT).unwrap_or(0), m.rd::<u16>(k + K_FIELD_COUNT).unwrap_or(0));
        println!("{c} @ {k:#x}  methods {mc} @ {:#x}  field_count {fc} (fields() sees {})  type handle {:#x}", m.q(k + K_METHODS), fields(&m, k).len(), m.q(k + K_TYPE));
        for f in methods(&m, k).into_iter().filter(|f| names.is_empty() || names.contains(&f.name.as_str())) {
            let ps: Vec<String> = f.params.iter().map(|p| format!("{:#x}:{:#x}", p.0, p.1)).collect();
            println!("  {}{} ptr {:#x} info {:#x} ({})", if f.stat { "static " } else { "" }, f.name, f.ptr, f.info, ps.join(", "));
        }
    }
}

pub fn classes_report(args: &[String]) {
    let Some(m) = mem::find_pid().and_then(|p| Mem::open(p).ok()) else { return eprintln!("GoreBox not running") };
    let mut seen = std::collections::BTreeMap::new();
    let mut buf = vec![0u8; CHUNK + 0x80];
    for r in mem::maps(m.pid) {
        if !r.r || !(r.path.is_empty() || r.path.starts_with("[anon")) || r.hi - r.lo > 1 << 30 {
            continue;
        }
        let mut at = r.lo;
        while at < r.hi {
            let n = ((r.hi - at) as usize).min(CHUNK + 0x80);
            if !m.read(at, &mut buf[..n]) {
                break;
            }
            for i in 0..(n / 8).saturating_sub(16) {
                let w = |k: usize| u64::from_le_bytes(buf[(i + k) * 8..(i + k) * 8 + 8].try_into().unwrap());
                let a = at + i as u64 * 8;
                if w(K_SELF as usize / 8) != a || !mem::ok(w(2)) || !mem::ok(w(3)) {
                    continue;
                }
                let (Some(name), Some(ns)) = (m.cstr(w(2), 96), m.cstr(w(3), 96)) else { continue };
                let fqn = if ns.is_empty() { name } else { format!("{ns}.{name}") };
                if args.iter().any(|t| fqn.to_lowercase().contains(&t.to_lowercase())) {
                    seen.entry(fqn).or_insert(a);
                }
            }
            at += CHUNK as u64;
        }
    }
    for (n, k) in seen {
        println!("{k:#x} {n}");
    }
}

pub fn fields_report(args: &[String]) {
    let Some(m) = mem::find_pid().and_then(|p| Mem::open(p).ok()) else { return eprintln!("GoreBox not running") };
    let want: Vec<&str> = args.iter().map(|a| a.as_str()).collect();
    let il = Il2::scan(&m, &want);
    for c in want {
        let Some(k) = il.class(c) else {
            println!("{c}: not found");
            continue;
        };
        let parent = class_name(&m, m.q(k + K_PARENT)).unwrap_or_default();
        println!("{c} @ {k:#x} : {parent}  size {:#x}", m.u32(k + K_INSTANCE_SIZE));
        for f in fields(&m, k) {
            println!("  {}{:#06x} te {:#04x} {:#x} {}", if f.stat { "static " } else { "" }, f.off, f.te, f.ty, f.name);
        }
    }
}

pub fn field_search(args: &[String]) {
    let Some(m) = mem::find_pid().and_then(|p| Mem::open(p).ok()) else { return eprintln!("GoreBox not running") };
    let mut seen = std::collections::BTreeMap::new();
    let mut buf = vec![0u8; CHUNK + 0x80];
    for r in mem::maps(m.pid) {
        if !r.r || !(r.path.is_empty() || r.path.starts_with("[anon")) || r.hi - r.lo > 1 << 30 {
            continue;
        }
        let mut at = r.lo;
        while at < r.hi {
            let n = ((r.hi - at) as usize).min(CHUNK + 0x80);
            if !m.read(at, &mut buf[..n]) {
                break;
            }
            for i in 0..(n / 8).saturating_sub(16) {
                let w = |k: usize| u64::from_le_bytes(buf[(i + k) * 8..(i + k) * 8 + 8].try_into().unwrap());
                let a = at + i as u64 * 8;
                if w(K_SELF as usize / 8) != a || !mem::ok(w(2)) || !mem::ok(w(3)) || seen.values().any(|&(k, _)| k == a) {
                    continue;
                }
                let (Some(name), Some(ns)) = (m.cstr(w(2), 96), m.cstr(w(3), 96)) else { continue };
                let hits: Vec<String> = fields(&m, a)
                    .into_iter()
                    .filter(|f| args.iter().any(|t| f.name.to_lowercase().contains(&t.to_lowercase())))
                    .map(|f| format!("{}{}@{:#x}", if f.stat { "static " } else { "" }, f.name, f.off))
                    .collect();
                if !hits.is_empty() {
                    let fqn = if ns.is_empty() { name } else { format!("{ns}.{name}") };
                    seen.insert(fqn, (a, hits.join(" ")));
                }
            }
            at += CHUNK as u64;
        }
    }
    for (n, (k, h)) in seen {
        println!("{k:#x} {n}: {h}");
    }
}

pub fn class_with_fields(m: &Mem, names: &[&str]) -> Option<u64> {
    let mut buf = vec![0u8; CHUNK + 0x80];
    for r in mem::maps(m.pid) {
        if !r.r || !(r.path.is_empty() || r.path.starts_with("[anon")) || r.hi - r.lo > 1 << 30 {
            continue;
        }
        let mut at = r.lo;
        while at < r.hi {
            let n = ((r.hi - at) as usize).min(CHUNK + 0x80);
            if !m.read(at, &mut buf[..n]) {
                break;
            }
            for i in 0..(n / 8).saturating_sub(16) {
                let w = |k: usize| u64::from_le_bytes(buf[(i + k) * 8..(i + k) * 8 + 8].try_into().unwrap());
                let a = at + i as u64 * 8;
                if w(K_SELF as usize / 8) != a || !mem::ok(w(2)) || !mem::ok(w(K_FIELDS as usize / 8)) {
                    continue;
                }
                let fs = fields(m, a);
                if names.iter().all(|n| fs.iter().any(|f| f.name == *n)) {
                    return Some(a);
                }
            }
            at += CHUNK as u64;
        }
    }
    None
}
