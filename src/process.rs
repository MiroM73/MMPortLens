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

pub use crate::winproc::{Icon, IoCounters};

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
    /// PID → executable name of every process (Windows: one Toolhelp snapshot), taken at most
    /// once per refresh and only when a process without a readable path needs it (#37).
    names: Option<HashMap<u32, String>>,
}

impl ProcessCache {
    pub fn get(&mut self, pid: u32) -> Arc<ProcessInfo> {
        let started = platform::start_time(pid);
        if let Some(p) = self.map.get(&pid).cloned()
            && p.started == started
        {
            // Without access the start time is unknown, so a reused PID is told apart by the
            // name in the process list instead.
            let unchecked = started.is_none() && p.path.is_empty() && special_name(pid).is_none();
            if !unchecked || self.name_of(pid).is_none_or(|n| n == p.name) {
                return p;
            }
        }
        let mut info = lookup(pid, |pid| self.name_of(pid));
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

    /// The executable name of `pid` from the process list of this refresh.
    fn name_of(&mut self, pid: u32) -> Option<String> {
        self.names
            .get_or_insert_with(platform::process_names)
            .get(&pid)
            .cloned()
    }

    /// Forgets PIDs not in `alive`. Called once per refresh: the process list is taken again
    /// when needed.
    pub fn retain(&mut self, alive: &HashSet<u32>) {
        self.names = None;
        self.map.retain(|pid, _| alive.contains(pid));
        // Icons of executables that no longer run are dropped too.
        let paths: HashSet<&str> = self.map.values().map(|p| p.path.as_str()).collect();
        self.icons.retain(|path, _| paths.contains(path.as_str()));
    }
}

/// When the process `pid` started (ms since the Unix epoch); `None` when it does not exist or
/// cannot be opened. With the PID it identifies the process (properties window, #28).
pub fn start_time(pid: u32) -> Option<u64> {
    platform::start_time(pid)
}

/// The I/O counters of the process `pid` (graph of the properties window, #30); `None` when it
/// does not exist or cannot be opened.
pub fn io_counters(pid: u32) -> Option<IoCounters> {
    platform::io_counters(pid)
}

/// `/proc/<pid>/io` → counters: `syscr` / `syscw` are the read / write operations, `rchar` /
/// `wchar` their bytes (Linux has no "other" operations).
#[cfg_attr(windows, allow(dead_code))]
pub fn parse_proc_io(text: &str) -> Option<IoCounters> {
    let field = |name: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(name)?.strip_prefix(':')?.trim().parse().ok())
    };
    Some(IoCounters {
        read_ops: field("syscr")?,
        write_ops: field("syscw")?,
        read_bytes: field("rchar")?,
        write_bytes: field("wchar")?,
        ..Default::default()
    })
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

/// The details of `pid`; `name_of` gives the name of a process whose path cannot be read.
fn lookup(pid: u32, name_of: impl FnOnce(u32) -> Option<String>) -> ProcessInfo {
    if let Some(name) = special_name(pid) {
        return ProcessInfo {
            name: name.into(),
            ..Default::default()
        };
    }
    let path = platform::image_path(pid).unwrap_or_default();
    let name = if path.is_empty() {
        name_of(pid).unwrap_or_else(|| format!("<{pid}>"))
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
    pub use crate::winproc::{details, icon, image_path, io_counters, services, start_time};
    use std::collections::HashMap;

    /// PID → executable name of every process. Windows gives the names of other users' and
    /// protected processes, whose path needs administrator rights (#37).
    pub fn process_names() -> HashMap<u32, String> {
        crate::winproc::process_list()
            .into_iter()
            .map(|e| (e.pid, e.exe))
            .collect()
    }
}

#[cfg(not(windows))]
mod platform {
    use crate::winproc::{Details, Icon, IoCounters};
    use std::collections::HashMap;

    pub fn io_counters(pid: u32) -> Option<IoCounters> {
        super::parse_proc_io(&std::fs::read_to_string(format!("/proc/{pid}/io")).ok()?)
    }

    pub fn image_path(pid: u32) -> Option<String> {
        std::fs::read_link(format!("/proc/{pid}/exe"))
            .ok()
            .map(|p| p.to_string_lossy().into_owned())
    }

    /// PID → command name (`/proc/<pid>/comm`) of every process.
    pub fn process_names() -> HashMap<u32, String> {
        let Ok(dir) = std::fs::read_dir("/proc") else {
            return HashMap::new();
        };
        dir.flatten()
            .filter_map(|e| {
                let pid: u32 = e.file_name().to_str()?.parse().ok()?;
                let comm = std::fs::read_to_string(e.path().join("comm")).ok()?;
                Some((pid, comm.trim().to_owned()))
            })
            .collect()
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
    fn proc_io_parsing() {
        let text = "rchar: 3000
wchar: 400
syscr: 12
syscw: 5
read_bytes: 0
";
        let io = parse_proc_io(text).unwrap();
        assert_eq!((io.read_ops, io.write_ops, io.other_ops), (12, 5, 0));
        assert_eq!((io.read_bytes, io.write_bytes), (3000, 400));
        assert_eq!(
            parse_proc_io(
                "rchar: 1
"
            ),
            None
        );
    }

    #[test]
    fn own_io_counters_are_readable() {
        assert!(io_counters(std::process::id()).is_some());
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
