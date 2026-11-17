//! Native rounding for NSH windows, including frameless popup windows.
pub fn refresh() {
    #[cfg(windows)]
    windows::refresh();
}

/// A secondary native window is created after its content's first layout.
pub fn after_open() {
    refresh();
    #[cfg(windows)]
    nsh_core::element::spawn(async {
        nsh_core::element::timer(std::time::Duration::from_millis(100)).await;
        refresh();
    });
}

#[cfg(windows)]
mod windows {
    use std::{cell::RefCell, collections::BTreeMap, ffi::c_void};
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM, RECT},
        Graphics::{
            Dwm::{DwmGetWindowAttribute, DwmSetWindowAttribute},
            Gdi::{CreateRectRgn, CreateRoundRectRgn, DeleteObject, GetWindowRgn, SetWindowRgn},
        },
        System::Threading::GetCurrentProcessId,
        UI::{
            HiDpi::GetDpiForWindow,
            WindowsAndMessaging::{
                EnumWindows, GWL_STYLE, GetClassNameW, GetWindowLongW, GetWindowRect,
                GetWindowThreadProcessId, IsZoomed, WS_CAPTION,
            },
        },
    };
    type Shape = (i32, i32, bool);
    thread_local! { static SHAPES: RefCell<BTreeMap<usize, Shape>> = const { RefCell::new(BTreeMap::new()) }; }

    unsafe extern "system" fn collect(hwnd: HWND, data: LPARAM) -> i32 {
        let mut process = 0;
        let mut class = [0_u16; 32];
        // SAFETY: the out buffers are live locals; EnumWindows supplies a live HWND.
        let ours = unsafe {
            GetWindowThreadProcessId(hwnd, &mut process);
            process == GetCurrentProcessId()
                && GetClassNameW(hwnd, class.as_mut_ptr(), 32) == 9
                && class[..9] == [78, 115, 104, 87, 105, 110, 100, 111, 119]
        };
        if ours {
            // SAFETY: refresh owns this Vec for the synchronous EnumWindows call.
            unsafe { (&mut *(data as *mut Vec<HWND>)).push(hwnd) };
        }
        1
    }
    pub(super) fn refresh() {
        let mut handles = Vec::new();
        // SAFETY: collect only appends to the Vec, which remains alive throughout enumeration.
        unsafe { EnumWindows(Some(collect), &mut handles as *mut Vec<HWND> as LPARAM) };
        if std::env::var_os("EXEDITOR_NATIVE_DIAGNOSTICS").is_some() {
            eprintln!("Native compatibility: {} NSH windows", handles.len());
        }
        SHAPES.with(|shapes| {
            let mut shapes = shapes.borrow_mut();
            shapes.retain(|handle, _| handles.iter().any(|hwnd| *hwnd as usize == *handle));
            for hwnd in handles {
                crate::keyboard::install_keypad_aliases(hwnd);
                let mut rect = RECT::default();
                // SAFETY: rect is a valid out buffer; a closed HWND simply fails.
                if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
                    continue;
                }
                let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
                // SAFETY: querying this process's native window.
                let maximized = unsafe { IsZoomed(hwnd) } != 0;
                let shape = (width, height, maximized);
                if std::env::var_os("EXEDITOR_NATIVE_DIAGNOSTICS").is_some() {
                    eprintln!("Native shape {}: {shape:?}", hwnd as usize);
                }
                if width <= 0 || height <= 0 {
                    continue;
                }
                // NSH can reset DWM corners when it applies its frame after first layout.
                // Only reuse a shape while its native DWM corner preference or GDI region persists.
                let mut current_corner = 0_u32;
                // SAFETY: DWMWA_WINDOW_CORNER_PREFERENCE (33) writes one DWORD into a live local.
                let dwm_query = unsafe {
                    DwmGetWindowAttribute(
                        hwnd,
                        33,
                        (&mut current_corner as *mut u32).cast::<c_void>(),
                        4,
                    )
                };
                if shapes.get(&(hwnd as usize)) == Some(&shape) {
                    if maximized || (dwm_query >= 0 && current_corner == 2) {
                        continue;
                    }
                    // SAFETY: creates a temporary empty GDI region to query the window's region kind.
                    let region = unsafe { CreateRectRgn(0, 0, 0, 0) };
                    if !region.is_null() {
                        // SAFETY: region is an owned query buffer; hwnd is a live app window.
                        let present = unsafe { GetWindowRgn(hwnd, region) } == 3;
                        // SAFETY: region is released immediately after GetWindowRgn inspects it.
                        unsafe { DeleteObject(region) };
                        if present {
                            continue;
                        }
                    }
                }
                let preference = if maximized { 1_u32 } else { 2_u32 };
                // SAFETY: DWMWA_WINDOW_CORNER_PREFERENCE (33) reads one DWORD. Older Windows returns an error.
                let rounded = unsafe {
                    DwmSetWindowAttribute(hwnd, 33, (&preference as *const u32).cast::<c_void>(), 4)
                } >= 0;
                let mut verified_corner = 0_u32;
                // SAFETY: verifies whether DWM actually retained the requested corner preference (false on headless/older displays).
                let dwm_active = rounded
                    && unsafe {
                        DwmGetWindowAttribute(
                            hwnd,
                            33,
                            (&mut verified_corner as *mut u32).cast::<c_void>(),
                            4,
                        )
                    } >= 0
                    && verified_corner == preference;
                if std::env::var_os("EXEDITOR_NATIVE_DIAGNOSTICS").is_some() {
                    eprintln!("Native corner preference set: {rounded} (active={dwm_active})");
                }
                // SAFETY: reads standard window style flags from a live HWND of this process.
                let has_native_caption =
                    (unsafe { GetWindowLongW(hwnd, GWL_STYLE) } as u32 & WS_CAPTION) != 0;
                if maximized || dwm_active || has_native_caption {
                    // SAFETY: a null region restores full DWM composition without GDI clipping.
                    unsafe { SetWindowRgn(hwnd, std::ptr::null_mut(), 1) };
                } else {
                    // SAFETY: GetDpiForWindow queries the window's DPI on Windows 10+.
                    let radius = (8 * unsafe { GetDpiForWindow(hwnd) }.max(96) / 96) as i32;
                    // SAFETY: positive bounded window dimensions; this creates a new owned GDI region.
                    let region = unsafe {
                        CreateRoundRectRgn(0, 0, width + 1, height + 1, radius * 2, radius * 2)
                    };
                    if !region.is_null() {
                        // On success Windows owns the region; on failure we must release it.
                        // SAFETY: hwnd and region refer to live native resources.
                        let applied = unsafe { SetWindowRgn(hwnd, region, 1) };
                        if std::env::var_os("EXEDITOR_NATIVE_DIAGNOSTICS").is_some() {
                            eprintln!("Native region applied: {applied}");
                        }
                        if applied == 0 {
                            // SAFETY: ownership was not transferred when SetWindowRgn failed.
                            unsafe { DeleteObject(region) };
                        }
                    }
                }
                shapes.insert(hwnd as usize, shape);
            }
        });
    }
}
