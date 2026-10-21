use crate::game::il2::{self, Il2};
use crate::os::mem::{self, Mem};

pub const WANT: &[&str] = &[
    "System.String",
    "GoreBox.PlayerController",
    "GoreBox.PlayerRegistry",
    "GoreBox.Networking.PlayerNetSync",
    "FishNet.InstanceFinder",
    "FishNet.Managing.NetworkManager",
    "FishNet.Managing.Timing.TimeManager",
    "FishNet.Managing.Client.ClientManager",
    "FishNet.Managing.Object.ManagedObjects",
    "FishNet.Object.NetworkObject",
    "FishNet.Connection.NetworkConnection",
    "UnityEngine.Quaternion",
    "UnityEngine.Camera",
    "PlayerCard",
];

pub mod nat {
    pub const CACHED_PTR: u64 = 0x10;
    pub const COMP_GO: u64 = 0x20;
    pub const COMP_ACTIVE: u64 = 0x29;
    pub const NAT_MANAGED: u64 = 0x18;
    pub const GO_COMPS: u64 = 0x20;
    pub const GO_NCOMPS: u64 = 0x30;
    pub const GO_NAME: u64 = 0x50;
    pub const TF_HIER: u64 = 0x28;
    pub const TF_INDEX: u64 = 0x30;
    pub const H_CAP: u64 = 0x10;
    pub const H_LOCAL: u64 = 0x18;
    pub const H_PARENT: u64 = 0x20;
    pub const H_TFS: u64 = 0x30;
    pub const CAM_VIEW: u64 = 0x70;
    pub const CAM_PROJ: u64 = 0xb0;
    pub const RB_ACTOR: u64 = 0x50;
    pub const RB_SCENE: u64 = 0x58;
}

pub mod col {
    pub const LIST_ITEMS: u64 = 0x10;
    pub const LIST_SIZE: u64 = 0x18;
    pub const ARR_LEN: u64 = 0x18;
    pub const ARR_DATA: u64 = 0x20;
    pub const DICT_ENTRIES: u64 = 0x18;
    pub const DICT_COUNT: u64 = 0x20;
    pub const ENTRY: u64 = 0x18;
}

#[derive(Clone, Copy, Default)]
pub struct Look {
    pub sens: u64,
    pub yaw: u64,
    pub pitch: u64,
}

pub const HIT_WANT: &[&str] = &["GoreBox.HitMarker", "GoreBox.HitFeedbackKind", "GoreBox.GoreDoll.LimbId"];

#[derive(Clone, Copy)]
pub struct HitDefs {
    pub k_hm: u64,
    pub time: u64,
    pub kind: u64,
    pub limb_ty: u64,
}

impl HitDefs {
    pub fn resolve(m: &Mem, il: &Il2) -> Option<HitDefs> {
        let k_hm = il.class("GoreBox.HitMarker")?;
        let kind_ty = m.q(il.class("GoreBox.HitFeedbackKind")? + il2::K_TYPE);
        let limb_ty = m.q(il.class("GoreBox.GoreDoll.LimbId")? + il2::K_TYPE);
        let fs: Vec<_> = il2::fields(m, k_hm).into_iter().filter(|f| f.stat).collect();
        let time = fs.iter().find(|f| f.te == il2::TE_R4)?.off;
        let kind = fs.iter().find(|f| f.ty == kind_ty)?.off;
        Some(HitDefs { k_hm, time, kind, limb_ty })
    }
}

pub struct Sdk {
    pub k_str: u64,
    pub k_pc: u64,
    pub k_reg: u64,
    pub k_pns: u64,
    pub k_if: u64,

    pub pc_head: u64,
    pub pc_ai: u64,
    pub pc_item: u64,
    pub pc_rb: u64,
    pub pc_look: u64,
    pub quat_ty: u64,
    pub k_cam: u64,
    pub pns_ctrl: u64,
    pub no_owner: u64,
    pub no_behaviours: u64,
    pub conn_cid: u64,
    pub if_nm: u64,
    pub nm_client: u64,

    pub nm_time: Option<u64>,
    pub tm_rtt: Option<u64>,
    pub tm_tick: Option<u64>,
    pub cm_conn: u64,
    pub cm_objects: u64,
    pub mo_spawned: u64,

    pub reg_statics: Vec<u64>,
    pub reg_fields: Vec<u64>,
    pub pc_fields: Vec<u64>,

    pub reg_teams: Option<u64>,
    pub reg_cards: Option<u64>,
    pub card_role: Option<u64>,

    pub pns_fire_origin: Option<u64>,
    pub reg_host: Option<u64>,
}

#[derive(Default)]
pub struct Sides {
    pub teams: std::collections::HashMap<i32, i32>,
    pub roles: std::collections::HashMap<i32, u8>,
}

impl Sides {
    pub fn mates(&self, a: i32, b: i32) -> bool {
        if !self.teams.is_empty() {
            return matches!((self.teams.get(&a), self.teams.get(&b)), (Some(x), Some(y)) if x == y);
        }
        matches!((self.roles.get(&a), self.roles.get(&b)), (Some(x), Some(y)) if x == y && *x != 0)
    }
}

impl Sdk {
    pub fn resolve(m: &Mem, il: &Il2) -> Result<Sdk, String> {
        let k = |n: &str| il.class(n).ok_or(format!("class {n}"));
        let f = |kl: u64, n: &str| il2::field(m, kl, n).ok_or(format!("field {n}"));

        let (k_pc, k_pns, k_reg, k_if) = (k("GoreBox.PlayerController")?, k("GoreBox.Networking.PlayerNetSync")?, k("GoreBox.PlayerRegistry")?, k("FishNet.InstanceFinder")?);
        let k_nm = k("FishNet.Managing.NetworkManager")?;
        let k_cm = k("FishNet.Managing.Client.ClientManager")?;
        let k_mo = k("FishNet.Managing.Object.ManagedObjects")?;
        let k_no = k("FishNet.Object.NetworkObject")?;
        let k_conn = k("FishNet.Connection.NetworkConnection")?;

        let all = il2::fields(m, k_reg);
        let pc_look = f(k_pc, "mouseLook")?;
        Ok(Sdk {
            k_str: k("System.String")?,
            k_pc,
            k_reg,
            k_pns,
            k_if,
            pc_head: f(k_pc, "head")?,
            pc_ai: f(k_pc, "aiDriven")?,
            pc_item: f(k_pc, "itemSwitcher")?,
            pc_rb: f(k_pc, "rigidBody")?,
            pc_look,
            quat_ty: m.q(k("UnityEngine.Quaternion")? + il2::K_TYPE),
            k_cam: k("UnityEngine.Camera")?,
            pns_ctrl: f(k_pns, "controller")?,
            no_owner: f(k_no, "_owner")?,
            no_behaviours: f(k_no, "NetworkBehaviours")?,
            conn_cid: f(k_conn, "ClientId")?,
            if_nm: il2::static_field(m, k_if, "_networkManager").ok_or("static _networkManager")?,
            nm_client: f(k_nm, "<ClientManager>k__BackingField")?,
            nm_time: il2::field(m, k_nm, "<TimeManager>k__BackingField"),
            tm_rtt: il.class("FishNet.Managing.Timing.TimeManager").and_then(|k| il2::field(m, k, "<RoundTripTime>k__BackingField")),
            tm_tick: il.class("FishNet.Managing.Timing.TimeManager").and_then(|k| il2::field(m, k, "<TickDelta>k__BackingField")),
            cm_conn: f(k_cm, "Connection")?,
            cm_objects: f(k_cm, "<Objects>k__BackingField")?,
            mo_spawned: f(k_mo, "_spawned")?,

            reg_statics: all.iter().filter(|f| f.stat && f.off % 8 == 0).map(|f| f.off).collect(),
            reg_fields: all.iter().filter(|f| !f.stat && f.off % 8 == 0).map(|f| f.off).collect(),
            pc_fields: il2::fields(m, k_pc).iter().filter(|f| !f.stat && f.off % 8 == 0).map(|f| f.off).collect(),
            reg_teams: il2::field(m, k_reg, "_teams"),
            reg_cards: il2::field(m, k_reg, "_cards"),
            card_role: il.class("PlayerCard").and_then(|c| il2::field(m, c, "role")),
            pns_fire_origin: il2::field(m, k_pns, "maxFireOriginDistance"),
            reg_host: il2::field(m, k_reg, "_hostClientId"),
        })
    }

    pub fn look(&self, m: &Mem, ml: u64) -> Option<Look> {
        let fs = il2::fields(m, m.q(ml));
        let sens = fs.iter().find(|f| !f.stat && f.name == "Sensitivity")?.off;
        let mut q = fs.iter().filter(|f| !f.stat && f.ty == self.quat_ty).map(|f| f.off);
        Some(Look { sens, yaw: q.next()?, pitch: q.next()? })
    }

    pub fn is(&self, m: &Mem, obj: u64, k: u64) -> bool {
        mem::ok(obj) && m.q(obj) == k
    }

    pub fn statics(&self, m: &Mem, k: u64) -> u64 {
        m.q(k + il2::K_STATICS)
    }

    fn sync_dict(&self, m: &Mem, sync: u64) -> Vec<(i32, u64, u64)> {
        if !mem::ok(sync) {
            return vec![];
        }
        let is_dict = |p: u64| mem::ok(p) && il2::class_name(m, m.q(p)).as_deref() == Some("Dictionary`2");
        let d = il2::field(m, m.q(sync), "Collection").map(|o| m.q(sync + o)).filter(|&p| is_dict(p)).or_else(|| (0..0x100u64).step_by(8).map(|i| m.q(sync + i)).find(|&p| is_dict(p)));
        let Some(d) = d else { return vec![] };
        let ent = m.q(d + col::DICT_ENTRIES);
        let cnt = m.i32(d + col::DICT_COUNT);

        let stride = m.u32(m.q(ent) + 0x104) as u64;
        if !mem::ok(ent) || !(0..=4096).contains(&cnt) || !(12..=256).contains(&stride) {
            return vec![];
        }
        (0..cnt as u64).map(|i| ent + col::ARR_DATA + i * stride).filter(|&e| m.i32(e) >= 0 && m.i32(e + 4) >= -1).map(|e| (m.i32(e + 8), e, stride)).collect()
    }

    pub fn host(&self, m: &Mem) -> i32 {
        let reg = self.registry(m);
        let Some(o) = self.reg_host.filter(|_| mem::ok(reg)) else { return -1 };
        let sv = m.q(reg + o);
        il2::field(m, m.q(sv), "_value").map_or(-1, |v| m.i32(sv + v))
    }

    pub fn net(&self, m: &Mem, nm: u64) -> (f32, f32) {
        let tm = self.nm_time.map_or(0, |o| m.q(nm + o));
        if !mem::ok(tm) {
            return (0.0, 0.0);
        }
        let rtt = self.tm_rtt.and_then(|o| m.rd::<u64>(tm + o)).filter(|&v| v < 5000).unwrap_or(0) as f32;
        let tick = self.tm_tick.and_then(|o| m.rd::<u64>(tm + o)).map(f64::from_bits).filter(|v| (0.0..1.0).contains(v)).unwrap_or(0.0) as f32;
        (rtt, tick)
    }

    pub fn sides(&self, m: &Mem) -> Sides {
        let reg = self.registry(m);
        let mut out = Sides::default();
        if !mem::ok(reg) {
            return out;
        }

        if let Some(o) = self.reg_teams {
            out.teams = self.sync_dict(m, m.q(reg + o)).into_iter().filter(|e| e.2 <= 16).map(|(k, e, _)| (k, m.i32(e + 12))).collect();
        }

        if let (Some(o), Some(role)) = (self.reg_cards, self.card_role) {
            out.roles = self.sync_dict(m, m.q(reg + o)).into_iter().map(|(k, e, _)| (k, m.u8(e + 0x10 + role - 0x10))).collect();
        }
        out
    }

    pub fn registry(&self, m: &Mem) -> u64 {
        let base = self.statics(m, self.k_reg);
        if !mem::ok(base) {
            return 0;
        }
        self.reg_statics.iter().map(|o| m.q(base + o)).find(|&v| self.is(m, v, self.k_reg)).unwrap_or(0)
    }

    pub fn list(&self, m: &Mem, l: u64) -> Vec<u64> {
        if !mem::ok(l) {
            return vec![];
        }
        let items = m.q(l + col::LIST_ITEMS);
        let n = m.i32(l + col::LIST_SIZE);
        if !(1..=1024).contains(&n) {
            return vec![];
        }
        m.bytes(items + col::ARR_DATA, n as usize * 8).map(|b| b.chunks_exact(8).map(|c| u64::from_le_bytes(c.try_into().unwrap())).collect()).unwrap_or_default()
    }

    pub fn int_dict(&self, m: &Mem, d: u64) -> Option<Vec<(i32, u64)>> {
        let ent = m.q(d + col::DICT_ENTRIES);
        let cnt = m.i32(d + col::DICT_COUNT);
        let len = m.i32(ent + col::ARR_LEN);
        if !mem::ok(ent) || !(0..=4096).contains(&cnt) || !(cnt..=8192).contains(&len) {
            return None;
        }

        let b = m.bytes(ent + col::ARR_DATA, cnt as usize * col::ENTRY as usize)?;
        let mut out = Vec::with_capacity(cnt as usize);
        for e in b.chunks_exact(col::ENTRY as usize) {
            let hash = i32::from_le_bytes(e[0..4].try_into().unwrap());
            let next = i32::from_le_bytes(e[4..8].try_into().unwrap());
            if hash < 0 || next < -1 {
                continue;
            }
            let key = i32::from_le_bytes(e[8..12].try_into().unwrap());
            if hash != key & 0x7fff_ffff {
                return None;
            }
            out.push((key, u64::from_le_bytes(e[16..24].try_into().unwrap())));
        }
        Some(out)
    }

    pub fn names_at(&self, m: &Mem, reg: u64, off: u64) -> Option<Vec<(i32, String)>> {
        let d = m.q(reg + off);
        if !mem::ok(d) || il2::class_name(m, m.q(d)).as_deref() != Some("Dictionary`2") {
            return None;
        }
        let ents = self.int_dict(m, d)?;
        if ents.is_empty() {
            return None;
        }
        let mut out = vec![];
        for (key, v) in ents {
            if !self.is(m, v, self.k_str) {
                return None;
            }
            out.push((key, m.mstr(v)?));
        }
        Some(out)
    }
}

pub fn registry_report() {
    let Some(m) = mem::find_pid().and_then(|p| Mem::open(p).ok()) else { return eprintln!("GoreBox not running") };
    let sdk = match Sdk::resolve(&m, &Il2::scan(&m, WANT)) {
        Ok(s) => s,
        Err(e) => return eprintln!("not ready: {e}"),
    };
    let reg = sdk.registry(&m);
    println!("registry {reg:#x}");
    let k = m.q(reg);
    let base = sdk.statics(&m, sdk.k_reg);
    let fields: Vec<(String, u64)> =
        il2::fields(&m, k).into_iter().filter(|f| f.off % 8 == 0).map(|f| (format!("{}{}", if f.stat { "static " } else { "" }, f.name), if f.stat { base + f.off } else { reg + f.off })).collect();
    let dict_in = |o: u64| -> Option<u64> {
        let name = il2::class_name(&m, m.q(o))?;
        if name == "Dictionary`2" {
            return Some(o);
        }

        (0..0x100u64).step_by(8).map(|i| m.q(o + i)).find(|&p| mem::ok(p) && il2::class_name(&m, m.q(p)).as_deref() == Some("Dictionary`2"))
    };
    for (name, at) in fields {
        let o = m.q(at);
        if !mem::ok(o) {
            continue;
        }
        let cls = il2::class_name(&m, m.q(o)).unwrap_or_default();
        let Some(d) = dict_in(o) else { continue };
        let ent = m.q(d + col::DICT_ENTRIES);
        let cnt = m.i32(d + col::DICT_COUNT).clamp(0, 64) as u64;

        let stride = m.u32(m.q(ent) + 0x104).clamp(8, 256) as u64;
        println!("{name} ({cls}) {cnt} entries, entry {stride:#x} bytes");
        for i in 0..cnt {
            let e = m.bytes(ent + col::ARR_DATA + i * stride, stride as usize).unwrap_or_default();
            if e.len() < 12 || i32::from_le_bytes(e[0..4].try_into().unwrap()) < 0 {
                continue;
            }
            let key = i32::from_le_bytes(e[8..12].try_into().unwrap());
            let val = &e[12..];
            let v8 = if val.len() >= 12 { u64::from_le_bytes(e[16..24].try_into().unwrap()) } else { 0 };
            let s = if mem::ok(v8) { m.mstr(v8).map(|s| format!(" \"{s}\"")).unwrap_or_default() } else { String::new() };
            let hex: Vec<String> = val.iter().map(|b| format!("{b:02x}")).collect();

            let strs: Vec<String> = (8..e.len().saturating_sub(7))
                .step_by(8)
                .filter_map(|o| {
                    let p = u64::from_le_bytes(e[o..o + 8].try_into().unwrap());
                    (mem::ok(p) && il2::class_name(&m, m.q(p)).as_deref() == Some("String")).then(|| format!("+{o:#x}=\"{}\"", m.mstr(p).unwrap_or_default()))
                })
                .collect();
            println!("  {key}: {}{s} {}", hex.join(""), strs.join(" "));
        }
    }
}

pub fn sides_report() {
    let Some(m) = mem::find_pid().and_then(|p| Mem::open(p).ok()) else { return eprintln!("GoreBox not running") };
    let sdk = match Sdk::resolve(&m, &Il2::scan(&m, WANT)) {
        Ok(s) => s,
        Err(e) => return eprintln!("not ready: {e}"),
    };
    let nm = m.q(sdk.statics(&m, sdk.k_if) + sdk.if_nm);
    let me = m.q(m.q(nm + sdk.nm_client) + sdk.cm_conn);
    let my = m.i32(me + sdk.conn_cid);
    let s = sdk.sides(&m);
    println!("me {my}: team {:?} role {:?}  (teams {}, roles {})", s.teams.get(&my), s.roles.get(&my), s.teams.len(), s.roles.len());
    let mut ids: Vec<i32> = s.roles.keys().chain(s.teams.keys()).copied().collect();
    ids.sort();
    ids.dedup();
    for id in ids {
        println!("  {id}: team {:?} role {:?}{}", s.teams.get(&id), s.roles.get(&id), if s.mates(id, my) && id != my { "  <- teammate" } else { "" });
    }
}

#[cfg(test)]
mod tests {
    use super::Sides;

    #[test]
    fn mates_by_team_then_role() {
        let mut s = Sides::default();
        s.roles = [(1, 3), (2, 3), (3, 1), (4, 0), (5, 0)].into_iter().collect();

        assert!(s.mates(1, 2) && !s.mates(1, 3) && !s.mates(4, 5) && !s.mates(1, 9));

        s.teams = [(1, 0), (2, 1), (3, 0)].into_iter().collect();
        assert!(s.mates(1, 3) && !s.mates(1, 2) && !s.mates(1, 4));
    }
}

pub fn fire_limits_report() {
    let Some(m) = mem::find_pid().and_then(|p| Mem::open(p).ok()) else { return eprintln!("GoreBox not running") };
    let sdk = match Sdk::resolve(&m, &Il2::scan(&m, WANT)) {
        Ok(s) => s,
        Err(e) => return eprintln!("not ready: {e}"),
    };
    let nm = m.q(sdk.statics(&m, sdk.k_if) + sdk.if_nm);
    let cm = m.q(nm + sdk.nm_client);
    let me = m.q(cm + sdk.cm_conn);
    let spawned = sdk.int_dict(&m, m.q(m.q(cm + sdk.cm_objects) + sdk.mo_spawned)).unwrap_or_default();
    let f = |n: &str| il2::field(&m, sdk.k_pns, n);
    for (_, no) in spawned {
        if m.q(no + sdk.no_owner) != me {
            continue;
        }
        let Some(pns) = sdk.list(&m, m.q(no + sdk.no_behaviours)).into_iter().find(|&nb| sdk.is(&m, nb, sdk.k_pns)) else { continue };
        for n in ["maxFireOriginDistance", "maxRewindSeconds", "maxShockwaveOriginDistance", "maxBodyCheckDistance", "maxPropActionDistance", "spawnProtectionSeconds"] {
            println!("{n} = {}", f(n).and_then(|o| m.rd::<f32>(pns + o)).unwrap_or(f32::NAN));
        }
    }
    let reg = sdk.registry(&m);
    if let Some(o) = il2::field(&m, sdk.k_reg, "_hostClientId") {
        let sv = m.q(reg + o);
        println!("_hostClientId SyncVar {sv:#x} class {}", il2::class_name(&m, m.q(sv)).unwrap_or_default());
        for fl in il2::fields(&m, m.q(sv)).into_iter().filter(|f| !f.stat) {
            println!("  {:#06x} te {:#04x} {} = {}", fl.off, fl.te, fl.name, m.i32(sv + fl.off));
        }
    }
    println!("my clientId {}", m.i32(me + sdk.conn_cid));
    let (rtt, tick) = sdk.net(&m, nm);
    println!("round trip {rtt}ms, tick {tick:.4}s ({:.0}Hz) -> network lead {:.0}ms", if tick > 0.0 { 1.0 / tick } else { 0.0 }, (rtt / 2000.0 + 2.0 * tick) * 1000.0);
}

pub fn cam_probe() {
    let Some(m) = mem::find_pid().and_then(|p| Mem::open(p).ok()) else { return eprintln!("GoreBox not running") };
    let (local, _, cam) = match crate::game::local(&m) {
        Ok(v) => v,
        Err(e) => return eprintln!("{e}"),
    };
    let native = m.q(cam + nat::CACHED_PTR);
    let proj: [f32; 16] = m.rd(native + nat::CAM_PROJ).unwrap_or_default();
    let fov = 2.0 * (1.0 / proj[5]).atan().to_degrees();
    println!("aim camera {cam:#x} native {native:#x}: fov from projection {fov:.2}");
    let hits: Vec<u64> = (0..0x1000u64).step_by(4).filter(|&o| m.rd::<f32>(native + o).is_some_and(|v| (v - fov).abs() < 0.05)).collect();
    println!("  fov-valued floats at {:x?}", hits);
    if let Some(&o) = hits.first() {
        let old = m.rd::<f32>(native + o).unwrap();
        m.write(native + o, &(old + 20.0).to_le_bytes());
        std::thread::sleep(std::time::Duration::from_millis(300));
        let p2: [f32; 16] = m.rd(native + nat::CAM_PROJ).unwrap_or_default();
        println!("  wrote {} at +{o:#x}: 300ms later field {} projection fov {:.2}", old + 20.0, m.rd::<f32>(native + o).unwrap_or(0.0), 2.0 * (1.0 / p2[5]).atan().to_degrees());
        m.write(native + o, &old.to_le_bytes());
    }
    let il = Il2::scan(&m, &["ctw"]);
    let Some(k) = il.class("ctw") else { return println!("no ctw class") };
    for o in mem::objects(&m, k) {
        let nat = m.q(o + nat::CACHED_PTR);
        if !mem::ok(nat) || m.q(m.q(nat + nat::NAT_MANAGED)) != o {
            continue;
        }
        let f = |n: &str| il2::field(&m, k, n).unwrap_or(0);
        println!(
            "ctw {o:#x} controller {:#x}{} thirdPersonOffset {:?} b4 {} c0 {} cc {} d8 {} ec {}",
            m.q(o + f("controller")),
            if m.q(o + f("controller")) == local { " (us)" } else { "" },
            m.rd::<[f32; 3]>(o + f("thirdPersonOffset")).unwrap_or_default(),
            m.u8(o + 0xb4),
            m.u8(o + 0xc0),
            m.u8(o + 0xcc),
            m.u8(o + 0xd8),
            m.u8(o + 0xec)
        );
    }
}

pub fn melee_probe() {
    let Some(m) = mem::find_pid().and_then(|p| Mem::open(p).ok()) else { return eprintln!("GoreBox not running") };
    let (local, _, cam) = match crate::game::local(&m) {
        Ok(v) => v,
        Err(e) => return eprintln!("{e}"),
    };
    println!("local {local:#x} aim camera {cam:#x}");
    let il = Il2::scan(&m, &["cwe"]);
    let Some(k) = il.class("cwe") else { return println!("no cwe") };
    let f = |n: &str| il2::field(&m, k, n).unwrap_or(0);
    for o in mem::objects(&m, k) {
        let native = m.q(o + nat::CACHED_PTR);
        if !mem::ok(native) || m.q(m.q(native + nat::NAT_MANAGED)) != o {
            continue;
        }
        let go = m.q(native + nat::COMP_GO);
        let owner_cam = m.q(o + 0xb0);
        println!(
            "cwe {o:#x} id {:?} unarmed {} go '{}' live {} camera +0xb0 {owner_cam:#x}{} +0xc0 {:#x}{} source {:#x} ({})",
            m.mstr(m.q(o + f("meleeId"))),
            m.u8(o + f("builtInUnarmed")),
            m.cstr(m.q(go + nat::GO_NAME), 64).unwrap_or_default(),
            m.u8(native + nat::COMP_ACTIVE),
            if owner_cam == cam { " (our aim camera)" } else { "" },
            m.q(o + 0xc0),
            if m.q(o + 0xc0) == local { " (us)" } else { "" },
            m.q(o + f("sourceProvider")),
            il2::class_name(&m, m.q(m.q(o + f("sourceProvider")))).unwrap_or_default()
        );
        let arr = m.q(o + f("attacks"));
        let n = m.i32(arr + 0x18).clamp(0, 16) as u64;
        for i in 0..n.min(2) {
            let a = m.q(arr + 0x20 + i * 8);
            let ka = m.q(a);
            let fields: Vec<String> = il2::fields(&m, ka)
                .into_iter()
                .filter(|x| !x.stat)
                .map(|x| {
                    let v = match x.te {
                        0x0c => format!("{:.2}", m.rd::<f32>(a + x.off).unwrap_or(0.0)),
                        0x02 => m.u8(a + x.off).to_string(),
                        0x08 => m.i32(a + x.off).to_string(),
                        _ => "..".into(),
                    };
                    format!("{}={v}", x.name)
                })
                .collect();
            println!("  attack {i} ({}): {}", il2::class_name(&m, ka).unwrap_or_default(), fields.join(" "));
        }
    }
}
