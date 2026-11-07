//! What the table shows about the process of an endpoint, cached between refreshes (#7).
//!
//! A PID can be reused by a new process, so an entry is identified by the PID **and** the start
//! time of the process: when the start time changes, the process is looked up again. PIDs that no
//! longer own any endpoint are forgotten ([`ProcessCache::retain`]). Icons are cached per
//! executable path, the services hosted by each PID are read again every
//! [`SERVICES_EVERY`].

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub use crate::winproc::Icon;

/// How often the list of services per PID is read again.
pub const SERVICES_EVERY: Duration = Duration::from_secs(10);

/// What the table shows about a process.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProcessInfo {
    /// File name, e.g. `chrome.exe`; for system processes their usual name.
    pub name: String,
    /// Full path of the executable; empty when it cannot be read (other users' processes
    /// without admin rights, protected processes).
    pub path: String,
    /// When the process started (ms since the Unix epoch), if readable.
    pub started: Option<u64>,
    /// `DOMAIN\user` owning the process.
    pub user: String,
    pub command_line: String,
    /// From the version resource of the executable (Windows).
    pub company: String,
    pub description: String,
    /// Windows services running in the process: (service name, display name).
    pub services: Vec<(String, String)>,
    pub icon: Option<Arc<Icon>>,
}

impl ProcessInfo {
    /// The service names joined, e.g. `Dnscache, LanmanWorkstation`.
    pub fn service_list(&self) -> String {
        let names: Vec<&str> = self.services.iter().map(|(n, _)| n.as_str()).collect();
        names.join(", ")
    }
}

#[derive(Default)]
pub struct ProcessCache {
    map: HashMap<u32, Arc<ProcessInfo>>,
    icons: HashMap<String, Option<Arc<Icon>>>,
    services: HashMap<u32, Vec<(String, String)>>,
    services_read: Option<Instant>,
}

impl ProcessCache {
    pub fn get(&mut self, pid: u32) -> Arc<ProcessInfo> {
        let started = platform::start_time(pid);
        if let Some(p) = self.map.get(&pid)
            && p.started == started
        {
            return p.clone();
        }
        let mut info = lookup(pid);
        info.started = started;
        if !info.path.is_empty() {
            info.icon = self
                .icons
                .entry(info.path.clone())
                .or_insert_with(|| platform::icon(&info.path).map(Arc::new))
                .clone();
        }
        info.services = self.services.get(&pid).cloned().unwrap_or_default();
        let info = Arc::new(info);
        self.map.insert(pid, info.clone());
        info
    }

    /// Reads the services per PID again when [`SERVICES_EVERY`] passed, and updates the cached
    /// processes whose services changed.
    pub fn refresh_services(&mut self) {
        if self
            .services_read
            .is_some_and(|t| t.elapsed() < SERVICES_EVERY)
        {
            return;
        }
        self.services_read = Some(Instant::now());
        self.services = platform::services();
        for (pid, info) in self.map.iter_mut() {
            let now = self.services.get(pid).cloned().unwrap_or_default();
            if info.services != now {
                let mut changed = (**info).clone();
                changed.services = now;
                *info = Arc::new(changed);
            }
        }
    }

    /// Forgets PIDs not in `alive`.
    pub fn retain(&mut self, alive: &HashSet<u32>) {
        self.map.retain(|pid, _| alive.contains(pid));
        // Icons of executables that no longer run are dropped too.
        let paths: HashSet<&str> = self.map.values().map(|p| p.path.as_str()).collect();
        self.icons.retain(|path, _| paths.contains(path.as_str()));
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
            ..Default::default()
        };
    }
    let path = platform::image_path(pid).unwrap_or_default();
    let name = if path.is_empty() {
        platform::short_name(pid).unwrap_or_else(|| format!("<{pid}>"))
    } else {
        file_name(&path).to_owned()
    };
    let d = platform::details(pid, &path);
    ProcessInfo {
        name,
        path,
        user: d.user,
        command_line: d.command_line,
        company: d.company,
        description: d.description,
        ..Default::default()
    }
}

#[cfg(windows)]
mod platform {
    pub use crate::winproc::{details, icon, image_path, services, start_time};

    /// Without a path (access denied) Windows gives us no name either.
    pub fn short_name(_pid: u32) -> Option<String> {
        None
    }
}

#[cfg(not(windows))]
mod platform {
    use crate::winproc::{Details, Icon};
    use std::collections::HashMap;

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

    /// Start time from `/proc/<pid>/stat` (field 22, clock ticks after boot, assumed 100 Hz)
    /// plus the boot time from `/proc/stat`.
    pub fn start_time(pid: u32) -> Option<u64> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // The command name (field 2) may contain spaces; fields after it follow ")".
        let rest = &stat[stat.rfind(')')? + 2..];
        let ticks: u64 = rest.split_whitespace().nth(19)?.parse().ok()?;
        let boot: u64 = std::fs::read_to_string("/proc/stat")
            .ok()?
            .lines()
            .find_map(|l| l.strip_prefix("btime ")?.trim().parse().ok())?;
        Some(boot * 1000 + ticks * 10)
    }

    pub fn details(pid: u32, _path: &str) -> Details {
        let command_line = std::fs::read(format!("/proc/{pid}/cmdline"))
            .map(|b| {
                b.split(|&c| c == 0)
                    .filter(|a| !a.is_empty())
                    .map(|a| String::from_utf8_lossy(a).into_owned())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();
        Details {
            user: user(pid).unwrap_or_default(),
            command_line,
            ..Default::default()
        }
    }

    /// The real UID of the process and its name from `/etc/passwd`.
    fn user(pid: u32) -> Option<String> {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
        let uid = status
            .lines()
            .find_map(|l| l.strip_prefix("Uid:"))?
            .split_whitespace()
            .next()?
            .to_owned();
        let passwd = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
        Some(
            passwd
                .lines()
                .find_map(|l| {
                    let f: Vec<&str> = l.split(':').collect();
                    (f.get(2) == Some(&uid.as_str())).then(|| f[0].to_owned())
                })
                .unwrap_or(uid),
        )
    }

    pub fn icon(_path: &str) -> Option<Icon> {
        None
    }

    pub fn services() -> HashMap<u32, Vec<(String, String)>> {
        HashMap::new()
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
        assert!(c.icons.is_empty());
    }

    #[test]
    fn own_process_is_identified_by_start_time() {
        let mut c = ProcessCache::default();
        let own = std::process::id();
        let a = c.get(own);
        assert!(a.started.is_some());
        assert!(!a.user.is_empty() && !a.command_line.is_empty(), "{a:?}");
        // Same process: the same entry.
        assert!(Arc::ptr_eq(&a, &c.get(own)));
        // A different start time (a reused PID) makes a new lookup.
        let mut old = (*a).clone();
        old.started = Some(1);
        c.map.insert(own, Arc::new(old));
        let b = c.get(own);
        assert_eq!(b.started, a.started);
    }

    #[test]
    fn service_list_text() {
        let p = ProcessInfo {
            services: vec![
                ("Dnscache".into(), "DNS Client".into()),
                ("NlaSvc".into(), "Network Location Awareness".into()),
            ],
            ..Default::default()
        };
        assert_eq!(p.service_list(), "Dnscache, NlaSvc");
    }
}
