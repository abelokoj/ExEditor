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
            Dwm::DwmSetWindowAttribute,
            Gdi::{CreateRoundRectRgn, DeleteObject, SetWindowRgn},
        },
        System::Threading::GetCurrentProcessId,
        UI::{
            HiDpi::GetDpiForWindow,
            WindowsAndMessaging::{
                EnumWindows, GWL_STYLE, GetClassNameW, GetWindowLongPtrW, GetWindowRect,
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
                if width <= 0 || height <= 0 || shapes.get(&(hwnd as usize)) == Some(&shape) {
                    continue;
                }
                let preference = if maximized { 1_u32 } else { 2_u32 };
                // SAFETY: DWMWA_WINDOW_CORNER_PREFERENCE (33) reads one DWORD. Older Windows returns an error.
                let rounded = unsafe {
                    DwmSetWindowAttribute(hwnd, 33, (&preference as *const u32).cast::<c_void>(), 4)
                } >= 0;
                // Frameless WS_POPUP windows need a region even when DWM accepts the preference.
                // SAFETY: reading style and DPI does not mutate or retain the handle.
                let frameless =
                    unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } & WS_CAPTION as isize == 0;
                if maximized {
                    // SAFETY: a null region restores the full rectangular window.
                    unsafe { SetWindowRgn(hwnd, std::ptr::null_mut(), 1) };
                } else if frameless || !rounded {
                    let radius = (8 * unsafe { GetDpiForWindow(hwnd) }.max(96) / 96) as i32;
                    // SAFETY: positive bounded window dimensions; this creates a new owned GDI region.
                    let region = unsafe {
                        CreateRoundRectRgn(0, 0, width + 1, height + 1, radius * 2, radius * 2)
                    };
                    if !region.is_null() {
                        // On success Windows owns the region; on failure we must release it.
                        // SAFETY: hwnd and region refer to live native resources.
                        if unsafe { SetWindowRgn(hwnd, region, 1) } == 0 {
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
