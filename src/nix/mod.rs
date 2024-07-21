///
/// This module contains the mouse action functions
/// for the unix-like systems
///
use crate::common::{CallbackId, MouseActions, MouseButton, MouseEvent, ScrollDirection};
use crate::nix::uinput::{
    InputEvent, TimeVal, BTN_LEFT, BTN_MIDDLE, BTN_RIGHT, EV_KEY, EV_REL, REL_HWHEEL, REL_WHEEL,
    REL_X, REL_Y,
};
use glob::glob;
use std::{
    collections::HashMap,
    fs::File,
    io::{self, Result},
    mem::size_of,
    os::unix::io::AsRawFd,
    sync::{mpsc, Arc, Mutex},
    thread,
};
#[cfg(feature = "x11")]
use std::{process::Command, str::from_utf8};
#[cfg(feature = "x11")]
mod x11;

mod uinput;

type Callbacks = Arc<Mutex<HashMap<CallbackId, Box<dyn Fn(&MouseEvent) + Send>>>>;

pub use uinput::UInputMouseManager;
pub use x11::X11MouseManager;

pub struct NixMouseManager {}

impl NixMouseManager {
    /// rng_x and rng_y is used by uinput mouse.
    /// As for x11, the params can be (0, 0), (0, 0)
    #[allow(clippy::new_ret_no_self)]
    pub fn new(rng_x: (i32, i32), rng_y: (i32, i32)) -> Result<Box<dyn MouseActions>> {
        #[cfg(feature = "x11")]
        {
            if is_x11() {
                Ok(Box::new(x11::X11MouseManager::new()))
            } else {
                Ok(Box::new(uinput::UInputMouseManager::new(rng_x, rng_y)?))
            }
        }
        #[cfg(not(feature = "x11"))]
        {
            // If x11 feature is disabled, just return uinput mouse manager
            return Ok(Box::new(uinput::UInputMouseManager::new(rng_x, rng_y)?));
        }
    }

    pub fn new_x11() -> X11MouseManager {
        x11::X11MouseManager::new()
    }

    pub fn new_uinput(rng_x: (i32, i32), rng_y: (i32, i32)) -> Result<UInputMouseManager> {
        Ok(uinput::UInputMouseManager::new(rng_x, rng_y)?)
    }
}

/// Start the event listener for nix systems
fn start_nix_listener(callbacks: &Callbacks) -> Result<()> {
    let (tx, rx) = mpsc::channel();

    let mut previous_paths = vec![];
    // Read all the mouse events listed under /dev/input/by-id and
    // /dev/input/by-path. These directories are collections of symlinks
    // to /dev/input/event*
    //
    // I am only interested in the ones that end with `-event-mouse`
    for file in glob("/dev/input/by-id/*-event-mouse")
        .map_err(|e| {
            io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to read by-id glob pattern, {}", e),
            )
        })?
        .chain(glob("/dev/input/by-path/*-event-mouse").map_err(|e| {
            io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to read by-path glob pattern, {}", e),
            )
        })?)
    {
        let mut file = file.map_err(|e| {
            io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed because of an IO error, {}", e),
            )
        })?;

        // Get the link if it exists
        if let Ok(rel_path) = file.read_link() {
            if rel_path.is_absolute() {
                file = rel_path;
            } else {
                // Remove the file name from the path buffer, leaving us with path to directory
                file.pop();
                // Push the relative path of the link (e.g. `../event8`)
                file.push(rel_path);
                // Get the absolute path to final path
                file = std::fs::canonicalize(file)?;
            }
        }

        let path = file.display().to_string();

        if previous_paths.contains(&path) {
            continue;
        }

        previous_paths.push(path.clone());

        let event = File::options().read(true).open(path)?;

        // Create a thread for this mouse-event file
        let tx = tx.clone();
        thread::spawn(move || loop {
            let mut buffer = InputEvent {
                time: TimeVal {
                    tv_sec: 0,
                    tv_usec: 0,
                },
                r#type: 0,
                code: 0,
                value: 0,
            };
            unsafe {
                read(event.as_raw_fd(), &mut buffer, size_of::<InputEvent>());
            }
            tx.send(buffer).ok();
        });
    }

    let callbacks = callbacks.clone();
    // Create a thread for handling the callbacks
    thread::spawn(move || {
        for received in rx {
            // Construct the library's MouseEvent
            let r#type = received.r#type as i32;
            let code = received.code as i32;
            let val = received.value as i32;

            let mouse_event = if r#type == EV_KEY {
                let button = if code == BTN_LEFT {
                    MouseButton::Left
                } else if code == BTN_RIGHT {
                    MouseButton::Right
                } else if code == BTN_MIDDLE {
                    MouseButton::Middle
                } else {
                    // Ignore the unknown mouse buttons
                    continue;
                };

                if received.value == 1 {
                    MouseEvent::Press(button)
                } else {
                    MouseEvent::Release(button)
                }
            } else if r#type == EV_REL {
                let code = received.code as u32;
                if code == REL_WHEEL {
                    MouseEvent::Scroll(if received.value > 0 {
                        ScrollDirection::Up
                    } else {
                        ScrollDirection::Down
                    })
                } else if code == REL_HWHEEL {
                    MouseEvent::Scroll(if received.value > 0 {
                        ScrollDirection::Right
                    } else {
                        ScrollDirection::Left
                    })
                } else if code == REL_X {
                    MouseEvent::RelativeMove(val, 0)
                } else if code == REL_Y {
                    MouseEvent::RelativeMove(0, val)
                } else {
                    continue;
                }
            } else {
                // Ignore other unknown events
                continue;
            };

            // Invoke all given callbacks with the constructed mouse event
            for callback in callbacks.lock().unwrap().values() {
                callback(&mouse_event);
            }
        }
    });

    Ok(())
}

#[cfg(feature = "x11")]
fn is_x11() -> bool {
    // Try to verify x11 using loginctl
    let loginctl_output = Command::new("sh")
        .arg("-c")
        .arg("loginctl show-session $(loginctl | awk '/tty/ {print $1}') -p Type --value")
        .output();

    if let Ok(out) = loginctl_output {
        if let Ok(typ) = from_utf8(&out.stdout) {
            if typ.trim().to_lowercase() == "x11" {
                return true;
            }
        }
    }

    // If loginctl fails try to read the environment variable $XDG_SESSION_TYPE
    if let Ok(session_type) = std::env::var("XDG_SESSION_TYPE") {
        if session_type.trim().to_lowercase() == "x11" {
            return true;
        }
    }

    false
}

extern "C" {
    fn read(fd: i32, buf: *mut InputEvent, count: usize) -> i32;
}
