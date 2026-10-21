use crate::features::aim::{Key, Method};
use std::collections::HashMap;
use std::path::PathBuf;

pub trait Val {
    fn put(&self) -> String;
    fn get(&mut self, s: &str) -> Option<()>;
}

impl Val for bool {
    fn put(&self) -> String {
        self.to_string()
    }
    fn get(&mut self, s: &str) -> Option<()> {
        *self = s.parse().ok()?;
        Some(())
    }
}

impl Val for f32 {
    fn put(&self) -> String {
        self.to_string()
    }
    fn get(&mut self, s: &str) -> Option<()> {
        *self = s.parse::<f32>().ok().filter(|v| v.is_finite())?;
        Some(())
    }
}

impl Val for usize {
    fn put(&self) -> String {
        self.to_string()
    }
    fn get(&mut self, s: &str) -> Option<()> {
        *self = s.parse().ok()?;
        Some(())
    }
}

impl<T: Val + Copy + Default, const N: usize> Val for [T; N] {
    fn put(&self) -> String {
        self.iter().map(Val::put).collect::<Vec<_>>().join(" ")
    }
    fn get(&mut self, s: &str) -> Option<()> {
        let parts: Vec<&str> = s.split_whitespace().collect();
        if parts.len() != N {
            return None;
        }
        let mut out = [T::default(); N];
        for (o, p) in out.iter_mut().zip(parts) {
            o.get(p)?;
        }
        *self = out;
        Some(())
    }
}

impl Val for u8 {
    fn put(&self) -> String {
        self.to_string()
    }
    fn get(&mut self, s: &str) -> Option<()> {
        *self = s.parse().ok()?;
        Some(())
    }
}

impl Val for Key {
    fn put(&self) -> String {
        self.label().into()
    }
    fn get(&mut self, s: &str) -> Option<()> {
        *self = *Key::ALL.iter().find(|k| k.label() == s)?;
        Some(())
    }
}

impl Val for crate::features::aa::Pitch {
    fn put(&self) -> String {
        self.label().into()
    }
    fn get(&mut self, s: &str) -> Option<()> {
        *self = *crate::features::aa::Pitch::ALL.iter().find(|m| m.label() == s)?;
        Some(())
    }
}

impl Val for crate::features::aa::Yaw {
    fn put(&self) -> String {
        self.label().into()
    }
    fn get(&mut self, s: &str) -> Option<()> {
        *self = *crate::features::aa::Yaw::ALL.iter().find(|m| m.label() == s)?;
        Some(())
    }
}

impl Val for Method {
    fn put(&self) -> String {
        self.label().into()
    }
    fn get(&mut self, s: &str) -> Option<()> {
        *self = *Method::ALL.iter().find(|m| m.label() == s)?;
        Some(())
    }
}

unsafe extern "C" {
    fn flock(fd: i32, op: i32) -> i32;
}

pub fn lock(name: &str) -> Option<std::fs::File> {
    use std::os::fd::AsRawFd;
    const LOCK_EX: i32 = 2;
    const LOCK_NB: i32 = 4;
    let _ = std::fs::create_dir_all(dir());
    let f = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir().join(name)).ok()?;
    (unsafe { flock(f.as_raw_fd(), LOCK_EX | LOCK_NB) } == 0).then_some(f)
}

pub fn dir() -> PathBuf {
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let Some(base) = var("XDG_CONFIG_HOME").or_else(|| var("HOME").map(|h| h.join(".config"))) else {
            return std::env::current_exe().ok().and_then(|e| e.parent().map(PathBuf::from)).unwrap_or_else(|| PathBuf::from("."));
        };
        let (dir, old) = (base.join("goretux"), base.join("gorebox"));
        if !dir.exists() && old.is_dir() {
            let _ = std::fs::rename(&old, &dir);
        }
        dir
    })
    .clone()
}

fn path() -> PathBuf {
    dir().join("settings.txt")
}

pub type Walk<'a> = &'a mut dyn FnMut(&str, &mut dyn Val);

pub fn dump(walk: impl FnOnce(Walk)) -> String {
    let mut out = String::new();
    walk(&mut |k, v| out += &format!("{k} = {}\n", v.put()));
    out
}

pub fn parse(text: &str, walk: impl FnOnce(Walk)) {
    let map: HashMap<&str, &str> = text.lines().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.trim(), v.trim())).collect();
    walk(&mut |k, v| {
        if let Some(s) = map.get(k)
            && v.get(s).is_none()
        {
            eprintln!("settings: bad value for {k}: {s}");
        }
    });
}

pub fn load(walk: impl FnOnce(Walk)) {
    if let Ok(text) = std::fs::read_to_string(path()) {
        parse(&text, walk);
    }
}

pub fn save(text: &str) {
    let p = path();
    let tmp = p.with_extension("tmp");
    let _ = std::fs::create_dir_all(dir());
    if std::fs::write(&tmp, text).is_ok() {
        let _ = std::fs::rename(&tmp, &p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let (mut a, mut b, mut c, mut k) = (true, 2.5f32, [1.0f32, 0.5, 0.25], Key::Mmb);
        let mut hit = [100u8, 0, 30, 0, 5];
        let text = dump(|f| {
            f("a", &mut a);
            f("b", &mut b);
            f("c", &mut c);
            f("k", &mut k);
            f("hit", &mut hit);
        });
        let (mut a2, mut b2, mut c2, mut k2, mut hit2, mut d2) = (false, 0.0f32, [0.0f32; 3], Key::Rmb, [0u8; 5], 7.0f32);

        parse(&format!("{text}d = nope\nzzz = 1\n"), |f| {
            f("a", &mut a2);
            f("b", &mut b2);
            f("c", &mut c2);
            f("k", &mut k2);
            f("hit", &mut hit2);
            f("d", &mut d2);
        });
        assert!(a2 && b2 == 2.5 && c2 == c && k2 == Key::Mmb && hit2 == hit && d2 == 7.0);
    }
}
