use std::thread::sleep;
use std::time::Duration;

use crate::error::{BotError, Result};

pub trait InputSender: Send + Sync {
    fn click_at(&self, screen_x: i32, screen_y: i32, press_duration_ms: u64) -> Result<()>;
}

pub struct DryRunSender;

impl InputSender for DryRunSender {
    fn click_at(&self, screen_x: i32, screen_y: i32, _press_duration_ms: u64) -> Result<()> {
        tracing::warn!(
            "[DRY-RUN] click suppressed at screen ({}, {}) — pass --live to actually send",
            screen_x,
            screen_y
        );
        Ok(())
    }
}

#[cfg(windows)]
pub use windows_impl::SendInputSender;

#[cfg(not(windows))]
pub use stub_impl::SendInputSender;

#[cfg(windows)]
mod windows_impl {
    use super::*;

    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
        MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK, MOUSEINPUT,
        MOUSE_EVENT_FLAGS,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };

    pub struct SendInputSender;

    impl SendInputSender {
        pub fn new() -> Self {
            Self
        }
    }

    impl Default for SendInputSender {
        fn default() -> Self {
            Self::new()
        }
    }

    fn normalize(screen_x: i32, screen_y: i32) -> (i32, i32) {
        let (vx, vy, vw, vh) = unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
            )
        };
        let vw = vw.max(1);
        let vh = vh.max(1);
        let nx = ((screen_x - vx) as i64) * 65535 / (vw - 1).max(1) as i64;
        let ny = ((screen_y - vy) as i64) * 65535 / (vh - 1).max(1) as i64;
        (nx as i32, ny as i32)
    }

    fn make_mouse_input(dx: i32, dy: i32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
        INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    fn send(inputs: &[INPUT]) -> Result<()> {
        let cb = std::mem::size_of::<INPUT>() as i32;
        let sent = unsafe { SendInput(inputs, cb) };
        if sent as usize != inputs.len() {
            return Err(BotError::InputFailed(format!(
                "SendInput sent {} of {} events",
                sent,
                inputs.len()
            )));
        }
        Ok(())
    }

    impl InputSender for SendInputSender {
        fn click_at(
            &self,
            screen_x: i32,
            screen_y: i32,
            press_duration_ms: u64,
        ) -> Result<()> {
            let (nx, ny) = normalize(screen_x, screen_y);
            let move_flags = MOUSE_EVENT_FLAGS(
                MOUSEEVENTF_MOVE.0 | MOUSEEVENTF_ABSOLUTE.0 | MOUSEEVENTF_VIRTUALDESK.0,
            );
            send(&[make_mouse_input(nx, ny, move_flags)])?;
            sleep(Duration::from_millis(20));
            send(&[make_mouse_input(0, 0, MOUSEEVENTF_LEFTDOWN)])?;
            sleep(Duration::from_millis(press_duration_ms));
            send(&[make_mouse_input(0, 0, MOUSEEVENTF_LEFTUP)])?;
            Ok(())
        }
    }
}

#[cfg(not(windows))]
mod stub_impl {
    use super::*;

    pub struct SendInputSender;
    impl SendInputSender {
        pub fn new() -> Self {
            Self
        }
    }
    impl Default for SendInputSender {
        fn default() -> Self {
            Self::new()
        }
    }
    impl InputSender for SendInputSender {
        fn click_at(&self, _x: i32, _y: i32, _ms: u64) -> Result<()> {
            Err(BotError::InputFailed("non-Windows: SendInput unavailable".into()))
        }
    }
}
