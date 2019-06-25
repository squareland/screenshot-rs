//! Capture a bitmap image of a display. The resulting screenshot is stored in
//! the `Screenshot` type, which varies per platform.
//!
//! # Platform-specific details
//!
//! Despite OS X's CoreGraphics documentation, the bitmap returned has its
//! origin at the top left corner. It uses ARGB pixels.
//!
//! The Windows GDI bitmap has its coordinate origin at the bottom left. We
//! attempt to undo this by reordering the rows. Windows also uses ARGB pixels.

#![allow(unused_assignments)]

extern crate libc;

#[cfg(target_os = "windows")]
extern crate winapi;

pub use ffi::get_screenshot;
use std::mem::size_of;

#[derive(Clone, Copy)]
pub struct Pixel {
    pub a: u8,
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// An image buffer containing the screenshot.
/// Pixels are stored as [ARGB](https://en.wikipedia.org/wiki/ARGB).
pub struct Screenshot {
    data: Vec<u8>,
    height: usize,
    width: usize,
    row_len: usize,
    // Might be superfluous
    pixel_width: usize,
}

impl Screenshot {
    /// Height of image in pixels.
    #[inline]
    pub fn height(&self) -> usize {
        self.height
    }

    /// Width of image in pixels.
    #[inline]
    pub fn width(&self) -> usize {
        self.width
    }

    /// Number of bytes in one row of bitmap.
    #[inline]
    pub fn row_len(&self) -> usize {
        self.row_len
    }

    /// Width of pixel in bytes.
    #[inline]
    pub fn pixel_width(&self) -> usize {
        self.pixel_width
    }

    /// Raw bitmap.
    #[inline]
    pub unsafe fn raw_data(&self) -> *const u8 {
        &self.data[0] as *const u8
    }

    /// Raw bitmap.
    #[inline]
    pub unsafe fn raw_data_mut(&mut self) -> *mut u8 {
        &mut self.data[0] as *mut u8
    }

    /// Number of bytes in bitmap
    #[inline]
    pub fn raw_len(&self) -> usize {
        self.data.len() * size_of::<u8>()
    }

    /// Gets pixel at (row, col)
    pub fn get_pixel(&self, row: usize, col: usize) -> Pixel {
        let idx = row * self.row_len() + col * self.pixel_width();
        unsafe {
            //let data = &self.data[0] as *const u8;
            if idx > self.data.len() {
                panic!("Bounds overflow");
            }

            Pixel {
                a: *self.data.get_unchecked(idx + 3),
                r: *self.data.get_unchecked(idx + 2),
                g: *self.data.get_unchecked(idx + 1),
                b: *self.data.get_unchecked(idx),
            }
        }
    }
}

impl AsRef<[u8]> for Screenshot {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.data.as_slice()
    }
}

pub type ScreenResult = Result<Screenshot, &'static str>;

#[cfg(target_os = "linux")]
mod ffi {
    extern crate xlib;

    use self::xlib::{
        XAllPlanes, XCloseDisplay, XDestroyWindow, XGetImage, XGetWindowAttributes, XImage,
        XOpenDisplay, XRootWindowOfScreen, XScreenOfDisplay, XWindowAttributes, ZPixmap,
    };
    use libc::{c_int, c_uint};
    use std::mem;
    use std::ptr::null_mut;
    use std::slice;
    use {ScreenResult, Screenshot};

    pub fn get_screenshot(screen: u32) -> ScreenResult {
        unsafe {
            let display = XOpenDisplay(null_mut());
            let screen = XScreenOfDisplay(display, screen as c_int);
            let root = XRootWindowOfScreen(screen);

            let mut attr: XWindowAttributes = mem::uninitialized();
            XGetWindowAttributes(display, root, &mut attr);

            let img = &mut *XGetImage(
                display,
                root,
                0,
                0,
                attr.width as c_uint,
                attr.height as c_uint,
                XAllPlanes(),
                ZPixmap,
            );
            XDestroyWindow(display, root);
            XCloseDisplay(display);
            // This is the function which XDestroyImage macro calls.
            // servo/rust-xlib doesn't handle function pointers correctly.
            // We have to transmute the variable.
            let destroy_image: extern "C" fn(*mut XImage) -> c_int =
                mem::transmute(img.f.destroy_image);
            let height = img.height as usize;
            let width = img.width as usize;
            let row_len = img.bytes_per_line as usize;
            let pixel_bits = img.bits_per_pixel as usize;
            if pixel_bits % 8 != 0 {
                destroy_image(&mut *img);
                return Err("Pixels aren't integral bytes.");
            }
            let pixel_width = pixel_bits / 8;

            // Create a Vec for image
            let size = width * height * pixel_width;
            let mut data = slice::from_raw_parts(img.data as *mut u8, size as usize).to_vec();
            destroy_image(&mut *img);

            // Fix Alpha channel when xlib cannot retrieve info correctly
            let has_alpha = data.iter().enumerate().any(|(n, x)| n % 4 == 3 && *x != 0);
            if !has_alpha {
                let mut n = 0;
                for channel in &mut data {
                    if n % 4 == 3 {
                        *channel = 255;
                    }
                    n += 1;
                }
            }

            Ok(Screenshot {
                data,
                height,
                width,
                row_len,
                pixel_width,
            })
        }
    }
}

#[cfg(target_os = "macos")]
mod ffi {
    #![allow(non_upper_case_globals, dead_code)]

    use libc;
    use std::slice;
    use ScreenResult;
    use Screenshot;

    type CFIndex = libc::c_long;
    type CFDataRef = *const u8; // *const CFData

    #[cfg(target_arch = "x86")]
    type CGFloat = libc::c_float;
    #[cfg(target_arch = "x86_64")]
    type CGFloat = libc::c_double;
    type CGError = libc::int32_t;

    type CGDirectDisplayID = libc::uint32_t;
    type CGDisplayCount = libc::uint32_t;
    type CGImageRef = *mut u8;
    // *mut CGImage
    type CGDataProviderRef = *mut u8; // *mut CGDataProvider

    const kCGErrorSuccess: CGError = 0;
    const kCGErrorFailure: CGError = 1000;
    const CGDisplayNoErr: CGError = kCGErrorSuccess;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGGetActiveDisplayList(
            max_displays: libc::uint32_t,
            active_displays: *mut CGDirectDisplayID,
            display_count: *mut CGDisplayCount,
        ) -> CGError;
        fn CGDisplayCreateImage(displayID: CGDirectDisplayID) -> CGImageRef;
        fn CGImageRelease(image: CGImageRef);

        fn CGImageGetBitsPerComponent(image: CGImageRef) -> libc::size_t;
        fn CGImageGetBitsPerPixel(image: CGImageRef) -> libc::size_t;
        fn CGImageGetBytesPerRow(image: CGImageRef) -> libc::size_t;
        fn CGImageGetDataProvider(image: CGImageRef) -> CGDataProviderRef;
        fn CGImageGetHeight(image: CGImageRef) -> libc::size_t;
        fn CGImageGetWidth(image: CGImageRef) -> libc::size_t;

        fn CGDataProviderCopyData(provider: CGDataProviderRef) -> CFDataRef;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFDataGetLength(theData: CFDataRef) -> CFIndex;
        fn CFDataGetBytePtr(theData: CFDataRef) -> *const u8;
        fn CFRelease(cf: *const libc::c_void);
    }

    /// Get a screenshot of the requested display.
    pub fn get_screenshot(screen: usize) -> ScreenResult {
        unsafe {
            // Get number of displays
            let mut count: CGDisplayCount = 0;
            let mut err = CGDisplayNoErr;
            err = CGGetActiveDisplayList(0, 0 as *mut CGDirectDisplayID, &mut count);
            if err != CGDisplayNoErr {
                return Err("Error getting number of displays.");
            }

            // Get list of displays
            let mut disps: Vec<CGDisplayCount> = Vec::with_capacity(count as usize);
            disps.set_len(count as usize);
            err = CGGetActiveDisplayList(
                disps.len() as libc::uint32_t,
                &mut disps[0] as *mut CGDirectDisplayID,
                &mut count,
            );
            if err != CGDisplayNoErr {
                return Err("Error getting list of displays.");
            }

            // Get screenshot of requested display
            let disp_id = disps[screen];
            let cg_img = CGDisplayCreateImage(disp_id);

            // Get info about image
            let width = CGImageGetWidth(cg_img) as usize;
            let height = CGImageGetHeight(cg_img) as usize;
            let row_len = CGImageGetBytesPerRow(cg_img) as usize;
            let pixel_bits = CGImageGetBitsPerPixel(cg_img) as usize;
            if pixel_bits % 8 != 0 {
                return Err("Pixels aren't integral bytes.");
            }

            // Copy image into a Vec buffer
            let cf_data = CGDataProviderCopyData(CGImageGetDataProvider(cg_img));
            let raw_len = CFDataGetLength(cf_data) as usize;

            let res = if width * height * pixel_bits != raw_len * 8 {
                Err("Image size is inconsistent with W*H*D.")
            } else {
                let data = slice::from_raw_parts(CFDataGetBytePtr(cf_data), raw_len).to_vec();
                Ok(Screenshot {
                    data,
                    height,
                    width,
                    row_len,
                    pixel_width: pixel_bits / 8,
                })
            };

            // Release native objects
            CGImageRelease(cg_img);
            CFRelease(cf_data as *const libc::c_void);

            return res;
        }
    }
}

#[cfg(target_os = "windows")]
mod ffi {
    #![allow(non_snake_case, dead_code)]

    use std::mem::size_of;

    use winapi::shared::minwindef;
    use winapi::shared::ntdef;
    use winapi::shared::windef;
    use winapi::um::shellscalingapi;
    use winapi::um::wingdi;
    use winapi::um::winuser;

    use ScreenResult;
    use Screenshot;

    /// Reorder rows in bitmap, last to first.
    /// TODO rewrite functionally
    fn flip_rows(data: Vec<u8>, height: usize, row_len: usize) -> Vec<u8> {
        let mut new_data = Vec::with_capacity(data.len());
        unsafe { new_data.set_len(data.len()) };
        for row_i in 0..height {
            for byte_i in 0..row_len {
                let old_idx = (height - row_i - 1) * row_len + byte_i;
                let new_idx = row_i * row_len + byte_i;
                new_data[new_idx] = data[old_idx];
            }
        }
        new_data
    }

    /// Reorder bytes in one pixel, BGR to RGB.
    fn flip_rgb(mut data: Vec<u8>, height: usize, width: usize, pixel_width: usize) -> Vec<u8> {
        // FIXME support other pixel_width but when?
        assert_eq!(pixel_width, 4);
        let row_len = width * pixel_width;
        for y in 0..height {
            for x in 0..width {
                data.swap(
                    y * row_len + x * pixel_width,
                    y * row_len + x * pixel_width + 2,
                );
            }
        }
        data
    }

    /// TODO Support multiple screens
    /// This may never happen, given the horrific quality of Win32 APIs
    pub fn get_screenshot(screen: usize) -> ScreenResult {
        use std::ptr::null_mut;
        unsafe {
            let mut dpi_awareness: shellscalingapi::PROCESS_DPI_AWARENESS = 0;
            shellscalingapi::GetProcessDpiAwareness(null_mut(), &mut dpi_awareness);
            shellscalingapi::SetProcessDpiAwareness(shellscalingapi::PROCESS_PER_MONITOR_DPI_AWARE);

            let monitor = enumerate_monitors()
                .into_iter()
                .nth(screen)
                .ok_or("failed to get specified screen")?;
            // Enumerate monitors, getting a handle and DC for requested monitor.
            // loljk, because doing that on Windows is worse than death
            let h_dc_screen = winuser::GetDC(null_mut());

            // Create a Windows Bitmap, and copy the bits into it
            let h_dc = wingdi::CreateCompatibleDC(h_dc_screen);
            if h_dc.is_null() {
                return Err("Can't get a Windows display.");
            }

            let h_bmp = wingdi::CreateCompatibleBitmap(h_dc_screen, monitor.width, monitor.height);
            if h_bmp.is_null() {
                return Err("Can't create a Windows buffer");
            }

            let res = wingdi::SelectObject(h_dc, h_bmp as windef::HGDIOBJ);
            if res == ntdef::NULL || res == wingdi::HGDI_ERROR {
                return Err("Can't select Windows buffer.");
            }

            let res = wingdi::BitBlt(
                h_dc,
                0,
                0,
                monitor.width,
                monitor.height,
                h_dc_screen,
                monitor.left,
                monitor.top,
                wingdi::SRCCOPY | wingdi::CAPTUREBLT,
            );
            if res == 0 {
                return Err("Failed to copy screen to Windows buffer");
            }

            // Get image info
            let pixel_width: usize = 4; // FIXME

            let mut bmi = wingdi::BITMAPINFO {
                bmiHeader: wingdi::BITMAPINFOHEADER {
                    biSize: size_of::<wingdi::BITMAPINFOHEADER>() as minwindef::DWORD,
                    biWidth: monitor.width as ntdef::LONG,
                    biHeight: monitor.height as ntdef::LONG,
                    biPlanes: 1,
                    biBitCount: 8 * pixel_width as minwindef::WORD,
                    biCompression: wingdi::BI_RGB,
                    biSizeImage: (monitor.width * monitor.height * pixel_width as minwindef::INT)
                        as minwindef::DWORD,
                    biXPelsPerMeter: 0,
                    biYPelsPerMeter: 0,
                    biClrUsed: 0,
                    biClrImportant: 0,
                },
                bmiColors: [wingdi::RGBQUAD {
                    rgbBlue: 0,
                    rgbGreen: 0,
                    rgbRed: 0,
                    rgbReserved: 0,
                }],
            };

            // Create a Vec for image
            let size: usize = (monitor.width * monitor.height) as usize * pixel_width;
            let mut data: Vec<u8> = Vec::with_capacity(size);
            data.set_len(size);

            // copy bits into Vec
            wingdi::GetDIBits(
                h_dc,
                h_bmp,
                0,
                monitor.height as minwindef::DWORD,
                &mut data[0] as *mut u8 as minwindef::LPVOID,
                &mut bmi as wingdi::LPBITMAPINFO,
                wingdi::DIB_RGB_COLORS,
            );

            // Release native image buffers
            winuser::ReleaseDC(null_mut(), h_dc_screen); // don't need screen anymore
            wingdi::DeleteDC(h_dc);
            wingdi::DeleteObject(h_bmp as windef::HGDIOBJ);

            let data = flip_rows(
                data,
                monitor.height as usize,
                monitor.width as usize * pixel_width,
            );

            let data = flip_rgb(
                data,
                monitor.height as usize,
                monitor.width as usize,
                pixel_width as usize,
            );

            shellscalingapi::SetProcessDpiAwareness(dpi_awareness);

            Ok(Screenshot {
                data,
                height: monitor.height as usize,
                width: monitor.width as usize,
                row_len: monitor.width as usize * pixel_width,
                pixel_width,
            })
        }
    }

    struct Monitor {
        left: i32,
        top: i32,
        height: i32,
        width: i32,
    }

    fn enumerate_monitors() -> Vec<Monitor> {
        use std::mem::transmute;
        use std::ptr::{null, null_mut};

        extern "system" fn callback(
            _: windef::HMONITOR,
            _: windef::HDC,
            rect: windef::LPRECT,
            lparam: minwindef::LPARAM,
        ) -> minwindef::BOOL {
            let refresult = unsafe { transmute::<minwindef::LPARAM, *mut Vec<Monitor>>(lparam) };
            let refresult: &mut Vec<Monitor> = unsafe { &mut *refresult };

            let monitor = unsafe {
                Monitor {
                    left: (*rect).left,
                    top: (*rect).top,
                    height: (*rect).bottom - (*rect).top,
                    width: (*rect).right - (*rect).left,
                }
            };

            refresult.push(monitor);

            minwindef::TRUE
        }

        let mut result: Vec<Monitor> = Vec::new();

        unsafe {
            winuser::EnumDisplayMonitors(
                null_mut::<*mut u8>() as windef::HDC,
                null::<*const u8>() as windef::LPCRECT,
                Some(callback),
                transmute::<*mut Vec<Monitor>, minwindef::LPARAM>(&mut result),
            );
        }

        result
    }
}

#[test]
fn test_get_screenshot() {
    let s: Screenshot = get_screenshot(0).unwrap();
    println!(
        "width: {}\n height: {}\npixel width: {}\n bytes: {}",
        s.width(),
        s.height(),
        s.pixel_width(),
        s.raw_len()
    );
}
