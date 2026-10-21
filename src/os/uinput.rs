use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::os::fd::AsRawFd;

const UI_SET_EVBIT: u64 = 0x4004_5564;
const UI_SET_KEYBIT: u64 = 0x4004_5565;
const UI_SET_RELBIT: u64 = 0x4004_5566;
const UI_DEV_SETUP: u64 = 0x405c_5503;
const UI_DEV_CREATE: u64 = 0x5501;
const UI_DEV_DESTROY: u64 = 0x5502;

const EV_SYN: u16 = 0;
const EV_KEY: u16 = 1;
const EV_REL: u16 = 2;
const REL_X: u16 = 0;
const REL_Y: u16 = 1;
const BTN_LEFT: i32 = 0x110;
const BTN_RIGHT: i32 = 0x111;
const BUS_USB: u16 = 3;

unsafe extern "C" {
    fn ioctl(fd: i32, req: u64, ...) -> i32;
}

#[repr(C)]
struct Setup {
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
    name: [u8; 80],
    ff_effects_max: u32,
}

#[repr(C)]
struct Event {
    sec: i64,
    usec: i64,
    ty: u16,
    code: u16,
    value: i32,
}

pub fn check() -> Result<(), String> {
    if !std::path::Path::new("/dev/uinput").exists() {
        return Err("the uinput kernel module is not loaded (run setup.sh, or sudo modprobe uinput)".into());
    }
    OpenOptions::new().write(true).open("/dev/uinput").map(|_| ()).map_err(|e| format!("can't open /dev/uinput: {e} (run setup.sh, then log out and back in)"))
}

pub struct Mouse {
    f: File,
}

impl Mouse {
    pub fn new() -> io::Result<Mouse> {
        let f = OpenOptions::new().write(true).open("/dev/uinput")?;
        let fd = f.as_raw_fd();
        let call = |req: u64, arg: i64| {
            if unsafe { ioctl(fd, req, arg) } < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
        };
        call(UI_SET_EVBIT, EV_KEY as i64)?;

        call(UI_SET_KEYBIT, BTN_LEFT as i64)?;
        call(UI_SET_KEYBIT, BTN_RIGHT as i64)?;
        call(UI_SET_EVBIT, EV_REL as i64)?;
        call(UI_SET_RELBIT, REL_X as i64)?;
        call(UI_SET_RELBIT, REL_Y as i64)?;
        let mut name = [0u8; 80];
        name[..11].copy_from_slice(b"goretux-aim");
        let setup = Setup { bustype: BUS_USB, vendor: 0x1234, product: 0x5678, version: 1, name, ff_effects_max: 0 };
        if unsafe { ioctl(fd, UI_DEV_SETUP, &setup as *const Setup) } < 0 {
            return Err(io::Error::last_os_error());
        }
        call(UI_DEV_CREATE, 0)?;
        Ok(Mouse { f })
    }

    pub fn left(&mut self, down: bool) -> io::Result<()> {
        let ev = |ty, code, value| Event { sec: 0, usec: 0, ty, code, value };
        let evs = [ev(EV_KEY, BTN_LEFT as u16, down as i32), ev(EV_SYN, 0, 0)];
        let b = unsafe { std::slice::from_raw_parts(evs.as_ptr() as *const u8, size_of_val(&evs)) };
        self.f.write_all(b)
    }

    pub fn rel(&mut self, dx: i32, dy: i32) -> io::Result<()> {
        if dx == 0 && dy == 0 {
            return Ok(());
        }
        let ev = |ty, code, value| Event { sec: 0, usec: 0, ty, code, value };
        let evs = [ev(EV_REL, REL_X, dx), ev(EV_REL, REL_Y, dy), ev(EV_SYN, 0, 0)];
        let b = unsafe { std::slice::from_raw_parts(evs.as_ptr() as *const u8, size_of_val(&evs)) };
        self.f.write_all(b)
    }
}

impl Drop for Mouse {
    fn drop(&mut self) {
        unsafe { ioctl(self.f.as_raw_fd(), UI_DEV_DESTROY) };
    }
}
