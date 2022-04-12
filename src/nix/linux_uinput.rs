///
/// This module contains the mouse action functions
/// for the linux systems that uses uinput
///
use crate::common::{MouseActions, MouseButton, ScrollDirection};
use std::ffi::CString;
use std::fs::File;
use std::mem::size_of;
use std::os::raw::{c_int, c_uint, c_ulong, c_ushort};
use std::os::unix::prelude::AsRawFd;
use std::thread;
use std::time::Duration;

pub struct LinuxUInputMouseManager {
    uinput_file: File,
}

impl LinuxUInputMouseManager {
    pub fn new() -> Self {
        let manager = LinuxUInputMouseManager {
            uinput_file: File::options()
                .write(true)
                .open("/dev/uinput")
                .expect("uinput file can not be opened"),
        };
        let fd = manager.uinput_file.as_raw_fd();
        unsafe {
            // For press events (also needed for mouse movement)
            ioctl(fd, UI_SET_EVBIT, EV_KEY);
            ioctl(fd, UI_SET_KEYBIT, BTN_LEFT);

            // For mouse movement
            ioctl(fd, UI_SET_EVBIT, EV_REL);
            ioctl(fd, UI_SET_RELBIT, REL_X);
            ioctl(fd, UI_SET_RELBIT, REL_Y);
        }

        let usetup = UInputSetup {
            id: InputId {
                bustype: BUS_USB,
                // Random vendor and product
                vendor: 0x2222,
                product: 0x3333,
                version: 0,
            },
            name: CString::new("mice-library-fake-mouse").unwrap(),
            ff_effects_max: 0,
        };

        unsafe {
            ioctl(fd, UI_DEV_SETUP, &usetup);
            ioctl(fd, UI_DEV_CREATE);
        }

        // On UI_DEV_CREATE the kernel will create the device node for this
        // device. We are inserting a pause here so that userspace has time
        // to detect, initialize the new device, and can start listening to
        // the event, otherwise it will not notice the event we are about to send.
        thread::sleep(Duration::from_millis(300));

        manager
    }

    fn emit(&self, r#type: c_int, code: c_int, value: c_int) {
        let mut event = InputEvent {
            time: TimeVal {
                tv_sec: 0,
                tv_usec: 0,
            },
            r#type: r#type as u16,
            code: code as u16,
            value,
        };
        let fd = self.uinput_file.as_raw_fd();

        unsafe {
            write(fd, &mut event, size_of::<InputEvent>());
        }
    }

    /// Syncronize the device
    fn syncronize(&self) {
        self.emit(EV_SYN, SYN_REPORT, 0);
    }

    fn move_relative(&self, x: i32, y: i32) {
        // uinput does not move the mouse in pixels but uses `units`. I couldn't
        // find information regarding to this uinput `unit`, but according to
        // my findings 1 unit corresponds to exactly 2 pixels.
        //
        // To achieve the expected behavior; divide the parameters by 2
        //
        // This seems like there is a bug in this crate, but the 
        // behavior is the same on other projects that make use of
        // uinput. e.g. `ydotool`. When you try to move your mouse,
        // it will move 2x further pixels
        self.emit(EV_REL, REL_X as i32, x / 2);
        self.emit(EV_REL, REL_Y as i32, y / 2);
        self.syncronize();
    }
}

impl Drop for LinuxUInputMouseManager {
    fn drop(&mut self) {
        let fd = self.uinput_file.as_raw_fd();
        unsafe {
            // Destroy the device, the file is closed automatically by the File module
            ioctl(fd, UI_DEV_DESTROY as u64);
        }
    }
}

impl MouseActions for LinuxUInputMouseManager {
    fn move_to(&self, x: usize, y: usize) {
        // For some reason absolute mouse move events are not working on uinput
        //
        // As a work around solution; first set the mouse to top left, then
        // call relative move function to simulate an absolute move event
        self.move_relative(i32::min_value(), i32::min_value());
        self.move_relative(x as i32, y as i32);
    }

    fn get_position(&self) -> (i32, i32) {
        unimplemented!();
    }

    fn press_button(&self, button: &MouseButton) {
        unimplemented!();
    }

    fn release_button(&self, button: &MouseButton) {
        unimplemented!();
    }

    fn click_button(&self, button: &MouseButton) {
        unimplemented!();
    }

    fn scroll_wheel(&self, direction: &ScrollDirection) {
        unimplemented!();
    }
}

/// ioctl and uinput definitions
const UI_SET_EVBIT: c_ulong = 1074025828;
const UI_SET_KEYBIT: c_ulong = 1074025829;
const UI_SET_RELBIT: c_ulong = 1074025830;
const UI_DEV_SETUP: c_ulong = 1079792899;
const UI_DEV_CREATE: c_ulong = 21761;
const UI_DEV_DESTROY: c_uint = 21762;

const SYN_REPORT: c_int = 0x00;
const EV_KEY: c_int = 0x01;
const EV_SYN: c_int = 0x00;
const EV_REL: c_int = 0x02;
const REL_X: c_uint = 0x00;
const REL_Y: c_uint = 0x01;
const BTN_LEFT: c_int = 0x110;
const BUS_USB: c_ushort = 0x03;

/// uinput types
#[repr(C)]
struct UInputSetup {
    id: InputId,
    name: CString,
    ff_effects_max: c_ulong,
}

#[repr(C)]
struct InputId {
    bustype: c_ushort,
    vendor: c_ushort,
    product: c_ushort,
    version: c_ushort,
}

#[repr(C)]
struct InputEvent {
    time: TimeVal,
    r#type: u16,
    code: u16,
    value: c_int,
}

#[repr(C)]
struct TimeVal {
    tv_sec: c_ulong,
    tv_usec: c_ulong,
}

extern "C" {
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
    fn write(fd: c_int, buf: *mut InputEvent, count: usize) -> usize;
}

#[cfg(test)]
mod tests {
    use super::LinuxUInputMouseManager;
    use crate::common::MouseActions;

    #[test]
    fn uinput_move() {
        let manager = LinuxUInputMouseManager::new();

        manager.move_to(1920 / 2, 1080 / 2);
    }
}