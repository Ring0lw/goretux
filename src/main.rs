mod features;
mod game;
mod os;
mod settings;
mod sound;
mod ui;

use std::sync::Arc;

#[cfg(not(target_os = "linux"))]
compile_error!("only linux is supported.");

const USAGE: &str = "\
goretux: external cheat for GoreBox on Linux

usage: goretux [option]

  -h, --help       this text
  -V, --version    print the version

with no option it waits for the game and opens the overlay.
insert opens the menu, home toggles stream mode, end quits.
settings live in $XDG_CONFIG_HOME/goretux (or ~/.config/goretux).

tools for finding things again after a game update:
  --classes <text>          classes whose name contains <text>
  --fields <class>...       a class's fields and offsets, statics included
  --methods <class>[:a,b]   a class's methods, their code addresses and parameters
  --field-search <name>...  classes that have a field with that name (obfuscated ones too)
  --third-test              check everything third person needs
  --aa-test dry             check everything anti-aim needs (writes its stubs into unused
                            memory, patches nothing)
the others only read.
";

unsafe extern "C" {
    fn geteuid() -> u32;
}

fn preflight() -> bool {
    if std::env::var_os("DISPLAY").is_none_or(|d| d.is_empty()) {
        eprintln!("error: DISPLAY is not set. the overlay is an X11 window and needs an X server or XWayland");
        return false;
    }
    if unsafe { geteuid() } == 0 {
        eprintln!("warning: running as root. settings go to root's config dir and the X server may refuse the overlay, see compatibility.md for running as your user");
    }
    if let Err(e) = os::uinput::check() {
        eprintln!("warning: {e}. the aimbot, the ragebot's trigger and auto punch need it, the rest works without");
    }
    true
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        return print!("{USAGE}");
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        return println!("goretux {}", env!("CARGO_PKG_VERSION"));
    }
    if args.iter().any(|a| a == "--third-test") {
        return features::third::report();
    }
    if args.iter().any(|a| a == "--aa-test") {
        return game::aa_report();
    }
    if let Some(i) = args.iter().position(|a| a == "--fists") {
        return game::fists_report(args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(60));
    }
    if args.iter().any(|a| a == "--melee-probe") {
        return game::sdk::melee_probe();
    }
    if args.iter().any(|a| a == "--grade-test") {
        return features::ambiance::grade_test();
    }
    if args.iter().any(|a| a == "--volumes") {
        return features::ambiance::volumes_report();
    }
    if args.iter().any(|a| a == "--cam-probe") {
        return game::sdk::cam_probe();
    }
    if args.iter().any(|a| a == "--fire-limits") {
        return game::sdk::fire_limits_report();
    }
    if args.iter().any(|a| a == "--sides") {
        return game::sdk::sides_report();
    }
    if args.iter().any(|a| a == "--registry") {
        return game::sdk::registry_report();
    }
    if let Some(i) = args.iter().position(|a| a == "--shots") {
        return features::magic::shots(args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(60));
    }
    if args.iter().any(|a| a == "--pool-owner") {
        return features::magic::owner();
    }
    if args.iter().any(|a| a == "--find-bullets") {
        return features::magic::find();
    }
    if args.iter().any(|a| a == "--pools") {
        return features::magic::pools();
    }
    if args.iter().any(|a| a == "--bullets") {
        return features::magic::dump();
    }
    if args.iter().any(|a| a == "--chams-explore") {
        return features::chams::explore();
    }
    if args.iter().any(|a| a == "--chams-debug") {
        return features::chams::debug();
    }
    if args.iter().any(|a| a == "--remote-test") {
        return features::chams::test();
    }
    if let Some(i) = args.iter().position(|a| a == "--field-search") {
        return game::il2::field_search(&args[i + 1..]);
    }
    if let Some(i) = args.iter().position(|a| a == "--fields") {
        return game::il2::fields_report(&args[i + 1..]);
    }
    if let Some(i) = args.iter().position(|a| a == "--classes") {
        return game::il2::classes_report(&args[i + 1..]);
    }
    if let Some(i) = args.iter().position(|a| a == "--methods") {
        return game::il2::methods_report(&args[i + 1..]);
    }
    if std::env::args().any(|a| a == "--vis") {
        return game::vis_report();
    }
    if !preflight() {
        std::process::exit(1);
    }

    let Some(_running) = settings::lock("overlay.lock") else {
        eprintln!("goretux is already running (or {} isn't writable)", settings::dir().display());
        std::process::exit(1);
    };

    game::undo::install();
    let sh = Arc::new(game::Shared::default());
    let aim = Arc::new(features::aim::Aim::default());
    let mods = Arc::new(features::mods::Mods::default());
    let chams = Arc::new(features::chams::Chams::default());
    let amb = Arc::new(features::ambiance::Ambiance::default());
    game::spawn(sh.clone(), mods.clone(), aim.clone());
    features::aim::spawn(sh.clone(), aim.clone());
    features::chams::spawn(sh.clone(), chams.clone());
    features::ambiance::spawn(sh.clone(), amb.clone());
    ui::overlay::run(sh, aim, mods, chams, amb);
}
