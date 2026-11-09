//! Desktop notifications and the alert sound of the watch rules (#18).
//!
//! Windows: a notification icon in the taskbar's notification area (`Shell_NotifyIconW`) whose
//! balloon (`NIF_INFO`) Windows 10 / 11 shows as a toast notification and keeps in the
//! notification center – no COM / WinRT needed. The icon is added at the first notification and
//! removed when the program ends ([`Notifier::remove`], also on drop). Its tooltip is the program
//! name. Other platforms: no notification (the hit is still listed in the Alerts window).

/// The notification icon of the program.
#[derive(Default)]
pub struct Notifier {
    #[cfg(windows)]
    icon: Option<win::TrayIcon>,
}

impl Notifier {
    /// Shows `title` / `text` as a notification of the window `hwnd` (the main window, which
    /// owns the icon). Returns false when it cannot be shown.
    pub fn show(&mut self, hwnd: Option<isize>, title: &str, text: &str) -> bool {
        #[cfg(windows)]
        {
            let Some(hwnd) = hwnd else {
                return false;
            };
            if self.icon.is_none() {
                self.icon = win::TrayIcon::add(hwnd);
            }
            self.icon.as_ref().is_some_and(|i| i.balloon(title, text))
        }
        #[cfg(not(windows))]
        {
            let _ = (hwnd, title, text);
            false
        }
    }

    /// Removes the icon from the notification area.
    pub fn remove(&mut self) {
        #[cfg(windows)]
        {
            self.icon = None;
        }
    }
}

/// The alert sound (Windows: the system "exclamation" sound).
pub fn beep() {
    #[cfg(windows)]
    {
        #[link(name = "user32")]
        unsafe extern "system" {
            fn MessageBeep(kind: u32) -> i32;
        }
        const MB_ICONWARNING: u32 = 0x30;
        // SAFETY: no pointers.
        unsafe {
            MessageBeep(MB_ICONWARNING);
        }
    }
}

/// `text` as a NUL-terminated UTF-16 array of `N` units, cut to fit.
#[cfg_attr(not(windows), allow(dead_code))]
fn fixed_wide<const N: usize>(text: &str) -> [u16; N] {
    let mut out = [0u16; N];
    for (slot, unit) in out.iter_mut().zip(text.encode_utf16().take(N - 1)) {
        *slot = unit;
    }
    out
}

#[cfg(windows)]
mod win {
    use super::fixed_wide;
    use std::ffi::c_void;

    type Handle = *mut c_void;

    /// `NOTIFYICONDATAW` (the full Vista+ layout).
    #[repr(C)]
    struct NotifyIconData {
        size: u32,
        hwnd: isize,
        id: u32,
        flags: u32,
        callback_message: u32,
        icon: Handle,
        tip: [u16; 128],
        state: u32,
        state_mask: u32,
        info: [u16; 256],
        timeout_or_version: u32,
        info_title: [u16; 64],
        info_flags: u32,
        guid: [u8; 16],
        balloon_icon: Handle,
    }
    #[cfg(target_pointer_width = "64")]
    const _: () = assert!(std::mem::size_of::<NotifyIconData>() == 976);

    #[link(name = "shell32")]
    unsafe extern "system" {
        fn Shell_NotifyIconW(message: u32, data: *mut NotifyIconData) -> i32;
        fn ExtractIconExW(
            file: *const u16,
            index: i32,
            large: *mut Handle,
            small: *mut Handle,
            count: u32,
        ) -> u32;
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn DestroyIcon(icon: Handle) -> i32;
    }

    const NIM_ADD: u32 = 0;
    const NIM_MODIFY: u32 = 1;
    const NIM_DELETE: u32 = 2;
    const NIF_ICON: u32 = 0x2;
    const NIF_TIP: u32 = 0x4;
    const NIF_INFO: u32 = 0x10;
    const NIIF_INFO: u32 = 0x1;
    /// The id of the program's only icon.
    const ICON_ID: u32 = 1;

    pub struct TrayIcon {
        hwnd: isize,
        icon: Handle,
    }

    impl TrayIcon {
        fn data(&self) -> NotifyIconData {
            NotifyIconData {
                size: std::mem::size_of::<NotifyIconData>() as u32,
                hwnd: self.hwnd,
                id: ICON_ID,
                flags: 0,
                callback_message: 0,
                icon: self.icon,
                tip: [0; 128],
                state: 0,
                state_mask: 0,
                info: [0; 256],
                timeout_or_version: 0,
                info_title: [0; 64],
                info_flags: 0,
                guid: [0; 16],
                balloon_icon: std::ptr::null_mut(),
            }
        }

        /// Adds the icon (the small icon of the own executable) for the window `hwnd`.
        pub fn add(hwnd: isize) -> Option<TrayIcon> {
            let exe = std::env::current_exe().ok()?;
            let wide: Vec<u16> = exe
                .as_os_str()
                .to_string_lossy()
                .encode_utf16()
                .chain([0])
                .collect();
            let mut small: Handle = std::ptr::null_mut();
            // SAFETY: `wide` is NUL-terminated; one small icon is written into `small`.
            unsafe { ExtractIconExW(wide.as_ptr(), 0, std::ptr::null_mut(), &mut small, 1) };
            let tray = TrayIcon { hwnd, icon: small };
            let mut d = tray.data();
            d.flags = NIF_ICON | NIF_TIP;
            d.tip = fixed_wide("MM PortLens");
            // SAFETY: `d` is a fully initialized NOTIFYICONDATAW with its size set.
            let ok = unsafe { Shell_NotifyIconW(NIM_ADD, &mut d) } != 0;
            // On failure `tray` is dropped: NIM_DELETE of an icon that is not there is harmless.
            ok.then_some(tray)
        }

        /// Shows a balloon / toast notification.
        pub fn balloon(&self, title: &str, text: &str) -> bool {
            let mut d = self.data();
            d.flags = NIF_INFO;
            d.info_title = fixed_wide(title);
            d.info = fixed_wide(text);
            d.info_flags = NIIF_INFO;
            // SAFETY: as in `add`.
            unsafe { Shell_NotifyIconW(NIM_MODIFY, &mut d) != 0 }
        }
    }

    impl Drop for TrayIcon {
        fn drop(&mut self) {
            let mut d = self.data();
            // SAFETY: as in `add`; the icon handle came from ExtractIconExW (or is null).
            unsafe {
                Shell_NotifyIconW(NIM_DELETE, &mut d);
                if !self.icon.is_null() {
                    DestroyIcon(self.icon);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts_are_cut_and_terminated() {
        let w: [u16; 4] = fixed_wide("abcdef");
        assert_eq!(w, [b'a' as u16, b'b' as u16, b'c' as u16, 0]);
        let w: [u16; 8] = fixed_wide("→ž");
        assert_eq!(&w[..3], &[0x2192, 0x17E, 0]);
    }
}
