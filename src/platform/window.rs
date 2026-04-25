use crate::error::{BotError, Result};

#[derive(Debug, Clone, Copy)]
pub struct WindowRect {
    pub screen_x: i32,
    pub screen_y: i32,
    pub width: u32,
    pub height: u32,
}

#[cfg(windows)]
pub use windows_impl::GameWindow;

#[cfg(not(windows))]
pub use stub_impl::GameWindow;

#[cfg(windows)]
mod windows_impl {
    use super::*;

    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, POINT, RECT};
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClientRect, GetWindowTextLengthW, GetWindowTextW, IsWindowVisible,
        SetForegroundWindow,
    };

    #[derive(Debug, Clone, Copy)]
    pub struct GameWindow {
        pub(crate) hwnd: HWND,
    }

    struct EnumState<'a> {
        pattern: &'a str,
        found: Option<HWND>,
    }

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let state_ptr = lparam.0 as *mut EnumState;
        if state_ptr.is_null() {
            return BOOL(0);
        }
        let state = unsafe { &mut *state_ptr };

        unsafe {
            if !IsWindowVisible(hwnd).as_bool() {
                return BOOL(1);
            }
            let len = GetWindowTextLengthW(hwnd);
            if len <= 0 {
                return BOOL(1);
            }
            let mut buf = vec![0u16; (len + 1) as usize];
            let n = GetWindowTextW(hwnd, &mut buf);
            if n <= 0 {
                return BOOL(1);
            }
            let title = String::from_utf16_lossy(&buf[..n as usize]);
            if title.contains(state.pattern) {
                state.found = Some(hwnd);
                return BOOL(0);
            }
        }
        BOOL(1)
    }

    impl GameWindow {
        pub fn find_by_title_substring(pattern: &str) -> Result<Self> {
            let mut state = EnumState {
                pattern,
                found: None,
            };
            unsafe {
                let _ = EnumWindows(
                    Some(enum_proc),
                    LPARAM(&mut state as *mut _ as isize),
                );
            }
            match state.found {
                Some(hwnd) => Ok(Self { hwnd }),
                None => Err(BotError::WindowNotFound(pattern.to_string())),
            }
        }

        pub fn client_rect(&self) -> Result<WindowRect> {
            let mut rect = RECT::default();
            unsafe {
                GetClientRect(self.hwnd, &mut rect)
                    .map_err(|e| BotError::other(format!("GetClientRect: {}", e)))?;
            }
            let mut origin = POINT { x: 0, y: 0 };
            unsafe {
                let ok = ClientToScreen(self.hwnd, &mut origin);
                if !ok.as_bool() {
                    return Err(BotError::other("ClientToScreen failed"));
                }
            }
            let width = (rect.right - rect.left).max(0) as u32;
            let height = (rect.bottom - rect.top).max(0) as u32;
            Ok(WindowRect {
                screen_x: origin.x,
                screen_y: origin.y,
                width,
                height,
            })
        }

        pub fn focus(&self) -> Result<()> {
            unsafe {
                let _ = SetForegroundWindow(self.hwnd);
            }
            Ok(())
        }

        #[allow(dead_code)]
        pub(crate) fn raw(&self) -> HWND {
            self.hwnd
        }
    }
}

#[cfg(not(windows))]
mod stub_impl {
    use super::*;

    #[derive(Debug, Clone, Copy)]
    pub struct GameWindow;

    impl GameWindow {
        pub fn find_by_title_substring(_pattern: &str) -> Result<Self> {
            Err(BotError::other("non-Windows: GameWindow not supported"))
        }
        pub fn client_rect(&self) -> Result<WindowRect> {
            Err(BotError::other("non-Windows: GameWindow not supported"))
        }
        pub fn focus(&self) -> Result<()> {
            Err(BotError::other("non-Windows: GameWindow not supported"))
        }
    }
}
