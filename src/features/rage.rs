use crate::os::uinput::Mouse;
use std::time::{Duration, Instant};

const PRESS: Duration = Duration::from_millis(30);

const MIN_GAP: Duration = Duration::from_millis(60);
const SLACK: Duration = Duration::from_millis(10);

const LINGER: Duration = Duration::from_millis(60);

#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Gun {
    pub auto: bool,
    pub cooldown: f32,
}

#[derive(Default)]
pub struct Trigger {
    mouse: Option<Mouse>,
    down: Option<Instant>,
    last: Option<Instant>,
    wanted: Option<Instant>,
    pub shots: u64,
}

impl Trigger {
    pub fn tick(&mut self, fire: bool, gun: Option<Gun>) {
        let now = Instant::now();
        if fire {
            self.wanted = Some(now);
        }
        let Some(g) = gun else { return self.lift() };
        let auto_hold = g.auto && self.wanted.is_some_and(|w| w.elapsed() < LINGER);
        if !fire && !auto_hold {
            if self.down.is_some_and(|d| g.auto || d.elapsed() >= PRESS) {
                self.lift();
            }
            return;
        }
        if self.mouse.is_none() {
            self.mouse = Mouse::new().ok();
        }
        let Some(mouse) = self.mouse.as_mut() else { return };
        if g.auto {
            if self.down.is_none() && mouse.left(true).is_ok() {
                (self.down, self.last) = (Some(now), Some(now));
                self.shots += 1;
            }
            return;
        }
        if let Some(at) = self.down {
            if at.elapsed() >= PRESS && mouse.left(false).is_ok() {
                self.down = None;
            }
            return;
        }
        let gap = Duration::from_secs_f32(g.cooldown.clamp(0.0, 2.0)).max(MIN_GAP) + SLACK;
        if self.last.is_none_or(|l| l.elapsed() >= gap) && mouse.left(true).is_ok() {
            (self.down, self.last) = (Some(now), Some(now));
            self.shots += 1;
        }
    }

    pub fn lift(&mut self) {
        if self.down.take().is_some()
            && let Some(mouse) = self.mouse.as_mut()
        {
            let _ = mouse.left(false);
        }
    }

    pub fn idle(&self) -> bool {
        self.down.is_none()
    }
}
