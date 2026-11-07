//! Process names and paths for the PIDs in the tables, cached between refreshes.
//!
//! A PID can be reused by a new process; the cache therefore forgets PIDs that no longer own any
//! endpoint ([`ProcessCache::retain`]) and looks them up again when they come back.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// What the table shows about a process.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProcessInfo {
    /// File name, e.g. `chrome.exe`; for system processes their usual name.
    pub name: String,
    /// Full path of the executable; empty when it cannot be read (other users' processes
    /// without admin rights, protected processes).
    pub path: String,
}

#[derive(Default)]
pub struct ProcessCache {
    map: HashMap<u32, Arc<ProcessInfo>>,
}

impl ProcessCache {
    pub fn get(&mut self, pid: u32) -> Arc<ProcessInfo> {
        self.map
            .entry(pid)
            .or_insert_with(|| Arc::new(lookup(pid)))
            .clone()
    }

    /// Forgets PIDs not in `alive`.
    pub fn retain(&mut self, alive: &HashSet<u32>) {
        self.map.retain(|pid, _| alive.contains(pid));
    }
}

/// File name part of a path (both `\` and `/` separators).
pub fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// Names of PIDs that have no executable to look up.
fn special_name(pid: u32) -> Option<&'static str> {
    match pid {
        0 if cfg!(windows) => Some("System Idle Process"),
        0 => Some("?"),
        4 if cfg!(windows) => Some("System"),
        _ => None,
    }
}

fn lookup(pid: u32) -> ProcessInfo {
    if let Some(name) = special_name(pid) {
        return ProcessInfo {
            name: name.into(),
            path: String::new(),
        };
    }
    let path = platform::image_path(pid).unwrap_or_default();
    let name = if path.is_empty() {
        platform::short_name(pid).unwrap_or_else(|| format!("<{pid}>"))
    } else {
        file_name(&path).to_owned()
    };
    ProcessInfo { name, path }
}

#[cfg(windows)]
mod platform {
    use std::ffi::c_void;

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn QueryFullProcessImageNameW(
            process: *mut c_void,
            flags: u32,
            name: *mut u16,
            size: *mut u32,
        ) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }

    /// Full image path, e.g. `C:\Program Files\…\chrome.exe`.
    pub fn image_path(pid: u32) -> Option<String> {
        // SAFETY: plain Win32 calls; the handle is closed before returning and the buffer is
        // valid for `len` UTF-16 units.
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return None;
            }
            let mut buf = vec![0u16; 32768];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len);
            CloseHandle(h);
            (ok != 0).then(|| String::from_utf16_lossy(&buf[..len as usize]))
        }
    }

    /// Without a path (access denied) Windows gives us no name either.
    pub fn short_name(_pid: u32) -> Option<String> {
        None
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn image_path(pid: u32) -> Option<String> {
        std::fs::read_link(format!("/proc/{pid}/exe"))
            .ok()
            .map(|p| p.to_string_lossy().into_owned())
    }

    pub fn short_name(pid: u32) -> Option<String> {
        std::fs::read_to_string(format!("/proc/{pid}/comm"))
            .ok()
            .map(|s| s.trim().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names() {
        assert_eq!(file_name(r"C:\Windows\System32\svchost.exe"), "svchost.exe");
        assert_eq!(file_name("/usr/bin/ssh"), "ssh");
        assert_eq!(file_name("plain"), "plain");
    }

    #[test]
    fn cache_forgets_dead_pids() {
        let mut c = ProcessCache::default();
        let own = std::process::id();
        let me = c.get(own);
        assert!(!me.name.is_empty());
        c.retain(&HashSet::new());
        assert!(c.map.is_empty());
    }
}
