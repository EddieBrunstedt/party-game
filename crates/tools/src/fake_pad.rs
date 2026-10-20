//! A virtual gamepad through Linux uinput, to script a pad in windowed runs: press buttons on
//! a schedule, and unplug and replug it to check controller loss. Every plug creates the device
//! with the same ids, so gilrs (and Bevy) see the same gamepad come back.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};

/// One scripted moment, `secs` after the tool starts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Step {
    Plug,
    Unplug,
    Press(u16),
}

/// The buttons a script can press, by the names the game uses.
const BUTTONS: [(&str, u16); 5] = [
    ("A", BTN_SOUTH),
    ("B", BTN_EAST),
    ("X", BTN_WEST),
    ("Y", BTN_NORTH),
    ("Start", BTN_START),
];

/// How long a press holds its button down: several ticks, so a 60 Hz game can't miss it.
const HOLD: Duration = Duration::from_millis(100);

const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const EV_ABS: u16 = 0x03;
const SYN_REPORT: u16 = 0;
const BTN_SOUTH: u16 = 0x130;
const BTN_EAST: u16 = 0x131;
const BTN_NORTH: u16 = 0x133;
const BTN_WEST: u16 = 0x134;
const BTN_START: u16 = 0x13b;
const ABS_X: u16 = 0x00;
const ABS_Y: u16 = 0x01;
const BUS_USB: u16 = 0x03;

// From <linux/uinput.h>: _IO('U', n) and _IOW('U', n, size).
const fn iow(nr: u64, size: u64) -> u64 {
    (1 << 30) | (size << 16) | ((b'U' as u64) << 8) | nr
}
const UI_DEV_CREATE: u64 = (b'U' as u64) << 8 | 1;
const UI_DEV_DESTROY: u64 = (b'U' as u64) << 8 | 2;
const UI_DEV_SETUP: u64 = iow(3, size_of::<UinputSetup>() as u64);
const UI_ABS_SETUP: u64 = iow(4, size_of::<UinputAbsSetup>() as u64);
const UI_SET_EVBIT: u64 = iow(100, size_of::<libc::c_int>() as u64);
const UI_SET_KEYBIT: u64 = iow(101, size_of::<libc::c_int>() as u64);
const UI_SET_ABSBIT: u64 = iow(103, size_of::<libc::c_int>() as u64);

#[repr(C)]
struct InputId {
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
}

#[repr(C)]
struct UinputSetup {
    id: InputId,
    name: [u8; 80],
    ff_effects_max: u32,
}

#[repr(C)]
struct InputAbsinfo {
    value: i32,
    minimum: i32,
    maximum: i32,
    fuzz: i32,
    flat: i32,
    resolution: i32,
}

#[repr(C)]
struct UinputAbsSetup {
    code: u16,
    absinfo: InputAbsinfo,
}

#[repr(C)]
struct InputEvent {
    time: libc::timeval,
    kind: u16,
    code: u16,
    value: i32,
}

/// Parses `SECS:WHAT`, where WHAT is `plug`, `unplug` or a button (A, B, X, Y, Start).
pub fn parse_step(text: &str) -> Result<(f32, Step), String> {
    let (secs, what) = text
        .split_once(':')
        .ok_or_else(|| format!("expected SECS:WHAT, got {text:?}"))?;
    let secs: f32 = secs
        .parse()
        .map_err(|_| format!("bad seconds in {text:?}"))?;
    let step = match what {
        "plug" => Step::Plug,
        "unplug" => Step::Unplug,
        button => BUTTONS
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(button))
            .map(|&(_, code)| Step::Press(code))
            .ok_or_else(|| format!("unknown step {button:?} (plug, unplug, A, B, X, Y, Start)"))?,
    };
    Ok((secs, step))
}

/// Plugs the pad in, plays `steps` (in time order), and unplugs it `until` seconds in.
pub fn run(mut steps: Vec<(f32, Step)>, until: f32) -> Result<(), String> {
    steps.sort_by(|a, b| a.0.total_cmp(&b.0));
    let start = Instant::now();
    let at = |secs: f32| start + Duration::from_secs_f32(secs);
    let mut pad = Some(VirtualPad::plug()?);
    println!("0.0 s: plugged in");
    for (secs, step) in steps {
        wait_until(at(secs));
        match (step, &mut pad) {
            (Step::Plug, None) => pad = Some(VirtualPad::plug()?),
            (Step::Unplug, Some(_)) => pad = None,
            (Step::Press(code), Some(device)) => {
                device.button(code, true)?;
                std::thread::sleep(HOLD);
                device.button(code, false)?;
            }
            (Step::Press(_), None) => return Err(format!("{secs} s: pressing an unplugged pad")),
            (Step::Plug | Step::Unplug, _) => {}
        }
        println!("{secs:.1} s: {step:?}");
    }
    wait_until(at(until));
    Ok(())
}

fn wait_until(when: Instant) {
    std::thread::sleep(when.saturating_duration_since(Instant::now()));
}

/// A uinput device that exists while this lives.
struct VirtualPad(File);

impl VirtualPad {
    fn plug() -> Result<Self, String> {
        let file = OpenOptions::new()
            .write(true)
            .open("/dev/uinput")
            .map_err(|e| format!("can't open /dev/uinput ({e}); is this user in `input`?"))?;
        let pad = Self(file);
        pad.ioctl(UI_SET_EVBIT, EV_KEY as libc::c_ulong)?;
        for (_, code) in BUTTONS {
            pad.ioctl(UI_SET_KEYBIT, code as libc::c_ulong)?;
        }
        pad.ioctl(UI_SET_EVBIT, EV_ABS as libc::c_ulong)?;
        for code in [ABS_X, ABS_Y] {
            pad.ioctl(UI_SET_ABSBIT, code as libc::c_ulong)?;
            let abs = UinputAbsSetup {
                code,
                absinfo: InputAbsinfo {
                    value: 0,
                    minimum: -32768,
                    maximum: 32767,
                    fuzz: 16,
                    flat: 128,
                    resolution: 0,
                },
            };
            pad.ioctl(UI_ABS_SETUP, &raw const abs as libc::c_ulong)?;
        }
        let mut setup = UinputSetup {
            id: InputId {
                bustype: BUS_USB,
                vendor: 0x1209,
                product: 0x0001,
                version: 1,
            },
            name: [0; 80],
            ff_effects_max: 0,
        };
        let name = b"Virtual Test Pad";
        setup.name[..name.len()].copy_from_slice(name);
        pad.ioctl(UI_DEV_SETUP, &raw const setup as libc::c_ulong)?;
        pad.ioctl(UI_DEV_CREATE, 0)?;
        Ok(pad)
    }

    fn ioctl(&self, request: u64, arg: libc::c_ulong) -> Result<(), String> {
        // SAFETY: `request` is a uinput ioctl and `arg` is the value or pointer it expects;
        // pointers are to locals that outlive the call.
        let result = unsafe { libc::ioctl(self.0.as_raw_fd(), request as _, arg) };
        if result < 0 {
            return Err(format!(
                "uinput ioctl {request:#x} failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(())
    }

    fn button(&mut self, code: u16, down: bool) -> Result<(), String> {
        self.event(EV_KEY, code, down as i32)?;
        self.event(EV_SYN, SYN_REPORT, 0)
    }

    fn event(&mut self, kind: u16, code: u16, value: i32) -> Result<(), String> {
        let event = InputEvent {
            time: libc::timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
            kind,
            code,
            value,
        };
        // SAFETY: `InputEvent` is plain `repr(C)` data with no padding on Linux.
        let bytes = unsafe {
            std::slice::from_raw_parts((&raw const event).cast::<u8>(), size_of::<InputEvent>())
        };
        self.0
            .write_all(bytes)
            .map_err(|e| format!("writing to uinput: {e}"))
    }
}

impl Drop for VirtualPad {
    fn drop(&mut self) {
        // Closing the file destroys the device too; this just makes it explicit.
        let _ = self.ioctl(UI_DEV_DESTROY, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ioctl_numbers_match_the_kernel_headers() {
        assert_eq!(UI_DEV_CREATE, 0x5501);
        assert_eq!(UI_DEV_DESTROY, 0x5502);
        assert_eq!(UI_DEV_SETUP, 0x405c_5503);
        assert_eq!(UI_ABS_SETUP, 0x401c_5504);
        assert_eq!(UI_SET_EVBIT, 0x4004_5564);
        assert_eq!(UI_SET_KEYBIT, 0x4004_5565);
        assert_eq!(UI_SET_ABSBIT, 0x4004_5567);
    }

    #[test]
    fn steps_parse() {
        assert_eq!(parse_step("2.5:start"), Ok((2.5, Step::Press(BTN_START))));
        assert_eq!(parse_step("10:unplug"), Ok((10.0, Step::Unplug)));
        assert!(parse_step("1:Z").is_err());
        assert!(parse_step("plug").is_err());
    }
}
