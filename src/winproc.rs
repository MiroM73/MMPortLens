//! Windows details of a process (#7): image path, start time, user, command line, version
//! strings, icon, and the services hosted by each PID. All functions return `None` / empty
//! values when Windows refuses access (other users' and protected processes without
//! administrator rights).
//!
//! The Win32 functions are declared directly (no binding crate). The parts that only shuffle
//! bytes ([`icon_rgba`], [`version_key`], [`filetime_to_unix_ms`]) are pure and tested on every
//! platform.

// The pure helpers are used only on Windows; tested everywhere.
#![cfg_attr(not(windows), allow(dead_code))]

/// A process icon as RGBA pixels (`size[0] * size[1] * 4` bytes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Icon {
    pub size: [usize; 2],
    pub rgba: Vec<u8>,
}

/// FILETIME (100 ns since 1601) → ms since the Unix epoch.
pub fn filetime_to_unix_ms(ft: u64) -> Option<u64> {
    const EPOCH_DIFF: u64 = 116_444_736_000_000_000;
    ft.checked_sub(EPOCH_DIFF).map(|t| t / 10_000)
}

/// The `VerQueryValueW` path of a version string, e.g. `\StringFileInfo\040904b0\CompanyName`.
pub fn version_key(lang: u16, codepage: u16, name: &str) -> String {
    format!("\\StringFileInfo\\{lang:04x}{codepage:04x}\\{name}")
}

/// RGBA pixels of an icon from the BGRA rows of its color bitmap and (for icons without an alpha
/// channel) the BGRA rows of its AND mask: a black mask pixel is opaque, a white one transparent.
pub fn icon_rgba(color_bgra: &[u8], mask_bgra: Option<&[u8]>) -> Vec<u8> {
    let has_alpha = color_bgra.as_chunks::<4>().0.iter().any(|p| p[3] != 0);
    let mut out = Vec::with_capacity(color_bgra.len());
    for (i, p) in color_bgra.as_chunks::<4>().0.iter().enumerate() {
        let a = if has_alpha {
            p[3]
        } else {
            match mask_bgra.and_then(|m| m.get(i * 4)) {
                Some(&m) if m != 0 => 0,
                _ => 255,
            }
        };
        out.extend_from_slice(&[p[2], p[1], p[0], a]);
    }
    out
}

/// What [`details`] finds about a process.
#[derive(Default, Debug)]
pub struct Details {
    pub user: String,
    pub command_line: String,
    pub company: String,
    pub description: String,
}

#[cfg(windows)]
pub use imp::*;

#[cfg(windows)]
mod imp {
    use super::{Details, Icon, filetime_to_unix_ms, icon_rgba, version_key};
    use std::collections::HashMap;
    use std::ffi::c_void;

    type Handle = *mut c_void;

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const TOKEN_QUERY: u32 = 0x0008;
    const TOKEN_USER_CLASS: u32 = 1;
    const PROCESS_COMMAND_LINE_INFORMATION: u32 = 60;
    const STATUS_INFO_LENGTH_MISMATCH: i32 = 0xC000_0004_u32 as i32;
    const SC_MANAGER_ENUMERATE_SERVICE: u32 = 0x0004;
    const SC_ENUM_PROCESS_INFO: i32 = 0;
    const SERVICE_WIN32: u32 = 0x30;
    const SERVICE_ACTIVE: u32 = 0x1;
    const ERROR_MORE_DATA: u32 = 234;
    const DIB_RGB_COLORS: u32 = 0;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
        fn CloseHandle(handle: Handle) -> i32;
        fn QueryFullProcessImageNameW(p: Handle, flags: u32, name: *mut u16, size: *mut u32)
        -> i32;
        fn GetProcessTimes(p: Handle, c: *mut u64, e: *mut u64, k: *mut u64, u: *mut u64) -> i32;
        fn GetLastError() -> u32;
    }

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn OpenProcessToken(p: Handle, access: u32, token: *mut Handle) -> i32;
        fn GetTokenInformation(
            token: Handle,
            class: u32,
            info: *mut c_void,
            len: u32,
            returned: *mut u32,
        ) -> i32;
        fn LookupAccountSidW(
            system: *const u16,
            sid: *mut c_void,
            name: *mut u16,
            name_len: *mut u32,
            domain: *mut u16,
            domain_len: *mut u32,
            use_: *mut i32,
        ) -> i32;
        fn OpenSCManagerW(machine: *const u16, db: *const u16, access: u32) -> Handle;
        fn EnumServicesStatusExW(
            scm: Handle,
            level: i32,
            kind: u32,
            state: u32,
            buf: *mut u8,
            size: u32,
            needed: *mut u32,
            returned: *mut u32,
            resume: *mut u32,
            group: *const u16,
        ) -> i32;
        fn CloseServiceHandle(h: Handle) -> i32;
    }

    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtQueryInformationProcess(
            p: Handle,
            class: u32,
            buf: *mut c_void,
            len: u32,
            ret: *mut u32,
        ) -> i32;
    }

    #[link(name = "version")]
    unsafe extern "system" {
        fn GetFileVersionInfoSizeW(path: *const u16, handle: *mut u32) -> u32;
        fn GetFileVersionInfoW(path: *const u16, handle: u32, len: u32, data: *mut c_void) -> i32;
        fn VerQueryValueW(
            block: *const c_void,
            sub: *const u16,
            out: *mut *mut c_void,
            len: *mut u32,
        ) -> i32;
    }

    #[link(name = "shell32")]
    unsafe extern "system" {
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
        fn GetIconInfo(icon: Handle, info: *mut IconInfo) -> i32;
        fn DestroyIcon(icon: Handle) -> i32;
        fn GetDC(wnd: Handle) -> Handle;
        fn ReleaseDC(wnd: Handle, dc: Handle) -> i32;
    }

    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn GetObjectW(obj: Handle, size: i32, out: *mut c_void) -> i32;
        fn GetDIBits(
            dc: Handle,
            bmp: Handle,
            start: u32,
            lines: u32,
            bits: *mut c_void,
            info: *mut BitmapInfo,
            usage: u32,
        ) -> i32;
        fn DeleteObject(obj: Handle) -> i32;
    }

    #[repr(C)]
    struct IconInfo {
        is_icon: i32,
        x_hotspot: u32,
        y_hotspot: u32,
        mask: Handle,
        color: Handle,
    }

    #[repr(C)]
    struct Bitmap {
        kind: i32,
        width: i32,
        height: i32,
        width_bytes: i32,
        planes: u16,
        bits_pixel: u16,
        bits: *mut c_void,
    }

    #[repr(C)]
    struct BitmapInfo {
        size: u32,
        width: i32,
        height: i32,
        planes: u16,
        bit_count: u16,
        compression: u32,
        size_image: u32,
        x_ppm: i32,
        y_ppm: i32,
        clr_used: u32,
        clr_important: u32,
        colors: [u32; 4],
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn from_wide(buf: &[u16]) -> String {
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..end])
    }

    /// An open process handle, closed on drop.
    struct Process(Handle);

    impl Process {
        fn open(pid: u32) -> Option<Self> {
            // SAFETY: plain Win32 call; a null handle is checked.
            let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
            (!h.is_null()).then_some(Process(h))
        }
    }

    impl Drop for Process {
        fn drop(&mut self) {
            // SAFETY: the handle came from OpenProcess and is closed once.
            unsafe { CloseHandle(self.0) };
        }
    }

    /// Full image path, e.g. `C:\Program Files\…\chrome.exe`.
    pub fn image_path(pid: u32) -> Option<String> {
        let p = Process::open(pid)?;
        let mut buf = vec![0u16; 32768];
        let mut len = buf.len() as u32;
        // SAFETY: `buf` is valid for `len` UTF-16 units; the handle is open.
        let ok = unsafe { QueryFullProcessImageNameW(p.0, 0, buf.as_mut_ptr(), &mut len) };
        (ok != 0).then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }

    /// When the process started (ms since the Unix epoch) – with the PID it identifies the
    /// process (a reused PID has another start time).
    pub fn start_time(pid: u32) -> Option<u64> {
        let p = Process::open(pid)?;
        let (mut c, mut e, mut k, mut u) = (0u64, 0u64, 0u64, 0u64);
        // SAFETY: four valid FILETIME-sized outputs; the handle is open.
        let ok = unsafe { GetProcessTimes(p.0, &mut c, &mut e, &mut k, &mut u) };
        if ok == 0 {
            return None;
        }
        filetime_to_unix_ms(c)
    }

    /// User, command line and the version strings of `path`.
    pub fn details(pid: u32, path: &str) -> Details {
        let mut d = Details::default();
        if let Some(p) = Process::open(pid) {
            d.user = user(&p).unwrap_or_default();
            d.command_line = command_line(&p).unwrap_or_default();
        }
        if !path.is_empty() {
            d.company = version_string(path, "CompanyName").unwrap_or_default();
            d.description = version_string(path, "FileDescription").unwrap_or_default();
        }
        d
    }

    /// `DOMAIN\user` of the process token.
    fn user(p: &Process) -> Option<String> {
        let mut token: Handle = std::ptr::null_mut();
        // SAFETY: `token` receives a handle that is closed below.
        if unsafe { OpenProcessToken(p.0, TOKEN_QUERY, &mut token) } == 0 {
            return None;
        }
        let token = Process(token);
        // TOKEN_USER: a SID pointer + attributes, followed by the SID itself.
        let mut buf = vec![0u64; 64];
        let mut len = 0u32;
        // SAFETY: `buf` is valid and 8-byte aligned for 512 bytes.
        let ok = unsafe {
            GetTokenInformation(
                token.0,
                TOKEN_USER_CLASS,
                buf.as_mut_ptr().cast(),
                (buf.len() * 8) as u32,
                &mut len,
            )
        };
        if ok == 0 {
            return None;
        }
        let sid = buf[0] as usize as *mut c_void;
        let (mut name, mut domain) = (vec![0u16; 256], vec![0u16; 256]);
        let (mut nl, mut dl, mut use_) = (256u32, 256u32, 0i32);
        // SAFETY: `sid` points into `buf`, which is alive; the buffers are valid for their sizes.
        let ok = unsafe {
            LookupAccountSidW(
                std::ptr::null(),
                sid,
                name.as_mut_ptr(),
                &mut nl,
                domain.as_mut_ptr(),
                &mut dl,
                &mut use_,
            )
        };
        if ok == 0 {
            return None;
        }
        let (name, domain) = (from_wide(&name), from_wide(&domain));
        Some(if domain.is_empty() {
            name
        } else {
            format!("{domain}\\{name}")
        })
    }

    /// The command line (Windows 8.1+: `ProcessCommandLineInformation`).
    fn command_line(p: &Process) -> Option<String> {
        let mut size = 4096u32;
        for _ in 0..3 {
            // UNICODE_STRING (Length, MaximumLength, Buffer) followed by the text it points to.
            let mut buf = vec![0u64; (size as usize).div_ceil(8)];
            let mut ret = 0u32;
            // SAFETY: `buf` is valid and aligned for `size` bytes.
            let st = unsafe {
                NtQueryInformationProcess(
                    p.0,
                    PROCESS_COMMAND_LINE_INFORMATION,
                    buf.as_mut_ptr().cast(),
                    size,
                    &mut ret,
                )
            };
            if st == STATUS_INFO_LENGTH_MISMATCH && ret > size {
                size = ret;
                continue;
            }
            if st < 0 {
                return None;
            }
            let bytes = (buf[0] & 0xFFFF) as usize;
            let text = buf[1] as usize as *const u16;
            let base = buf.as_ptr() as usize;
            // The text must lie inside our buffer.
            if text.is_null()
                || (text as usize) < base
                || text as usize + bytes > base + buf.len() * 8
            {
                return None;
            }
            // SAFETY: checked above that `bytes` bytes at `text` are inside `buf`.
            let units = unsafe { std::slice::from_raw_parts(text, bytes / 2) };
            return Some(String::from_utf16_lossy(units));
        }
        None
    }

    /// A string of the version resource (first translation).
    fn version_string(path: &str, name: &str) -> Option<String> {
        let wpath = wide(path);
        let mut handle = 0u32;
        // SAFETY: `wpath` is NUL-terminated.
        let size = unsafe { GetFileVersionInfoSizeW(wpath.as_ptr(), &mut handle) };
        if size == 0 {
            return None;
        }
        let mut data = vec![0u64; (size as usize).div_ceil(8)];
        // SAFETY: `data` is valid for `size` bytes.
        if unsafe { GetFileVersionInfoW(wpath.as_ptr(), 0, size, data.as_mut_ptr().cast()) } == 0 {
            return None;
        }
        let query = |sub: &str| -> Option<(*mut c_void, u32)> {
            let wsub = wide(sub);
            let mut out: *mut c_void = std::ptr::null_mut();
            let mut len = 0u32;
            // SAFETY: `data` holds the version block; `out` points into it on success.
            let ok =
                unsafe { VerQueryValueW(data.as_ptr().cast(), wsub.as_ptr(), &mut out, &mut len) };
            (ok != 0 && !out.is_null()).then_some((out, len))
        };
        // Translations: pairs of (language, code page); fall back to US English / Unicode.
        let mut langs = Vec::new();
        if let Some((p, len)) = query("\\VarFileInfo\\Translation") {
            let n = len as usize / 4;
            // SAFETY: `len` bytes of u16 pairs at `p`, inside `data`.
            let pairs = unsafe { std::slice::from_raw_parts(p as *const u16, n * 2) };
            langs.extend(pairs.as_chunks::<2>().0.iter().map(|c| (c[0], c[1])));
        }
        langs.extend([(0x0409, 0x04b0), (0x0409, 0x04e4)]);
        for (lang, cp) in langs {
            if let Some((p, len)) = query(&version_key(lang, cp, name)) {
                // SAFETY: `len` UTF-16 units at `p`, inside `data`.
                let s = unsafe { std::slice::from_raw_parts(p as *const u16, len as usize) };
                let s = from_wide(s).trim().to_owned();
                if !s.is_empty() {
                    return Some(s);
                }
            }
        }
        None
    }

    /// The first (large, 32 px) icon of an executable.
    pub fn icon(path: &str) -> Option<Icon> {
        let wpath = wide(path);
        let mut large: Handle = std::ptr::null_mut();
        // SAFETY: `wpath` is NUL-terminated; one icon handle is written into `large`.
        let n = unsafe { ExtractIconExW(wpath.as_ptr(), 0, &mut large, std::ptr::null_mut(), 1) };
        if n == 0 || n == u32::MAX || large.is_null() {
            return None;
        }
        let res = icon_pixels(large);
        // SAFETY: the icon came from ExtractIconExW and is destroyed once.
        unsafe { DestroyIcon(large) };
        res
    }

    fn icon_pixels(icon: Handle) -> Option<Icon> {
        let mut info = IconInfo {
            is_icon: 0,
            x_hotspot: 0,
            y_hotspot: 0,
            mask: std::ptr::null_mut(),
            color: std::ptr::null_mut(),
        };
        // SAFETY: `info` is a valid ICONINFO; the bitmaps it returns are deleted below.
        if unsafe { GetIconInfo(icon, &mut info) } == 0 {
            return None;
        }
        let res = (|| {
            if info.color.is_null() {
                return None; // monochrome icon
            }
            let (w, h) = bitmap_size(info.color)?;
            let color = dib_bgra(info.color, w, h)?;
            let mask = (!info.mask.is_null())
                .then(|| dib_bgra(info.mask, w, h))
                .flatten();
            Some(Icon {
                size: [w as usize, h as usize],
                rgba: icon_rgba(&color, mask.as_deref()),
            })
        })();
        // SAFETY: the bitmaps of GetIconInfo belong to the caller.
        unsafe {
            if !info.color.is_null() {
                DeleteObject(info.color);
            }
            if !info.mask.is_null() {
                DeleteObject(info.mask);
            }
        }
        res
    }

    fn bitmap_size(bmp: Handle) -> Option<(i32, i32)> {
        let mut b = Bitmap {
            kind: 0,
            width: 0,
            height: 0,
            width_bytes: 0,
            planes: 0,
            bits_pixel: 0,
            bits: std::ptr::null_mut(),
        };
        let size = std::mem::size_of::<Bitmap>() as i32;
        // SAFETY: `b` is a valid BITMAP of `size` bytes.
        let n = unsafe { GetObjectW(bmp, size, (&mut b as *mut Bitmap).cast()) };
        (n != 0 && b.width > 0 && b.height > 0 && b.width <= 256 && b.height <= 256)
            .then_some((b.width, b.height))
    }

    /// The bitmap as top-down 32-bit BGRA rows.
    fn dib_bgra(bmp: Handle, w: i32, h: i32) -> Option<Vec<u8>> {
        let mut bi = BitmapInfo {
            size: 40,
            width: w,
            height: -h,
            planes: 1,
            bit_count: 32,
            compression: 0,
            size_image: 0,
            x_ppm: 0,
            y_ppm: 0,
            clr_used: 0,
            clr_important: 0,
            colors: [0; 4],
        };
        let mut out = vec![0u8; (w * h * 4) as usize];
        // SAFETY: the screen DC is released below; `out` holds w * h 32-bit pixels.
        unsafe {
            let dc = GetDC(std::ptr::null_mut());
            let n = GetDIBits(
                dc,
                bmp,
                0,
                h as u32,
                out.as_mut_ptr().cast(),
                &mut bi,
                DIB_RGB_COLORS,
            );
            ReleaseDC(std::ptr::null_mut(), dc);
            (n == h).then_some(out)
        }
    }

    /// Running Win32 services by PID: (service name, display name).
    pub fn services() -> HashMap<u32, Vec<(String, String)>> {
        let mut map: HashMap<u32, Vec<(String, String)>> = HashMap::new();
        // SAFETY: plain call; the handle is closed below.
        let scm = unsafe {
            OpenSCManagerW(
                std::ptr::null(),
                std::ptr::null(),
                SC_MANAGER_ENUMERATE_SERVICE,
            )
        };
        if scm.is_null() {
            return map;
        }
        // ENUM_SERVICE_STATUS_PROCESSW: two string pointers + SERVICE_STATUS_PROCESS (9 DWORDs,
        // the process id is the 8th), padded to pointer alignment.
        let ptr = std::mem::size_of::<usize>();
        let stride = (2 * ptr + 36).div_ceil(ptr) * ptr;
        let pid_at = 2 * ptr + 7 * 4;
        let mut resume = 0u32;
        let mut buf = vec![0u64; 64 * 1024 / 8];
        for _ in 0..64 {
            let (mut needed, mut count) = (0u32, 0u32);
            // SAFETY: `buf` is valid and aligned for its byte length; strings it points to lie
            // inside it.
            let ok = unsafe {
                EnumServicesStatusExW(
                    scm,
                    SC_ENUM_PROCESS_INFO,
                    SERVICE_WIN32,
                    SERVICE_ACTIVE,
                    buf.as_mut_ptr().cast(),
                    (buf.len() * 8) as u32,
                    &mut needed,
                    &mut count,
                    &mut resume,
                    std::ptr::null(),
                )
            };
            // SAFETY: reads the calling thread's last error.
            let more = ok == 0 && unsafe { GetLastError() } == ERROR_MORE_DATA;
            if ok == 0 && !more {
                break;
            }
            let base = buf.as_ptr() as *const u8;
            for i in 0..count as usize {
                // SAFETY: `count` entries of `stride` bytes were written at the start of `buf`;
                // their string pointers point into `buf` and are NUL-terminated.
                unsafe {
                    let e = base.add(i * stride);
                    let name = (e as *const *const u16).read_unaligned();
                    let display = (e.add(ptr) as *const *const u16).read_unaligned();
                    let pid = (e.add(pid_at) as *const u32).read_unaligned();
                    if pid != 0 {
                        map.entry(pid)
                            .or_default()
                            .push((wide_ptr(name), wide_ptr(display)));
                    }
                }
            }
            if !more {
                break;
            }
            if needed as usize > buf.len() * 8 {
                buf = vec![0u64; (needed as usize).div_ceil(8)];
            }
        }
        // SAFETY: the handle came from OpenSCManagerW.
        unsafe { CloseServiceHandle(scm) };
        for v in map.values_mut() {
            v.sort();
        }
        map
    }

    /// A NUL-terminated UTF-16 string.
    ///
    /// # Safety
    /// `p` is null or points to a NUL-terminated string.
    unsafe fn wide_ptr(p: *const u16) -> String {
        if p.is_null() {
            return String::new();
        }
        let mut n = 0;
        // SAFETY: the caller guarantees a terminating NUL.
        unsafe {
            while *p.add(n) != 0 && n < 4096 {
                n += 1;
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(p, n))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filetime_conversion() {
        // 2023-11-14T22:13:20Z
        assert_eq!(
            filetime_to_unix_ms(116_444_736_000_000_000 + 17_000_000_000_000_000),
            Some(1_700_000_000_000)
        );
        assert_eq!(filetime_to_unix_ms(5), None);
    }

    #[test]
    fn version_keys() {
        assert_eq!(
            version_key(0x0409, 0x04b0, "CompanyName"),
            "\\StringFileInfo\\040904b0\\CompanyName"
        );
    }

    #[test]
    fn icon_alpha_from_color_or_mask() {
        // Two pixels BGRA with alpha: kept, channels swapped.
        let c = [1, 2, 3, 200, 4, 5, 6, 0];
        assert_eq!(icon_rgba(&c, None), vec![3, 2, 1, 200, 6, 5, 4, 0]);
        // No alpha at all: the mask decides (black = opaque, white = transparent).
        let c = [1, 2, 3, 0, 4, 5, 6, 0];
        let m = [0, 0, 0, 0, 255, 255, 255, 0];
        assert_eq!(icon_rgba(&c, Some(&m)), vec![3, 2, 1, 255, 6, 5, 4, 0]);
    }

    #[cfg(windows)]
    #[test]
    fn own_process_details() {
        let me = std::process::id();
        let path = image_path(me).unwrap();
        let started = start_time(me).unwrap();
        let now = crate::model::now_ms();
        assert!(
            started <= now && now - started < 3_600_000,
            "{started} {now}"
        );
        let d = details(me, &path);
        assert!(d.user.contains('\\'), "{:?}", d.user);
        let exe = crate::process::file_name(&path).to_lowercase();
        assert!(
            d.command_line
                .to_lowercase()
                .contains(exe.trim_end_matches(".exe")),
            "{d:?}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn system_file_version_and_icon() {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let path = format!(r"{root}\System32\notepad.exe");
        let d = details(0, &path);
        assert!(d.company.contains("Microsoft"), "{d:?}");
        assert!(!d.description.is_empty());
        let icon = icon(&path).expect("notepad has an icon");
        assert_eq!(icon.rgba.len(), icon.size[0] * icon.size[1] * 4);
        assert!(icon.rgba.as_chunks::<4>().0.iter().any(|p| p[3] > 0));
    }

    #[cfg(windows)]
    #[test]
    fn services_by_pid() {
        let map = services();
        // Every Windows has running services in svchost processes.
        assert!(!map.is_empty());
        assert!(map.values().flatten().all(|(n, _)| !n.is_empty()));
    }
}
