use x11rb::connection::Connection;
use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::xproto::*;
use x11rb::rust_connection::RustConnection;

const XK_INSERT: u32 = 0xff63;
const XK_END: u32 = 0xff57;
const XK_HOME: u32 = 0xff50;

pub struct X {
    c: RustConnection,
    root: Window,
    clients: Atom,
    wm_pid: Atom,
    active: Atom,
    lo: u8,
    per: usize,
    syms: Vec<u32>,
    pub kc_menu: u8,
    pub kc_quit: u8,
    pub kc_stream: u8,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl X {
    pub fn new() -> Option<X> {
        let (c, screen) = x11rb::connect(None).ok()?;
        let root = c.setup().roots[screen].root;
        let atom = |n: &str| c.intern_atom(false, n.as_bytes()).ok()?.reply().ok().map(|r| r.atom);
        let clients = atom("_NET_CLIENT_LIST")?;
        let wm_pid = atom("_NET_WM_PID")?;
        let active = atom("_NET_ACTIVE_WINDOW")?;
        let (lo, hi) = (c.setup().min_keycode, c.setup().max_keycode);
        let map = c.get_keyboard_mapping(lo, hi - lo + 1).ok()?.reply().ok()?;
        let mut x = X { c, root, clients, wm_pid, active, lo, per: map.keysyms_per_keycode as usize, syms: map.keysyms, kc_menu: 0, kc_quit: 0, kc_stream: 0 };
        (x.kc_menu, x.kc_quit, x.kc_stream) = (x.kc(XK_INSERT), x.kc(XK_END), x.kc(XK_HOME));
        Some(x)
    }

    pub fn kc(&self, sym: u32) -> u8 {
        self.syms.chunks(self.per).position(|k| k.contains(&sym)).map(|i| self.lo + i as u8).unwrap_or(0)
    }

    pub fn buttons(&self) -> u16 {
        self.pointer().map_or(0, |p| p.2)
    }

    pub fn pointer(&self) -> Option<(i32, i32, u16)> {
        let r = self.c.query_pointer(self.root).ok()?.reply().ok()?;
        Some((r.root_x as i32, r.root_y as i32, u16::from(r.mask)))
    }

    pub fn active(&self) -> Option<Window> {
        self.card(self.root, self.active).first().copied()
    }

    fn card(&self, w: Window, a: Atom) -> Vec<u32> {
        self.c.get_property(false, w, a, AtomEnum::ANY, 0, 1024).ok().and_then(|r| r.reply().ok()).and_then(|r| r.value32().map(|v| v.collect())).unwrap_or_default()
    }

    pub fn game_win(&self, pid: i32) -> Option<Window> {
        self.card(self.root, self.clients).into_iter().find(|&w| self.card(w, self.wm_pid).first() == Some(&(pid as u32)))
    }

    pub fn game(&self, pid: i32) -> Option<Rect> {
        let w = self.game_win(pid)?;
        let g = self.c.get_geometry(w).ok()?.reply().ok()?;
        let t = self.c.translate_coordinates(w, self.root, 0, 0).ok()?.reply().ok()?;
        Some(Rect { x: t.dst_x as i32, y: t.dst_y as i32, w: g.width as u32, h: g.height as u32 })
    }

    pub fn depth(&self, win: u32) -> Option<u8> {
        self.c.get_geometry(win).ok()?.reply().ok().map(|g| g.depth)
    }

    pub fn keys(&self) -> [u8; 32] {
        self.c.query_keymap().ok().and_then(|r| r.reply().ok()).map(|r| r.keys).unwrap_or([0; 32])
    }

    pub fn down(keys: &[u8; 32], kc: u8) -> bool {
        kc != 0 && keys[kc as usize / 8] & (1 << (kc % 8)) != 0
    }

    pub fn input(&self, win: u32, r: Option<Rect>) {
        let rects: Vec<Rectangle> = r.map(|r| Rectangle { x: r.x as i16, y: r.y as i16, width: r.w as u16, height: r.h as u16 }).into_iter().collect();
        let _ = self.c.shape_rectangles(shape::SO::SET, shape::SK::INPUT, ClipOrdering::UNSORTED, win, 0, 0, &rects);
        let _ = self.c.flush();
    }

    pub fn place(&self, win: u32, r: Rect) {
        let aux = ConfigureWindowAux::new().x(r.x).y(r.y).width(r.w).height(r.h).stack_mode(StackMode::ABOVE);
        let _ = self.c.configure_window(win, &aux);
        let _ = self.c.flush();
    }

    pub fn raise(&self, win: u32) {
        let _ = self.c.configure_window(win, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE));
        let _ = self.c.flush();
    }
}
