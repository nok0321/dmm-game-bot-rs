use image::RgbaImage;

use crate::config::CaptureMethod;
use crate::error::{BotError, Result};
use crate::platform::window::GameWindow;

pub trait Capturer {
    fn capture(&self, window: &GameWindow) -> Result<RgbaImage>;
}

#[derive(Debug, Default)]
pub struct PrintWindowCapturer;

#[derive(Debug, Default)]
pub struct BitBltCapturer;

pub fn build_capturer(method: CaptureMethod) -> Box<dyn Capturer + Send + Sync> {
    match method {
        CaptureMethod::PrintWindow => Box::new(PrintWindowCapturer),
        CaptureMethod::Bitblt => Box::new(BitBltCapturer),
    }
}

#[cfg(windows)]
mod imp {
    use super::*;

    use std::ptr;

    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
        DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ, SRCCOPY,
    };
    use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};

    /// PrintWindow に渡す PW_RENDERFULLCONTENT (0x02)。Chrome 等の GPU 描画ウィンドウ用。
    const PW_RENDERFULLCONTENT: u32 = 0x00000002;

    fn capture_with_bitblt_from_screen(
        client_screen_x: i32,
        client_screen_y: i32,
        width: u32,
        height: u32,
    ) -> Result<RgbaImage> {
        if width == 0 || height == 0 {
            return Err(BotError::CaptureFailed(
                "client area has zero size".to_string(),
            ));
        }

        unsafe {
            let screen_dc = GetDC(HWND(ptr::null_mut()));
            if screen_dc.is_invalid() {
                return Err(BotError::CaptureFailed("GetDC(NULL) failed".to_string()));
            }

            let result = (|| -> Result<RgbaImage> {
                let mem_dc = CreateCompatibleDC(screen_dc);
                if mem_dc.is_invalid() {
                    return Err(BotError::CaptureFailed(
                        "CreateCompatibleDC failed".to_string(),
                    ));
                }
                let bitmap = CreateCompatibleBitmap(screen_dc, width as i32, height as i32);
                if bitmap.is_invalid() {
                    let _ = DeleteDC(mem_dc);
                    return Err(BotError::CaptureFailed(
                        "CreateCompatibleBitmap failed".to_string(),
                    ));
                }

                let bitmap_obj = HGDIOBJ(bitmap.0);
                let _old = SelectObject(mem_dc, bitmap_obj);

                let blt = BitBlt(
                    mem_dc,
                    0,
                    0,
                    width as i32,
                    height as i32,
                    screen_dc,
                    client_screen_x,
                    client_screen_y,
                    SRCCOPY,
                );

                let img = match blt {
                    Ok(()) => extract_pixels(mem_dc, bitmap, width, height),
                    Err(e) => Err(BotError::CaptureFailed(format!("BitBlt: {}", e))),
                };

                let _ = DeleteObject(bitmap_obj);
                let _ = DeleteDC(mem_dc);
                img
            })();

            let _ = ReleaseDC(HWND(ptr::null_mut()), screen_dc);
            result
        }
    }

    fn capture_with_print_window(
        hwnd: HWND,
        width: u32,
        height: u32,
    ) -> Result<RgbaImage> {
        if width == 0 || height == 0 {
            return Err(BotError::CaptureFailed(
                "window area has zero size".to_string(),
            ));
        }

        unsafe {
            let win_dc = GetDC(hwnd);
            if win_dc.is_invalid() {
                return Err(BotError::CaptureFailed("GetDC(hwnd) failed".to_string()));
            }

            let result = (|| -> Result<RgbaImage> {
                let mem_dc = CreateCompatibleDC(win_dc);
                if mem_dc.is_invalid() {
                    return Err(BotError::CaptureFailed(
                        "CreateCompatibleDC failed".to_string(),
                    ));
                }
                let bitmap = CreateCompatibleBitmap(win_dc, width as i32, height as i32);
                if bitmap.is_invalid() {
                    let _ = DeleteDC(mem_dc);
                    return Err(BotError::CaptureFailed(
                        "CreateCompatibleBitmap failed".to_string(),
                    ));
                }

                let bitmap_obj = HGDIOBJ(bitmap.0);
                let _old = SelectObject(mem_dc, bitmap_obj);

                let ok = PrintWindow(hwnd, mem_dc, PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT));
                let img = if ok.as_bool() {
                    extract_pixels(mem_dc, bitmap, width, height)
                } else {
                    Err(BotError::CaptureFailed("PrintWindow returned 0".to_string()))
                };

                let _ = DeleteObject(bitmap_obj);
                let _ = DeleteDC(mem_dc);
                img
            })();

            let _ = ReleaseDC(hwnd, win_dc);
            result
        }
    }

    unsafe fn extract_pixels(
        mem_dc: HDC,
        bitmap: HBITMAP,
        width: u32,
        height: u32,
    ) -> Result<RgbaImage> {
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            ..Default::default()
        };

        let stride = (width as usize) * 4;
        let mut buf = vec![0u8; stride * (height as usize)];

        let scanned = unsafe {
            GetDIBits(
                mem_dc,
                bitmap,
                0,
                height,
                Some(buf.as_mut_ptr() as *mut _),
                &mut bmi,
                DIB_RGB_COLORS,
            )
        };

        if scanned == 0 {
            return Err(BotError::CaptureFailed("GetDIBits returned 0".to_string()));
        }

        // BGRA → RGBA
        for px in buf.chunks_exact_mut(4) {
            px.swap(0, 2);
        }

        RgbaImage::from_raw(width, height, buf)
            .ok_or_else(|| BotError::CaptureFailed("RgbaImage::from_raw failed".to_string()))
    }

    impl Capturer for BitBltCapturer {
        fn capture(&self, window: &GameWindow) -> Result<RgbaImage> {
            let rect = window.client_rect()?;
            capture_with_bitblt_from_screen(
                rect.screen_x,
                rect.screen_y,
                rect.width,
                rect.height,
            )
        }
    }

    impl Capturer for PrintWindowCapturer {
        fn capture(&self, window: &GameWindow) -> Result<RgbaImage> {
            // PrintWindow はウィンドウ全体を描画するため、ウィンドウ全体を撮ってから
            // クライアント領域でクロップする。失敗時は BitBlt にフォールバック。
            use windows::Win32::Foundation::RECT;
            use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

            let hwnd = window.raw();

            let mut win_rect = RECT::default();
            unsafe {
                GetWindowRect(hwnd, &mut win_rect)
                    .map_err(|e| BotError::CaptureFailed(format!("GetWindowRect: {}", e)))?;
            }
            let win_w = (win_rect.right - win_rect.left).max(0) as u32;
            let win_h = (win_rect.bottom - win_rect.top).max(0) as u32;

            let client = window.client_rect()?;

            match capture_with_print_window(hwnd, win_w, win_h) {
                Ok(full) => {
                    let off_x = (client.screen_x - win_rect.left).max(0) as u32;
                    let off_y = (client.screen_y - win_rect.top).max(0) as u32;
                    let cropped = image::imageops::crop_imm(
                        &full,
                        off_x,
                        off_y,
                        client.width,
                        client.height,
                    )
                    .to_image();
                    Ok(cropped)
                }
                Err(_) => capture_with_bitblt_from_screen(
                    client.screen_x,
                    client.screen_y,
                    client.width,
                    client.height,
                ),
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    impl Capturer for BitBltCapturer {
        fn capture(&self, _window: &GameWindow) -> Result<RgbaImage> {
            Err(BotError::CaptureFailed(
                "non-Windows: capture not supported".into(),
            ))
        }
    }
    impl Capturer for PrintWindowCapturer {
        fn capture(&self, _window: &GameWindow) -> Result<RgbaImage> {
            Err(BotError::CaptureFailed(
                "non-Windows: capture not supported".into(),
            ))
        }
    }
}
