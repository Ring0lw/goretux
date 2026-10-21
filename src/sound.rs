use crate::settings;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

const NAMES: [&str; 3] = ["hitsound.wav", "hitsound.mp3", "hitsound.ogg"];
const PLAYERS: [&str; 3] = ["pw-play", "paplay", "aplay"];

static PLAYER: AtomicUsize = AtomicUsize::new(0);

pub fn file() -> Option<PathBuf> {
    NAMES.iter().map(|n| settings::dir().join(n)).find(|p| p.is_file())
}

pub fn play(volume: f32) {
    let Some(p) = file() else { return };
    let v = volume.clamp(0.0, 1.0);
    for i in PLAYER.load(Ordering::Relaxed)..PLAYERS.len() {
        let mut cmd = Command::new(PLAYERS[i]);
        match i {
            0 => cmd.arg(format!("--volume={v:.2}")),
            1 => cmd.arg(format!("--volume={}", (v * 65536.0) as u32)),
            _ => cmd.arg("-q"),
        };
        if let Ok(mut c) = cmd.arg(&p).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
            PLAYER.store(i, Ordering::Relaxed);
            std::thread::spawn(move || c.wait());
            return;
        }
    }
    PLAYER.store(PLAYERS.len(), Ordering::Relaxed);
}
