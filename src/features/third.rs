use crate::game::il2::{self, Il2, Method};
use crate::game::remote::{Call, Remote};
use crate::game::sdk::nat;
use crate::game::undo;
use crate::os::mem::{self, Mem};
use std::time::{Duration, Instant};

const PERMS: &str = "GoreBox.ServerPermissions";

const PERM_FIELD: &str = "<ddda>k__BackingField";

const ENTER: &str = "bdum";
const LEAVE: &str = "bduo";

const K_STATICS: u64 = 0xb8;

struct Api {
    k: u64,
    enter: Method,
    leave: Method,
    flag: u64,
    controller: u64,
    perm: u64,
    perms_k: u64,
}

#[derive(Default)]
pub struct Third {
    api: Option<Api>,
    failed: Option<Instant>,
    obj: u64,
    looked: Option<Instant>,
    entered: Option<Instant>,
    perm_orig: Option<u8>,
    pub status: String,
}

fn sets(code: &[u8], off: u64, v: u8) -> bool {
    let mut pat = vec![0xc6, 0x83];
    pat.extend_from_slice(&(off as u32).to_le_bytes());
    pat.push(v);
    code.windows(pat.len()).any(|w| w == pat)
}

impl Api {
    fn resolve(m: &Mem) -> Result<Api, String> {
        let k = il2::class_with_fields(m, &["thirdPersonOffset", "thirdPersonRecoilScale", "SpectatorSuppressed"]).ok_or("no camera controller class")?;
        let recoil = il2::field(m, k, "thirdPersonRecoilScale").ok_or("no thirdPersonRecoilScale")?;
        let controller = il2::field(m, k, "controller").ok_or("no controller field")?;

        let ms = il2::methods(m, k);
        let flag = ms
            .iter()
            .filter_map(|f| m.bytes(f.ptr, 17))
            .find(|b| b[..2] == [0x80, 0xb9] && b[6] == 0 && b[7] == 0x74 && b[9..13] == [0xf3, 0x0f, 0x10, 0x81] && u32::from_le_bytes(b[13..17].try_into().unwrap()) as u64 == recoil)
            .map(|b| u32::from_le_bytes(b[2..6].try_into().unwrap()) as u64)
            .ok_or("no third-person flag")?;
        let enter = il2::method(m, k, ENTER, &[]).ok_or("no enter method")?;
        let leave = il2::method(m, k, LEAVE, &[]).ok_or("no leave method")?;

        if !m.bytes(enter.ptr, 0x80).is_some_and(|c| sets(&c, flag, 1)) || !m.bytes(leave.ptr, 0x80).is_some_and(|c| sets(&c, flag, 0)) {
            return Err("enter/leave methods changed in this game version".into());
        }
        let perms_k = Il2::scan(m, &[PERMS]).class(PERMS).ok_or("no ServerPermissions")?;
        let off = il2::fields(m, perms_k).into_iter().find(|f| f.stat && f.name == PERM_FIELD).ok_or("no third-person permission")?.off;
        let statics = m.q(perms_k + K_STATICS);
        if !mem::ok(statics) {
            return Err("ServerPermissions not set up yet".into());
        }
        Ok(Api { k, enter, leave, flag, controller, perm: statics + off, perms_k })
    }
}

impl Third {
    pub fn tick(&mut self, m: &Mem, r: &mut Remote, want: bool, local: u64) {
        if !want {
            self.off(m, r);
            self.status.clear();
            return;
        }
        if self.api.is_none() {
            if self.failed.is_some_and(|t| t.elapsed() < Duration::from_secs(10)) {
                return;
            }
            match Api::resolve(m) {
                Ok(a) => self.api = Some(a),
                Err(e) => {
                    self.status = format!("third person: {e}");
                    self.failed = Some(Instant::now());
                    return;
                }
            }
        }
        let a = self.api.as_ref().unwrap();

        if self.perm_orig.is_none() {
            let Some(old) = m.rd::<u8>(a.perm) else { return };
            undo::keep(m.fd(), a.perm, a.perms_k, m.q(a.perms_k), &[old]);
            self.perm_orig = Some(old);
        }
        if m.u8(a.perm) != 1 {
            m.write(a.perm, &[1]);
        }
        if local == 0 {
            self.status = "third person: not in game".into();
            return;
        }

        let ours = |o: u64| alive(m, a.k, o) && m.q(o + a.controller) == local;
        if !ours(self.obj) {
            self.obj = 0;
            if self.looked.is_none_or(|t| t.elapsed() > Duration::from_secs(2)) {
                self.looked = Some(Instant::now());
                self.obj = mem::objects(m, a.k).into_iter().find(|&o| ours(o)).unwrap_or(0);
            }
        }
        if self.obj == 0 {
            self.status = "third person: no camera controller for our player".into();
            return;
        }
        let on = m.u8(self.obj + a.flag) != 0;
        if !on && self.entered.is_none_or(|t| t.elapsed() > Duration::from_secs(1)) {
            self.entered = Some(Instant::now());
            if let Err(e) = r.call(m, Call::method(&a.enter, &[self.obj]).alive(self.obj)) {
                self.status = format!("third person: {e}");
                return;
            }
        }
        self.status = if on { "third person: on".into() } else { "third person: entering".into() };
    }

    pub fn active(&self) -> bool {
        self.perm_orig.is_some()
    }

    pub fn off(&mut self, m: &Mem, r: &mut Remote) {
        let Some(a) = self.api.as_ref() else { return };
        if self.obj != 0 && alive(m, a.k, self.obj) && m.u8(self.obj + a.flag) != 0 {
            let _ = r.call(m, Call::method(&a.leave, &[self.obj]).alive(self.obj));
        }
        if let Some(old) = self.perm_orig.take() {
            if m.q(a.perms_k) != 0 {
                m.write(a.perm, &[old]);
            }
            undo::forget(a.perm);
        }
        (self.obj, self.entered) = (0, None);
    }
}

fn alive(m: &Mem, k: u64, o: u64) -> bool {
    let native = m.q(o + nat::CACHED_PTR);
    k != 0 && mem::ok(o) && m.q(o) == k && mem::ok(native) && m.q(m.q(native + nat::NAT_MANAGED)) == o
}

pub fn report() {
    let Some(m) = mem::find_pid().and_then(|p| Mem::open(p).ok()) else { return eprintln!("GoreBox not running") };
    match Api::resolve(&m) {
        Ok(a) => {
            let objs: Vec<u64> = mem::objects(&m, a.k).into_iter().filter(|&o| alive(&m, a.k, o)).collect();
            println!(
                "controller class {:#x}, flag +{:#x}, controller +{:#x}, enter {:#x} leave {:#x}, permission {:#x} = {}",
                a.k,
                a.flag,
                a.controller,
                a.enter.ptr,
                a.leave.ptr,
                a.perm,
                m.u8(a.perm)
            );
            for o in objs {
                println!("  camera controller {o:#x}: third person {} player {:#x}", m.u8(o + a.flag), m.q(o + a.controller));
            }
        }
        Err(e) => println!("third person: {e}"),
    }
}
