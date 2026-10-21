use crate::features::aa;
use crate::features::aim::{self, Aim, Key, Method};
use crate::features::ambiance::{self, Ambiance};
use crate::features::chams::{self, Chams};
use crate::features::hits::{self, Hit};
use crate::features::mods::{self, Mods};
use crate::features::silent;
use crate::features::tracers::{self, Trail};
use crate::game::{CHEST, Ent, HEAD, LINKS, NECK, PELVIS, Shared, Snap, V3};
use crate::os::grab::Grab;
use crate::os::mem;
use crate::settings::{self, Walk};
use crate::sound;
use crate::ui::x11::{Rect, X};
use glutin::config::{ConfigTemplateBuilder, GlConfig};
use glutin::context::{ContextApi, ContextAttributesBuilder, NotCurrentGlContext, PossiblyCurrentContext};
use glutin::display::{GetGlDisplay, GlDisplay};
use glutin::surface::{GlSurface, Surface, SurfaceAttributesBuilder, SwapInterval, WindowSurface};
use imgui::{Condition, DrawListMut, MouseButton};
use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::platform::x11::WindowAttributesExtX11;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{Window, WindowAttributes, WindowId};

type Col = [f32; 4];

const WHITE: Col = [1.0, 1.0, 1.0, 1.0];
const BLACK: Col = [0.0, 0.0, 0.0, 1.0];
const RED: Col = [1.0, 0.25, 0.25, 1.0];

struct Cfg {
    boxes: bool,
    skeleton: bool,
    head: bool,
    names: bool,
    dist: bool,
    snaplines: bool,
    npcs: bool,
    dead: bool,
    max_dist: f32,
    delay_ms: f32,
    player: [f32; 3],
    npc: [f32; 3],
    vis: bool,
    seen: [f32; 3],

    teammates: bool,
    team_col: [f32; 3],
    status: bool,

    stream: bool,

    third: bool,
    hitmarker: bool,
    hitsound: bool,
    hitsound_vol: f32,
    hitlog: bool,
    trace_own: bool,
    trace_others: bool,
    trace_col: [f32; 3],
    arrows: bool,
    arrow_radius: f32,
    arrow_size: f32,
    cursor_speed: f32,
    aim: aim::Cfg,
    mods: mods::Cfg,
    chams: chams::Cfg,
    world: ambiance::Cfg,
}

impl Default for Cfg {
    fn default() -> Cfg {
        Cfg {
            boxes: true,
            skeleton: true,
            head: false,
            names: true,
            dist: true,
            snaplines: false,
            npcs: true,
            dead: false,
            max_dist: 300.0,
            delay_ms: 0.0,
            player: [1.0, 0.27, 0.27],
            npc: [1.0, 0.78, 0.24],
            vis: true,
            seen: [0.3, 1.0, 0.47],
            teammates: true,
            team_col: [0.3, 0.6, 1.0],
            status: false,
            stream: false,
            third: false,
            hitmarker: true,
            hitsound: true,
            hitsound_vol: 0.6,
            hitlog: true,
            trace_own: true,
            trace_others: false,
            trace_col: [1.0, 0.85, 0.2],
            arrows: true,
            arrow_radius: 220.0,
            arrow_size: 12.0,
            cursor_speed: 1.0,
            aim: aim::Cfg::default(),
            mods: mods::Cfg::default(),
            chams: chams::Cfg::default(),
            world: ambiance::Cfg::default(),
        }
    }
}

impl Cfg {
    fn walk(&mut self, f: Walk) {
        f("esp.box", &mut self.boxes);
        f("esp.skeleton", &mut self.skeleton);
        f("esp.head_circle", &mut self.head);
        f("esp.names", &mut self.names);
        f("esp.distance", &mut self.dist);
        f("esp.snaplines", &mut self.snaplines);
        f("esp.npcs", &mut self.npcs);
        f("esp.dead", &mut self.dead);
        f("esp.range", &mut self.max_dist);
        f("esp.delay_ms", &mut self.delay_ms);
        f("esp.vis_check", &mut self.vis);
        f("esp.color.player", &mut self.player);
        f("esp.color.npc", &mut self.npc);
        f("esp.color.visible", &mut self.seen);
        f("esp.teammates", &mut self.teammates);
        f("esp.color.team", &mut self.team_col);
        f("esp.arrows", &mut self.arrows);
        f("esp.arrow_radius", &mut self.arrow_radius);
        f("esp.arrow_size", &mut self.arrow_size);
        f("esp.hitmarker", &mut self.hitmarker);
        f("esp.hitsound", &mut self.hitsound);
        f("esp.hitsound_volume", &mut self.hitsound_vol);
        f("esp.hitlog", &mut self.hitlog);
        f("esp.tracers.mine", &mut self.trace_own);
        f("esp.tracers.others", &mut self.trace_others);
        f("esp.tracers.color", &mut self.trace_col);
        f("chams.on", &mut self.chams.on);
        f("chams.npcs", &mut self.chams.npcs);
        f("chams.dead", &mut self.chams.dead);
        f("chams.color.visible", &mut self.chams.vis);
        f("chams.color.hidden", &mut self.chams.hid);
        f("chams.weapon", &mut self.chams.weapon);
        f("chams.local", &mut self.chams.local);
        f("chams.glow.color", &mut self.chams.glow_col);
        f("chams.glow.strength", &mut self.chams.glow);
        f("chams.glow.rainbow", &mut self.chams.rainbow);
        f("chams.glow.speed", &mut self.chams.speed);
        let a = &mut self.aim;
        f("aim.on", &mut a.on);
        f("aim.silent", &mut a.silent);
        f("aim.team_check", &mut a.team_check);
        f("aim.key", &mut a.key);
        f("aim.bone", &mut a.bone);
        f("aim.fov", &mut a.fov);
        f("aim.smooth", &mut a.smooth);
        f("aim.range", &mut a.max_dist);
        f("aim.players", &mut a.players);
        f("aim.npcs", &mut a.npcs);
        f("aim.visible_only", &mut a.vis_only);
        f("aim.draw_fov", &mut a.draw_fov);
        f("aim.silent_hit_chance", &mut a.hit);
        f("aim.silent_method", &mut a.method);
        f("aim.magic_fov", &mut a.magic_fov);
        f("aim.head_when_visible", &mut a.head_vis);
        f("aim.ragebot", &mut a.rage);
        f("aim.aa_pitch", &mut a.aa.pitch);
        f("aim.aa_yaw", &mut a.aa.yaw);
        f("aim.aa_spin_speed", &mut a.aa.spin);
        f("aim.prediction", &mut a.predict);
        f("aim.melee_aura", &mut a.aura);
        f("aim.aura_range", &mut a.aura_range);
        f("aim.auto_punch", &mut a.auto_punch);
        let w = &mut self.mods;
        f("weapon.no_recoil", &mut w.no_recoil);
        f("weapon.no_spread", &mut w.no_spread);
        f("weapon.no_sway", &mut w.no_sway);
        f("weapon.zoom", &mut w.zoom);
        f("weapon.zoom_mag", &mut w.zoom_mag);
        let w = &mut self.world;
        f("world.time", &mut w.time);
        f("world.hour", &mut w.hour);
        f("world.tint", &mut w.tint);
        f("world.tint.sky", &mut w.sky);
        f("world.tint.ground", &mut w.ground);
        f("world.tint.brightness", &mut w.bright);
        f("world.fog", &mut w.fog);
        f("world.fog.scale", &mut w.fog_scale);
        f("world.weather", &mut w.weather);
        f("world.weather.clouds", &mut w.clouds);
        f("world.weather.storm", &mut w.storm);
        f("world.weather.rain", &mut w.rain);
        f("world.grade", &mut w.grade);
        f("world.grade.filter", &mut w.filter);
        f("world.grade.hue", &mut w.hue);
        f("world.grade.saturation", &mut w.sat);
        f("world.grade.contrast", &mut w.contrast);
        f("world.grade.exposure", &mut w.exposure);
        f("world.glow", &mut w.glow);
        f("world.glow.intensity", &mut w.glow_int);
        f("world.glow.threshold", &mut w.glow_thr);
        f("world.glow.tint", &mut w.glow_tint);
        f("misc.status_line", &mut self.status);
        f("misc.stream_mode", &mut self.stream);
        f("misc.third_person", &mut self.third);
        f("misc.cursor_speed", &mut self.cursor_speed);
    }
}

struct Gl {
    window: Window,
    ctx: PossiblyCurrentContext,
    surface: Surface<WindowSurface>,
    xid: u32,
}

struct App {
    sh: Arc<Shared>,
    aim: Arc<Aim>,
    mods: Arc<Mods>,
    chams: Arc<Chams>,
    amb: Arc<Ambiance>,
    x: X,
    grab: Grab,
    gl: Option<Gl>,
    imgui: imgui::Context,
    renderer: Option<imgui_glow_renderer::AutoRenderer>,
    last: Instant,

    heard: Option<Instant>,
    cfg: Cfg,
    menu: bool,
    input_full: bool,
    prev: (bool, bool, bool),
    geo: Rect,
    geo_at: Instant,
    focused: bool,
    frames: u32,
    fps: f32,
    fps_at: Instant,

    saved: String,
    saved_at: Instant,
}

static STOP: AtomicBool = AtomicBool::new(false);

unsafe extern "C" {
    fn signal(sig: i32, handler: usize) -> usize;
}

extern "C" fn on_signal(_: i32) {
    STOP.store(true, Ordering::Relaxed);
}

pub fn run(sh: Arc<Shared>, aim: Arc<Aim>, mods: Arc<Mods>, chams: Arc<Chams>, amb: Arc<Ambiance>) {
    for sig in [1, 2, 15] {
        unsafe { signal(sig, on_signal as *const () as usize) };
    }
    let Some(x) = X::new() else {
        eprintln!("error: can't connect to the X server, the overlay needs X11 or XWayland");
        std::process::exit(1);
    };
    eprintln!("waiting for the GoreBox window");

    let mut unseen = 0;
    let geo = loop {
        let pid = mem::find_pid();
        if let Some(r) = pid.and_then(|p| x.game(p)) {
            break r;
        }
        unseen = if pid.is_some() { unseen + 1 } else { 0 };
        if unseen == 15 {
            eprintln!("GoreBox is running but its window isn't on this X server (gamescope? Wine's Wayland driver?), see compatibility.md");
        }
        std::thread::sleep(Duration::from_secs(1));
    };
    let mut imgui = imgui::Context::create();
    imgui.set_ini_filename(None);
    let now = Instant::now();
    let mut cfg = Cfg::default();
    settings::load(|f| cfg.walk(f));
    let saved = settings::dump(|f| cfg.walk(f));
    let mut app = App {
        sh,
        aim,
        mods,
        chams,
        amb,
        x,
        grab: Grab::open(),
        gl: None,
        imgui,
        renderer: None,
        last: now,
        heard: None,
        cfg,
        menu: false,
        input_full: false,
        prev: (false, false, false),
        geo,
        geo_at: now,
        focused: true,
        frames: 0,
        fps: 0.0,
        fps_at: now,
        saved,
        saved_at: now,
    };
    let el = EventLoop::new().expect("event loop");
    el.set_control_flow(ControlFlow::Poll);
    el.run_app(&mut app).expect("run");
}

impl App {
    fn save(&mut self) {
        let now = settings::dump(|f| self.cfg.walk(f));
        if now != self.saved {
            settings::save(&now);
            self.saved = now;
        }
    }

    fn create(&mut self, el: &ActiveEventLoop) {
        let r = self.geo;
        let attrs = WindowAttributes::default()
            .with_title("goretux")
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_position(winit::dpi::PhysicalPosition::new(r.x, r.y))
            .with_inner_size(winit::dpi::PhysicalSize::new(r.w, r.h))
            .with_override_redirect(true)
            .with_name("goretux", "goretux")
            .with_visible(false);
        let tmpl = ConfigTemplateBuilder::new().with_alpha_size(8).with_transparency(true);
        let score = |c: &glutin::config::Config| c.supports_transparency().unwrap_or(false) as i32 * 100 - c.num_samples() as i32;
        let (window, cfg) = glutin_winit::DisplayBuilder::new()
            .with_preference(glutin_winit::ApiPreference::FallbackEgl)
            .with_window_attributes(Some(attrs.clone()))
            .build(el, tmpl, |cs| cs.reduce(|a, b| if score(&b) > score(&a) { b } else { a }).expect("no gl config"))
            .expect("gl config");
        let window = window.unwrap_or_else(|| glutin_winit::finalize_window(el, attrs, &cfg).expect("window"));
        let raw = window.window_handle().expect("handle").as_raw();
        let display = cfg.display();
        let ctx = unsafe {
            display
                .create_context(&cfg, &ContextAttributesBuilder::new().build(Some(raw)))
                .or_else(|_| display.create_context(&cfg, &ContextAttributesBuilder::new().with_context_api(ContextApi::Gles(None)).build(Some(raw))))
                .expect("gl context")
        };
        let sz = window.inner_size();
        let nz = |v: u32| NonZeroU32::new(v).unwrap_or(NonZeroU32::MIN);
        let sattr = SurfaceAttributesBuilder::<WindowSurface>::new().build(raw, nz(sz.width), nz(sz.height));
        let surface = unsafe { display.create_window_surface(&cfg, &sattr) }.expect("surface");
        let ctx = ctx.make_current(&surface).expect("make current");
        let _ = surface.set_swap_interval(&ctx, SwapInterval::Wait(NonZeroU32::MIN));
        let glow = unsafe { glow::Context::from_loader_function_cstr(|s| display.get_proc_address(s)) };

        let xid = match raw {
            RawWindowHandle::Xlib(h) => h.window as u32,
            RawWindowHandle::Xcb(h) => h.window.get(),
            _ => 0,
        };

        let depth = (0..100).find_map(|_| {
            self.x.depth(xid).or_else(|| {
                std::thread::sleep(Duration::from_millis(10));
                None
            })
        });
        eprintln!("gl {:?} alpha {} transparent {:?} window 0x{xid:x} depth {depth:?}", cfg.api(), cfg.alpha_size(), cfg.supports_transparency());

        if depth != Some(32) {
            eprintln!("no 32-bit ARGB visual for the overlay, is a compositor running?");
            std::process::exit(1);
        }
        self.x.input(xid, None);
        window.set_visible(true);
        self.x.place(xid, r);

        self.renderer = Some(imgui_glow_renderer::AutoRenderer::new(glow, &mut self.imgui).expect("imgui renderer"));
        self.gl = Some(Gl { window, ctx, surface, xid });
    }

    fn frame(&mut self, el: &ActiveEventLoop) {
        let keys = self.x.keys();
        let (ins, end, home) = (X::down(&keys, self.x.kc_menu), X::down(&keys, self.x.kc_quit), X::down(&keys, self.x.kc_stream));
        if ins && !self.prev.0 {
            self.menu = !self.menu;
        }
        if home && !self.prev.2 {
            self.cfg.stream = !self.cfg.stream;
        }
        if (end && !self.prev.1) || STOP.load(Ordering::Relaxed) {
            el.exit();
            return;
        }
        self.prev = (ins, end, home);

        let snap = self.sh.pick(Duration::from_secs_f32(self.cfg.delay_ms / 1000.0));
        let recent = self.sh.hits(LOG_AGE);
        if let Some(h) = recent.last()
            && self.heard.is_none_or(|t| h.at > t)
        {
            if self.heard.is_some() && self.cfg.hitsound && !self.cfg.stream {
                sound::play(self.cfg.hitsound_vol);
            }
            self.heard = Some(h.at);
        }
        self.sh.tracers.store(self.cfg.trace_own || self.cfg.trace_others, Ordering::Relaxed);
        let trails = self.sh.trails();
        let Some(xid) = self.gl.as_ref().map(|g| g.xid) else { return };

        let mut resync = self.menu != self.input_full;
        if self.geo_at.elapsed() >= Duration::from_millis(500) {
            self.geo_at = Instant::now();
            match self.x.game(snap.pid) {
                Some(r) if r != self.geo => {
                    self.geo = r;
                    self.x.place(xid, r);
                }
                _ => self.x.raise(xid),
            }

            self.focused = self.x.game_win(snap.pid).is_some_and(|w| self.x.active() == Some(w));

            resync = true;
        }
        if resync {
            self.input_full = self.menu;
            let full = Rect { x: 0, y: 0, w: self.geo.w, h: self.geo.h };
            self.x.input(xid, self.menu.then_some(full));
        }

        if self.saved_at.elapsed() >= Duration::from_secs(1) {
            self.saved_at = Instant::now();
            self.save();
        }

        self.frames += 1;
        if self.fps_at.elapsed() >= Duration::from_secs(1) {
            self.fps = self.frames as f32 / self.fps_at.elapsed().as_secs_f32();
            (self.frames, self.fps_at) = (0, Instant::now());
        }

        self.cfg.aim.aura_range = self.cfg.aim.aura_range.clamp(1.0, 4.0);
        *self.aim.cfg.lock().unwrap() = self.cfg.aim;
        *self.mods.cfg.lock().unwrap() = self.cfg.mods;

        let chams = if self.cfg.stream { chams::Cfg { on: false, weapon: false, local: false, ..self.cfg.chams } } else { self.cfg.chams };
        *self.chams.cfg.lock().unwrap() = chams;
        self.chams.third.store(self.cfg.third, Ordering::Relaxed);
        *self.amb.cfg.lock().unwrap() = if self.cfg.stream { ambiance::Cfg::default() } else { self.cfg.world };
        self.aim.menu.store(self.menu, Ordering::Relaxed);
        self.aim.focused.store(self.focused, Ordering::Relaxed);
        let (target, silent, silent_bone, aim_status, calib) = {
            let st = self.aim.state.lock().unwrap();
            let mut status = if self.cfg.aim.silent { format!("{}  silent decoy {}", st.status, st.decoy) } else { st.status.clone() };
            if self.cfg.aim.magic() {
                status += &format!("  magic {} bullets", st.moved);
            }
            (st.target, st.silent, st.silent_bone, status, st.calib)
        };
        let vis = if snap.vis_tris > 0 { format!("vis {}k tris", snap.vis_tris / 1000) } else { "vis building".into() };
        let chams_status = self.chams.status.lock().unwrap().clone();
        let world_status = self.amb.status.lock().unwrap().clone();
        let third_status = self.chams.third_status.lock().unwrap().clone();
        let line = format!("{}  {}  aim {}  {chams_status}  {third_status}  {world_status}  {:.0}hz read  {:.0}fps", snap.status, vis, aim_status, snap.rate, self.fps);

        let ptr = self.x.pointer();
        if self.menu && self.focused != self.grab.on {
            let at = ptr.map_or([0.0; 2], |(x, y, _)| [(x - self.geo.x) as f32, (y - self.geo.y) as f32]);
            self.grab.set(self.menu && self.focused, at);
        } else if !self.menu && self.grab.on {
            self.grab.set(false, [0.0; 2]);
        }

        let (Some(gl), Some(renderer)) = (self.gl.as_ref(), self.renderer.as_mut()) else { return };
        let size = gl.window.inner_size();
        self.grab.poll([size.width as f32, size.height as f32], self.cfg.cursor_speed);
        let io = self.imgui.io_mut();

        io.mouse_draw_cursor = self.grab.on;
        if self.grab.on {
            io.add_mouse_pos_event(self.grab.pos);
            for (b, down) in [MouseButton::Left, MouseButton::Right, MouseButton::Middle].into_iter().zip(self.grab.btn) {
                io.add_mouse_button_event(b, down);
            }
            if self.grab.wheel != [0.0; 2] {
                io.add_mouse_wheel_event(std::mem::take(&mut self.grab.wheel));
            }
        } else if let Some((px, py, mask)) = ptr {
            io.add_mouse_pos_event([(px - self.geo.x) as f32, (py - self.geo.y) as f32]);
            for (b, bit) in [(MouseButton::Left, 0x100), (MouseButton::Middle, 0x200), (MouseButton::Right, 0x400)] {
                io.add_mouse_button_event(b, mask & bit != 0);
            }
        }
        io.display_size = [size.width as f32, size.height as f32];
        io.display_framebuffer_scale = [1.0, 1.0];
        io.update_delta_time(self.last.elapsed());
        self.last = Instant::now();

        let (cfg, menu, focused, aim) = (&mut self.cfg, self.menu, self.focused, &self.aim);
        let mut quit = false;
        let ui = self.imgui.new_frame();
        let scr = [size.width as f32, size.height as f32];
        {
            let dl = ui.get_background_draw_list();
            if snap.ok && focused && !cfg.stream {
                for e in &snap.ents {
                    esp(&dl, ui, scr, &snap, e, cfg, e.id == target, (e.id == silent).then_some(silent_bone));
                    if cfg.arrows {
                        arrow(&dl, scr, &snap, e, cfg);
                    }
                }
                tracers(&dl, scr, &snap, &trails, cfg);

                let ring = |fov: f32, a: f32| {
                    let r = fov.min(89.0).to_radians().tan() * snap.fy * scr[1] * 0.5;
                    dl.add_circle([scr[0] / 2.0, scr[1] / 2.0], r, [1.0, 1.0, 1.0, a]).num_segments(64).build();
                };
                let ea = cfg.aim.effective();
                if (ea.on || (ea.silent && !ea.magic())) && ea.draw_fov && !ea.rage {
                    ring(ea.fov, 0.35);
                }
                if ea.magic() && ea.draw_fov && ea.magic_fov < 89.0 {
                    ring(ea.magic_fov, 0.2);
                }
            }
            if focused && !cfg.stream {
                if cfg.status {
                    text(&dl, [8.0, 6.0], &line, WHITE);
                }
                if cfg.hitmarker
                    && let Some(h) = recent.last()
                {
                    hitmarker(&dl, [scr[0] / 2.0, scr[1] / 2.0], h);
                }
                if cfg.hitlog {
                    hitlog(&dl, scr, &recent, cfg);
                }
            }
        }
        if menu {
            ui.window("goretux").size([430.0, 520.0], Condition::FirstUseEver).position([40.0, 40.0], Condition::FirstUseEver).build(|| settings(ui, cfg, &snap, &line, aim, calib, &mut quit));
        }
        let draw = self.imgui.render();
        if quit {
            el.exit();
            return;
        }
        unsafe {
            use glow::HasContext;
            let g = renderer.gl_context();
            g.clear_color(0.0, 0.0, 0.0, 0.0);
            g.clear(glow::COLOR_BUFFER_BIT);
        }
        let _ = renderer.render(draw);
        let _ = gl.surface.swap_buffers(&gl.ctx);
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.gl.is_none() {
            self.create(el);
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, ev: WindowEvent) {
        let io = self.imgui.io_mut();
        match ev {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::RedrawRequested => self.frame(el),
            WindowEvent::Resized(sz) => {
                if let Some(gl) = &self.gl {
                    let nz = |v: u32| NonZeroU32::new(v).unwrap_or(NonZeroU32::MIN);
                    gl.surface.resize(&gl.ctx, nz(sz.width), nz(sz.height));
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (x, y) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x, y),
                    MouseScrollDelta::PixelDelta(p) => (p.x as f32 / 40.0, p.y as f32 / 40.0),
                };
                io.add_mouse_wheel_event([x, y]);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _: &ActiveEventLoop) {
        if let Some(gl) = &self.gl {
            gl.window.request_redraw();
        }
    }

    fn exiting(&mut self, _: &ActiveEventLoop) {
        self.save();
        self.chams.shutdown();
        self.amb.shutdown();
        self.mods.shutdown();
    }
}

fn w2s(vp: &[f32; 16], p: V3, scr: [f32; 2]) -> Option<[f32; 2]> {
    let x = vp[0] * p[0] + vp[4] * p[1] + vp[8] * p[2] + vp[12];
    let y = vp[1] * p[0] + vp[5] * p[1] + vp[9] * p[2] + vp[13];
    let w = vp[3] * p[0] + vp[7] * p[1] + vp[11] * p[2] + vp[15];
    if w < 0.05 {
        return None;
    }
    Some([(x / w * 0.5 + 0.5) * scr[0], (0.5 - y / w * 0.5) * scr[1]])
}

fn rgb(c: [f32; 3], a: f32) -> Col {
    [c[0], c[1], c[2], a]
}

fn alpha(c: Col, a: f32) -> Col {
    [c[0], c[1], c[2], c[3] * a]
}

fn text(dl: &DrawListMut, at: [f32; 2], s: &str, c: Col) {
    for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
        dl.add_text([at[0] + dx, at[1] + dy], alpha(BLACK, c[3]), s);
    }
    dl.add_text(at, c, s);
}

#[allow(clippy::too_many_arguments)]
fn shown(e: &Ent, cfg: &Cfg) -> bool {
    !(e.npc && !cfg.npcs) && !(e.dead && !cfg.dead) && !(e.team && !cfg.teammates) && e.dist <= cfg.max_dist
}

fn color(s: &Snap, e: &Ent, cfg: &Cfg) -> Col {
    let base = if e.npc { cfg.npc } else { cfg.player };
    match (e.dead, cfg.vis && s.vis_tris > 0, e.vis != 0) {
        (true, ..) => [0.5, 0.5, 0.5, 1.0],
        _ if e.team => rgb(cfg.team_col, 1.0),
        (_, true, true) => rgb(cfg.seen, 1.0),
        (_, true, false) => rgb(base, 0.8),
        _ => rgb(base, 1.0),
    }
}

fn esp(dl: &DrawListMut, ui: &imgui::Ui, scr: [f32; 2], s: &Snap, e: &Ent, cfg: &Cfg, target: bool, silent: Option<usize>) {
    if !shown(e, cfg) {
        return;
    }
    let col = color(s, e, cfg);
    let pts: Vec<Option<[f32; 2]>> = e.bones.iter().map(|b| b.and_then(|b| w2s(&s.vp, b, scr))).collect();
    let bones = e.bones.iter().filter(|b| b.is_some()).count();

    let mut on: Vec<[f32; 2]> = pts.iter().flatten().copied().collect();
    if let Some(h) = e.bones[HEAD] {
        let up = aim::head_up(e);
        on.extend(w2s(&s.vp, std::array::from_fn(|k| h[k] + up[k] * aim::SKULL_TOP), scr));
    }
    if bones < 4 {
        let r = e.root;
        on = [[r[0], r[1] - 0.9, r[2]], [r[0], r[1] + 0.9, r[2]]].iter().filter_map(|&v| w2s(&s.vp, v, scr)).collect();
    }
    if on.len() < 2 {
        return;
    }
    let (mut lo, mut hi) = (on[0], on[0]);
    for q in &on {
        lo = [lo[0].min(q[0]), lo[1].min(q[1])];
        hi = [hi[0].max(q[0]), hi[1].max(q[1])];
    }
    let h = (hi[1] - lo[1]).max(4.0);
    let cx = (lo[0] + hi[0]) * 0.5;
    let half = ((hi[0] - lo[0]) * 0.5).max(h * 0.22) + h * 0.06;
    let (a, b) = ([(cx - half).round(), (lo[1] - h * 0.06).round()], [(cx + half).round(), (hi[1] + h * 0.03).round()]);
    if b[0] < 0.0 || b[1] < 0.0 || a[0] > scr[0] || a[1] > scr[1] {
        return;
    }

    if cfg.snaplines {
        dl.add_line([scr[0] / 2.0, scr[1]], [cx, b[1]], col).build();
    }
    if cfg.boxes {
        dl.add_rect([a[0] - 1.0, a[1] - 1.0], [b[0] + 1.0, b[1] + 1.0], BLACK).build();
        dl.add_rect([a[0] + 1.0, a[1] + 1.0], [b[0] - 1.0, b[1] - 1.0], BLACK).build();
        dl.add_rect(a, b, col).build();
    }
    if cfg.skeleton {
        for &(i, j) in &LINKS {
            if let (Some(p), Some(q)) = (pts[i], pts[j]) {
                dl.add_line(p, q, WHITE).build();
            }
        }
    }
    if cfg.head
        && let (Some(c), Some(j)) = (aim::spot(e, HEAD).and_then(|p| w2s(&s.vp, p, scr)), pts[HEAD])
    {
        let r = ((c[0] - j[0]).powi(2) + (c[1] - j[1]).powi(2)).sqrt().max(2.0);
        dl.add_circle(c, r, col).build();
    }

    let bone = silent.unwrap_or(cfg.aim.bone);
    if (target || silent.is_some())
        && let Some(p) = aim::spot(e, bone).and_then(|p| w2s(&s.vp, p, scr))
    {
        dl.add_line([scr[0] / 2.0, scr[1] / 2.0], p, WHITE).build();
        if target {
            dl.add_circle(p, 2.0, WHITE).filled(true).build();
        }
    }
    if cfg.names {
        let w = ui.calc_text_size(&e.name)[0];
        text(dl, [(cx - w / 2.0).round(), a[1] - 15.0], &e.name, col);
    }
    if cfg.dist {
        let d = format!("{:.0}m", e.dist);
        let w = ui.calc_text_size(&d)[0];
        text(dl, [(cx - w / 2.0).round(), b[1] + 2.0], &d, WHITE);
    }
}

fn arrow(dl: &DrawListMut, scr: [f32; 2], s: &Snap, e: &Ent, cfg: &Cfg) {
    if !shown(e, cfg) {
        return;
    }
    let p = e.bones[CHEST].unwrap_or(e.root);
    if w2s(&s.vp, p, scr).is_some_and(|q| (0.0..=scr[0]).contains(&q[0]) && (0.0..=scr[1]).contains(&q[1])) {
        return;
    }

    let l = s.view[0].hypot(s.view[8]).max(1e-6);
    let (rx, rz) = (s.view[0] / l, s.view[8] / l);
    let (dx, dz) = (p[0] - s.cam[0], p[2] - s.cam[2]);
    let ang = (dx * rx + dz * rz).atan2(dx * -rz + dz * rx);
    let (dir, side) = ([ang.sin(), -ang.cos()], [ang.cos(), ang.sin()]);
    let (c, r, sz) = ([scr[0] / 2.0, scr[1] / 2.0], cfg.arrow_radius, cfg.arrow_size);
    let at = |d: f32, w: f32| [c[0] + dir[0] * d + side[0] * w, c[1] + dir[1] * d + side[1] * w];

    let (tip, right, notch, left) = (at(r + sz * 1.2, 0.0), at(r, sz * 0.6), at(r + sz * 0.35, 0.0), at(r, -sz * 0.6));
    let k = color(s, e, cfg);
    let col = [k[0], k[1], k[2], 1.0];
    dl.add_triangle(tip, right, notch, col).filled(true).build();
    dl.add_triangle(tip, notch, left, col).filled(true).build();
    dl.add_polyline(vec![tip, right, notch, left, tip], BLACK).build();
}

fn tracers(dl: &DrawListMut, scr: [f32; 2], s: &Snap, trails: &[Trail], cfg: &Cfg) {
    for t in trails {
        if !(if t.own { cfg.trace_own } else { cfg.trace_others }) {
            continue;
        }
        let a = t.end.map_or(1.0, |e| 1.0 - e.elapsed().as_secs_f32() / tracers::FADE.as_secs_f32()).clamp(0.0, 1.0);
        let col = if t.own { rgb(cfg.trace_col, a) } else { alpha(RED, a) };
        let pts: Vec<Option<[f32; 2]>> = t.pts.iter().map(|&v| w2s(&s.vp, v, scr)).collect();
        for w in pts.windows(2) {
            if let [Some(p), Some(q)] = w {
                dl.add_line(*p, *q, col).build();
            }
        }
    }
}

const LOG_AGE: Duration = Duration::from_secs(5);
const MARK_AGE: f32 = 0.3;

fn hitmarker(dl: &DrawListMut, c: [f32; 2], h: &Hit) {
    let age = h.at.elapsed().as_secs_f32();
    if age > MARK_AGE {
        return;
    }
    let col = alpha(if h.kind == 2 { RED } else { WHITE }, 1.0 - age / MARK_AGE);
    for (dx, dy) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
        dl.add_line([c[0] + dx * 4.0, c[1] + dy * 4.0], [c[0] + dx * 10.0, c[1] + dy * 10.0], col).thickness(1.5).build();
    }
}

fn hitlog(dl: &DrawListMut, scr: [f32; 2], hits: &[Hit], cfg: &Cfg) {
    let mut y = scr[1] / 2.0 - 60.0;
    for h in hits.iter().rev() {
        let a = (LOG_AGE.as_secs_f32() - h.at.elapsed().as_secs_f32()).clamp(0.0, 1.0);
        let part = hits::part(h.part);
        let msg = match h.kind {
            2 => format!("killed {} ({part})", h.name),
            1 => format!("severed {}'s {part}", h.name),
            _ => format!("hit {} in the {part}", h.name),
        };
        let col = if h.kind == 2 { RED } else { rgb(if h.npc { cfg.npc } else { cfg.player }, 1.0) };
        text(dl, [12.0, y], &msg, alpha(col, a));
        y += 15.0;
    }
}

fn settings(ui: &imgui::Ui, cfg: &mut Cfg, s: &Snap, line: &str, aim: &Aim, calib: Option<(f32, f32)>, quit: &mut bool) {
    let Some(_bar) = ui.tab_bar("tabs") else { return };
    if let Some(_t) = ui.tab_item("esp") {
        ui.checkbox("box", &mut cfg.boxes);
        ui.checkbox("skeleton", &mut cfg.skeleton);
        ui.checkbox("head circle", &mut cfg.head);
        ui.checkbox("names", &mut cfg.names);
        ui.checkbox("distance", &mut cfg.dist);
        ui.checkbox("snaplines", &mut cfg.snaplines);
        ui.checkbox("npcs", &mut cfg.npcs);
        ui.checkbox("dead bodies", &mut cfg.dead);
        ui.slider("range", 10.0, 1000.0, &mut cfg.max_dist);

        ui.slider("esp delay ms", 0.0, 60.0, &mut cfg.delay_ms);
        ui.checkbox("vis check", &mut cfg.vis);
        ui.color_edit3("players", &mut cfg.player);
        ui.color_edit3("npcs##col", &mut cfg.npc);
        ui.color_edit3("visible", &mut cfg.seen);
        ui.checkbox("teammates", &mut cfg.teammates);
        ui.color_edit3("team", &mut cfg.team_col);
        ui.checkbox("off-screen arrows", &mut cfg.arrows);
        ui.slider("arrow radius", 60.0, 700.0, &mut cfg.arrow_radius);
        ui.slider("arrow size", 6.0, 30.0, &mut cfg.arrow_size);
        ui.separator();
        ui.checkbox("hitmarker", &mut cfg.hitmarker);
        ui.checkbox("hitsound", &mut cfg.hitsound);
        ui.slider("hitsound volume", 0.0, 1.0, &mut cfg.hitsound_vol);
        if sound::file().is_none() {
            ui.text_disabled(format!("no hitsound.wav in {}", settings::dir().display()));
        }
        ui.checkbox("hit log", &mut cfg.hitlog);
        ui.checkbox("tracers: mine", &mut cfg.trace_own);
        ui.checkbox("tracers: others", &mut cfg.trace_others);
        ui.color_edit3("tracer color", &mut cfg.trace_col);
        ui.separator();
        ui.checkbox("chams", &mut cfg.chams.on);
        ui.checkbox("chams on npcs", &mut cfg.chams.npcs);
        ui.checkbox("chams on dead bodies", &mut cfg.chams.dead);
        ui.color_edit4("chams visible", &mut cfg.chams.vis);
        ui.color_edit4("chams hidden", &mut cfg.chams.hid);
        ui.checkbox("weapon chams", &mut cfg.chams.weapon);
        ui.checkbox("local player chams", &mut cfg.chams.local);
        ui.checkbox("rainbow", &mut cfg.chams.rainbow);
        if cfg.chams.rainbow {
            ui.slider("rainbow speed", 0.02, 1.0, &mut cfg.chams.speed);
        } else {
            ui.color_edit3("glow color", &mut cfg.chams.glow_col);
        }
        ui.slider("glow", 0.0, 8.0, &mut cfg.chams.glow);
    }
    if let Some(_t) = ui.tab_item("aim") {
        let a = &mut cfg.aim;
        ui.checkbox("ragebot", &mut a.rage);
        ui.checkbox("team check (all aim, ragebot, aura)", &mut a.team_check);
        if a.rage {
            let shots = aim.state.lock().unwrap().rage_shots;
            ui.text_disabled(format!("camera silent aim at everyone in sight, head when visible,\nno spread or recoil, fires itself when a part is visible ({shots} fired)"));
        }
        let pitches: Vec<&str> = aa::Pitch::ALL.iter().map(|p| p.label()).collect();
        let mut pi = aa::Pitch::ALL.iter().position(|&p| p == a.aa.pitch).unwrap_or(0);
        if ui.combo_simple_string("anti-aim pitch", &mut pi, &pitches) {
            a.aa.pitch = aa::Pitch::ALL[pi];
        }
        let yaws: Vec<&str> = aa::Yaw::ALL.iter().map(|y| y.label()).collect();
        let mut yi = aa::Yaw::ALL.iter().position(|&y| y == a.aa.yaw).unwrap_or(0);
        if ui.combo_simple_string("anti-aim yaw", &mut yi, &yaws) {
            a.aa.yaw = aa::Yaw::ALL[yi];
        }
        if a.aa.yaw == aa::Yaw::Spin {
            ui.slider("spin speed (deg/s)", 90.0, 2000.0, &mut a.aa.spin);
        }
        let aa_status = aim.state.lock().unwrap().aa.clone();
        if !aa_status.is_empty() {
            ui.text_disabled(format!("{aa_status} (what others see, your view stays)"));
        }
        ui.separator();
        ui.checkbox("aimbot", &mut a.on);
        ui.checkbox("silent aim", &mut a.silent);
        let methods: Vec<&str> = Method::ALL.iter().map(|m| m.label()).collect();
        let mut mi = Method::ALL.iter().position(|&m| m == a.method).unwrap_or(0);
        if ui.combo_simple_string("method", &mut mi, &methods) {
            a.method = Method::ALL[mi];
        }
        if a.method == Method::Magic {
            ui.slider("magic fov", 1.0, 180.0, &mut a.magic_fov);
            ui.checkbox("head when visible", &mut a.head_vis);
            let (reach, host) = {
                let st = aim.state.lock().unwrap();
                (st.reach, st.host)
            };
            ui.text_disabled(if host {
                "you host: hidden targets hit through anything".to_string()
            } else if reach > 0.0 {
                format!("hidden targets: shots start up to {reach:.1}m out")
            } else {
                "hidden targets: no fire origin limit read yet".to_string()
            });
        }
        if a.silent || a.rage {
            let (host, ping, lead) = {
                let st = aim.state.lock().unwrap();
                (st.host, st.ping, st.lead_ms)
            };
            ui.slider("prediction (s)", -0.2, 0.3, &mut a.predict);
            ui.text_disabled(if host { "you host: no network lead needed".to_string() } else { format!("ping {ping:.0}ms, moving targets led {lead:.0}ms + bullet flight") });
        }
        let labels: Vec<&str> = Key::ALL.iter().map(|k| k.label()).collect();
        let mut k = Key::ALL.iter().position(|&k| k == a.key).unwrap_or(0);
        if ui.combo_simple_string("key", &mut k, &labels) {
            a.key = Key::ALL[k];
        }
        let bones = [HEAD, NECK, CHEST, PELVIS];
        let mut b = bones.iter().position(|&b| b == a.bone).unwrap_or(0);
        if ui.combo_simple_string("bone", &mut b, &["head", "neck", "chest", "pelvis"]) {
            a.bone = bones[b];
        }
        ui.slider("fov", 1.0, 45.0, &mut a.fov);
        ui.slider("smooth", 1.0, 20.0, &mut a.smooth);
        ui.slider("aim range", 10.0, 1000.0, &mut a.max_dist);
        ui.checkbox("players##aim", &mut a.players);
        ui.checkbox("npcs##aim", &mut a.npcs);
        ui.checkbox("visible only", &mut a.vis_only);
        ui.checkbox("draw fov", &mut a.draw_fov);
        ui.separator();
        ui.checkbox("melee aura (fists)", &mut a.aura);
        ui.slider("aura range", 1.0, 4.0, &mut a.aura_range);
        ui.checkbox("auto punch", &mut a.auto_punch);
        ui.separator();
        ui.text("silent aim hit chance");
        for (i, (_, name)) in silent::PARTS.iter().enumerate() {
            ui.slider(name, 0u8, 100u8, &mut a.hit[i]);
        }
        ui.separator();
        if ui.button("calibrate") {
            aim.calibrate.store(true, Ordering::Relaxed);
        }
        ui.same_line();
        match calib {
            Some((kx, ky)) => ui.text(format!("{kx:.5} / {ky:.5} deg per count")),
            None => ui.text_disabled("not calibrated, hold the aim key in game"),
        }
    }
    if let Some(_t) = ui.tab_item("weapon") {
        let w = &mut cfg.mods;
        ui.checkbox("no recoil", &mut w.no_recoil);
        ui.checkbox("no spread", &mut w.no_spread);
        ui.checkbox("no sway", &mut w.no_sway);
        ui.checkbox("zoom while aiming", &mut w.zoom);
        ui.slider("magnification", 1.2, 8.0, &mut w.zoom_mag);
    }
    if let Some(_t) = ui.tab_item("world") {
        let w = &mut cfg.world;
        for (i, (name, preset)) in ambiance::PRESETS.iter().enumerate() {
            if i % 4 != 0 {
                ui.same_line();
            }
            if ui.button(name) {
                *w = *preset;
            }
        }
        if ui.button("game default") {
            *w = ambiance::Cfg::default();
        }
        ui.separator();
        ui.checkbox("color grading", &mut w.grade);
        ui.color_edit3("color filter", &mut w.filter);
        ui.slider("hue shift", -180.0, 180.0, &mut w.hue);
        ui.slider("saturation", -100.0, 100.0, &mut w.sat);
        ui.slider("contrast", -100.0, 100.0, &mut w.contrast);
        ui.slider("exposure", -3.0, 3.0, &mut w.exposure);
        ui.separator();
        ui.checkbox("glow", &mut w.glow);
        ui.slider("glow strength", 0.0, 10.0, &mut w.glow_int);
        ui.slider("glow threshold", 0.0, 2.0, &mut w.glow_thr);
        ui.color_edit3("glow tint", &mut w.glow_tint);
        ui.separator();
        ui.checkbox("time of day", &mut w.time);
        ui.slider("hour", 0.0, 24.0, &mut w.hour);
        ui.separator();
        ui.checkbox("world tint", &mut w.tint);
        ui.color_edit3("sky light", &mut w.sky);
        ui.color_edit3("ground light", &mut w.ground);
        ui.slider("brightness", 0.1, 3.0, &mut w.bright);
        ui.separator();
        ui.checkbox("fog", &mut w.fog);
        ui.slider("fog density", 0.0, 5.0, &mut w.fog_scale);
        ui.separator();
        ui.checkbox("weather", &mut w.weather);
        ui.slider("clouds", 0.0, 1.0, &mut w.clouds);
        ui.slider("storm", 0.0, 1.0, &mut w.storm);
        ui.slider("rain", 0.0, 1.0, &mut w.rain);
    }
    if let Some(_t) = ui.tab_item("misc") {
        ui.checkbox("third person", &mut cfg.third);
        ui.checkbox("status line on screen", &mut cfg.status);
        ui.checkbox("stream mode (home)", &mut cfg.stream);
        if cfg.stream {
            ui.text_disabled("hidden from recordings: esp, chams, sky, tracers, hit log, hitsound");
        }
        ui.slider("menu cursor speed", 0.2, 4.0, &mut cfg.cursor_speed);
        ui.text_wrapped(format!("pid {}  {}", s.pid, line));
        ui.text_disabled("insert: menu   home: stream mode   end: quit");
        ui.text_disabled(format!("settings save to {}", settings::dir().join("settings.txt").display()));
        if ui.button("reset settings") {
            *cfg = Cfg::default();
        }
        ui.same_line();
        if ui.button("unload") {
            *quit = true;
        }
    }
}
