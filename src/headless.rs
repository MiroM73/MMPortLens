//! The command line without a window (#16): `--export <file>` writes one snapshot of the
//! endpoints, `--record <file> [--duration <time>]` records connection events until the time is
//! up or Ctrl+C. Both use the filters of the command line (`--preset`, `--filter`, `--column`;
//! without them every endpoint), the visible columns and the sort order of the table, and the
//! saved settings of File → Export… / File → Start recording…. Messages go to the console; the
//! caller turns the result into the exit code.

use crate::columns::ColumnId;
use crate::config::Config;
use crate::model::{Row, Tracker, now_ms};
use crate::net::etw::{EtwStatus, error_text as etw_error_text};
use crate::poller;
use crate::presets::FilterPreset;
use crate::recording::Event;
use crate::recording::threads::Recorder;
use crate::rowfilter::RowFilter;
use crate::t;
use crate::tabexport::{self, ColumnSpec, Control, TableFormat};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

/// How long `--export` waits for the host names of the addresses (#5).
const DNS_WAIT: Duration = Duration::from_secs(5);
/// Parallel reverse DNS lookups of `--export`.
const DNS_THREADS: usize = 16;

/// The visible columns of the table, with the columns that have a filter shown (like choosing a
/// preset in the window: the filters of hidden columns do not apply).
fn visible_columns(cfg: &Config, filters: Option<&FilterPreset>) -> Vec<ColumnId> {
    let mut layout = cfg.columns.clone();
    for (id, text) in filters.map_or(&[][..], |p| &p.column_filters) {
        if !text.trim().is_empty() {
            layout.set_visible(*id, true);
        }
    }
    layout.visible()
}

/// The filter of the command line (`None` = every endpoint).
fn row_filter(
    filters: Option<&FilterPreset>,
    cols: &[ColumnId],
) -> Result<Option<RowFilter>, String> {
    filters
        .map(|p| RowFilter::from_preset(p, cols.to_vec()))
        .transpose()
        .map_err(|(id, e)| format!("{}: {}", id.title(), e.text()))
}

/// Writes one snapshot of the endpoints to `target`; returns the message for the console.
pub fn export(
    target: &Path,
    filters: Option<&FilterPreset>,
    cfg: &Config,
) -> Result<String, String> {
    let format = TableFormat::from_path(target)
        .ok_or_else(|| t!("cli.error.export_format", file = target.display()))?;
    let snap = poller::snapshot().map_err(|e| t!("error.poll", error = e))?;
    let mut tracker = Tracker::default();
    tracker.update_with_events(snap.rows, snap.taken, cfg.highlight_ms, None);
    let mut rows: Vec<Row> = tracker.rows().cloned().collect();

    let visible = visible_columns(cfg, filters);
    let s = &cfg.export;
    let all: Vec<ColumnId> = cfg.columns.0.iter().map(|c| c.id).collect();
    let cols = tabexport::export_columns(s.columns, &visible, &all);
    // The country / ASN database (#11), only when one of its columns is exported or filtered.
    let geo = |c: &ColumnId| matches!(c, ColumnId::Country | ColumnId::Asn);
    if cfg.geo.any()
        && (cols.iter().any(geo) || visible.iter().any(geo))
        && let Err(e) = crate::geo::load_now(&cfg.geo)
    {
        crate::console::err(&t!("geo.error", error = e));
    }
    let hosts = |c: &ColumnId| matches!(c, ColumnId::RemoteHost | ColumnId::LocalHost);
    if cfg.resolve_addresses && (visible.iter().any(hosts) || cols.iter().any(hosts)) {
        let local = visible.contains(&ColumnId::LocalHost) || cols.contains(&ColumnId::LocalHost);
        resolve_host_names(&mut rows, local);
    }
    let filter = row_filter(filters, &visible)?;
    if let Some(f) = &filter {
        rows.retain(|r| f.matches(r));
    }
    let sort = cfg.sort;
    rows.sort_by(|a, b| sort.compare(a, b));

    let looks = if s.colors {
        let compiled = crate::rules::Compiled::new(&cfg.color_rules);
        rows.iter()
            .map(|r| {
                crate::rules::export_look(
                    r,
                    &cfg.color_rules,
                    &compiled,
                    &visible,
                    cfg.marks_over_rules,
                )
            })
            .collect()
    } else {
        Vec::new()
    };
    let now = snap.taken;
    let mut html_info = vec![
        t!("export.html.time", time = crate::timefmt::date_time(now)),
        t!("export.html.rows", rows = rows.len()),
    ];
    if let Some(p) = filters {
        let d = p.description();
        if !d.is_empty() {
            html_info.push(t!("export.html.filters", filters = d));
        }
    }
    let sep = s
        .csv_separator
        .unwrap_or_else(tabexport::csv::system_list_separator);
    let job = tabexport::Job {
        looks,
        cols: cols.into_iter().map(ColumnSpec::new).collect(),
        format,
        header: s.header,
        now,
        csv: tabexport::csv::CsvOptions::new(sep, s.csv_encoding, s.safe_for_excel),
        xlsx: tabexport::xlsx::XlsxOptions {
            colors: s.colors,
            sheet_name: t!("export.sheet"),
            ..Default::default()
        },
        html_title: t!("export.html.title", host = tabexport::computer_name()),
        html_info,
        html_colors: s.colors,
        rows,
    };
    let (n, cut) = tabexport::run(&job, target, &Control::default())
        .map_err(|e| t!("export.error", error = e))?;
    Ok(if cut > 0 {
        t!(
            "export.done.truncated",
            rows = n,
            path = target.display(),
            cells = cut
        )
    } else {
        t!("export.done", rows = n, path = target.display())
    })
}

/// Looks up the host names of the remote (and with `local` the local) addresses, in parallel,
/// waiting at most [`DNS_WAIT`]; names that come later are left out.
fn resolve_host_names(rows: &mut [Row], local: bool) {
    let mut ips: Vec<IpAddr> = rows
        .iter()
        .flat_map(|r| {
            let l = local.then_some(r.conn.local.ip());
            r.conn.remote.map(|a| a.ip()).into_iter().chain(l)
        })
        .filter(|ip| crate::dns::resolvable(*ip))
        .collect();
    ips.sort();
    ips.dedup();
    if ips.is_empty() {
        return;
    }
    let total = ips.len();
    let queue = Arc::new(Mutex::new(ips));
    let (tx, rx) = mpsc::channel();
    for _ in 0..DNS_THREADS.min(total) {
        let (queue, tx) = (queue.clone(), tx.clone());
        // Threads still waiting for the resolver at the deadline end with the program.
        let _ = std::thread::Builder::new()
            .name("mmportlens-dns".into())
            .spawn(move || {
                loop {
                    let Some(ip) = queue.lock().ok().and_then(|mut q| q.pop()) else {
                        return;
                    };
                    if tx.send((ip, crate::dns::system_lookup(ip))).is_err() {
                        return;
                    }
                }
            });
    }
    drop(tx);
    let deadline = Instant::now() + DNS_WAIT;
    let mut names: std::collections::HashMap<IpAddr, Option<Arc<str>>> = Default::default();
    while names.len() < total {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok((ip, name)) => {
                names.insert(ip, name.map(Arc::from));
            }
            Err(_) => break,
        }
    }
    let name = |ip: IpAddr| names.get(&ip).cloned().flatten();
    for r in rows {
        r.remote_host = r.conn.remote.and_then(|a| name(a.ip()));
        if local {
            r.local_host = name(r.conn.local.ip());
        }
    }
}

/// Set by Ctrl+C (or closing the console): the recording stops.
static STOP: AtomicBool = AtomicBool::new(false);
/// The recording file is finished (the console handler may let the program end).
static DONE: AtomicBool = AtomicBool::new(false);

/// Ctrl+C, Ctrl+Break and closing the console stop the recording instead of killing the
/// program, so the file is finished properly.
#[cfg(windows)]
fn catch_ctrl_c() {
    type Handler = unsafe extern "system" fn(u32) -> i32;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetConsoleCtrlHandler(handler: Option<Handler>, add: i32) -> i32;
    }
    unsafe extern "system" fn handler(kind: u32) -> i32 {
        STOP.store(true, Ordering::SeqCst);
        // Close / logoff / shutdown end the process when the handler returns (Windows allows
        // ~5 s): wait until the writer has finished the file.
        const CTRL_CLOSE_EVENT: u32 = 2;
        if kind >= CTRL_CLOSE_EVENT {
            let start = Instant::now();
            while !DONE.load(Ordering::SeqCst) && start.elapsed() < Duration::from_millis(4500) {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        1
    }
    // SAFETY: `handler` is a plain function that lives for the whole program and only touches
    // atomics; registering it has no other effect. `None, 0` clears an "ignore Ctrl+C" flag
    // inherited from the process that started this one (#44: found when a test sent Ctrl+C to a
    // console whose processes had inherited it – the recording went on).
    unsafe {
        SetConsoleCtrlHandler(None, 0);
        SetConsoleCtrlHandler(Some(handler), 1);
    }
}

#[cfg(not(windows))]
fn catch_ctrl_c() {}

/// `target` with the `.plrec` extension when it has none.
pub fn recording_path(target: &Path) -> PathBuf {
    if target.extension().is_some() {
        target.to_path_buf()
    } else {
        target.with_extension(crate::recording::EXTENSION)
    }
}

/// `1 h 30 min`-style text of a duration for the start message.
fn duration_text(ms: u64) -> String {
    let s = ms / 1000;
    let (h, m, sec) = (s / 3600, s / 60 % 60, s % 60);
    let mut parts = Vec::new();
    if h > 0 {
        parts.push(format!("{h} h"));
    }
    if m > 0 {
        parts.push(format!("{m} min"));
    }
    if sec > 0 || parts.is_empty() {
        parts.push(format!("{sec} s"));
    }
    parts.join(" ")
}

/// Records connection events into `target` until `duration` (ms) is up or Ctrl+C, with `etw`
/// also the connections shorter than a refresh (#4); returns the message for the console.
pub fn record(
    target: &Path,
    duration: Option<u64>,
    filters: Option<&FilterPreset>,
    interval_ms: u64,
    etw: bool,
    cfg: &Config,
) -> Result<String, String> {
    let path = recording_path(target);
    let visible = visible_columns(cfg, filters);
    let filter = row_filter(filters, &visible)?;
    let kinds = cfg.record.kinds;
    let wanted = |e: &Event| filter.as_ref().is_none_or(|f| f.matches_event(e));
    catch_ctrl_c();

    let (ctl, rx) = poller::start(interval_ms, || {});
    ctl.etw.store(etw, Ordering::Relaxed);
    // Also stops the ETW session: it would outlive the process.
    let stop_poller = || ctl.shutdown();
    // The starting state: the first snapshot.
    let snap = loop {
        if STOP.load(Ordering::SeqCst) {
            stop_poller();
            return Err(t!("cli.record.cancelled"));
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(snap)) => break snap,
            Ok(Err(e)) => {
                stop_poller();
                return Err(t!("error.poll", error = e));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(t!("record.error", error = "the poll thread ended"));
            }
        }
    };
    let etw_meta = if snap.etw_status == EtwStatus::Running {
        "on"
    } else {
        "off"
    };
    let mut tracker = Tracker::default();
    tracker.update_with_events(snap.rows, snap.taken, cfg.highlight_ms, None);
    let start = snap.taken;
    let first: Vec<Event> = tracker
        .present_events(start)
        .into_iter()
        .filter(|e| wanted(e))
        .collect();
    let mut meta = vec![
        ("computer".to_string(), tabexport::computer_name()),
        ("start_ms".into(), start.to_string()),
        ("interval_ms".into(), interval_ms.to_string()),
        ("app_version".into(), env!("CARGO_PKG_VERSION").to_string()),
        ("events".into(), kinds.text()),
        // What runs, not what was asked for: a standard user gets no session.
        ("etw".into(), etw_meta.into()),
    ];
    if let Some(d) = duration {
        meta.push(("duration_ms".into(), d.to_string()));
    }
    if let Some(d) = filters
        .map(FilterPreset::description)
        .filter(|d| !d.is_empty())
    {
        meta.push(("filter".into(), d));
    }
    let mut recorder = match Recorder::start(path.clone(), meta, first, || {}) {
        Ok(r) => r,
        Err(e) => {
            stop_poller();
            return Err(t!("record.error", error = e));
        }
    };
    crate::console::out(&match duration {
        Some(d) => t!(
            "cli.record.started_for",
            path = path.display(),
            duration = duration_text(d)
        ),
        None => t!("cli.record.started", path = path.display()),
    });
    let until = duration.map(|d| start + d);
    let mut failed = None;
    let mut etw_warned = false;
    while !STOP.load(Ordering::SeqCst) && until.is_none_or(|u| now_ms() < u) {
        // The writer ended by itself: a write error.
        if let Some(res) = recorder.finished() {
            failed = Some(res.err().unwrap_or_else(|| "the recording ended".into()));
            break;
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(snap)) => {
                let mut events = Vec::new();
                tracker.update_with_events(
                    snap.rows,
                    snap.taken,
                    cfg.highlight_ms,
                    Some(&mut events),
                );
                tracker.merge_etw(snap.etw, snap.taken, cfg.highlight_ms, Some(&mut events));
                if let EtwStatus::Error(e) = &snap.etw_status
                    && !etw_warned
                {
                    crate::console::err(&etw_error_text(e));
                    etw_warned = true;
                }
                let events: Vec<Event> = events
                    .into_iter()
                    .filter(|e| kinds.wants(e.kind) && wanted(e))
                    .collect();
                if !events.is_empty() {
                    recorder.send(events);
                }
            }
            // A failed refresh is skipped, like in the window.
            Ok(Err(e)) => crate::console::err(&t!("error.poll", error = e)),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                failed = Some("the poll thread ended".into());
                break;
            }
        }
        tracker.expire(now_ms());
    }
    stop_poller();
    let res = match failed {
        Some(e) => Err(t!("record.error", error = e)),
        None => recorder
            .wait()
            .map(|n| t!("record.done", events = n, path = path.display()))
            .map_err(|e| t!("record.error", error = e)),
    };
    DONE.store(true, Ordering::SeqCst);
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_and_texts() {
        assert_eq!(recording_path(Path::new("a")), PathBuf::from("a.plrec"));
        assert_eq!(recording_path(Path::new("a.db")), PathBuf::from("a.db"));
        assert_eq!(duration_text(600_000), "10 min");
        assert_eq!(duration_text(5_430_000), "1 h 30 min 30 s");
        assert_eq!(duration_text(0), "0 s");
    }

    #[test]
    fn columns_with_filters_are_shown() {
        let cfg = Config::default();
        let hidden = ColumnId::ALL
            .into_iter()
            .find(|c| !cfg.columns.visible().contains(c))
            .expect("a hidden column by default");
        let p = FilterPreset {
            column_filters: vec![(hidden, "x".into())],
            ..Default::default()
        };
        assert!(!visible_columns(&cfg, None).contains(&hidden));
        assert!(visible_columns(&cfg, Some(&p)).contains(&hidden));
    }

    /// A real snapshot of this machine exported as JSON Lines (only the endpoints of this test
    /// process, found by a listener it opens).
    #[test]
    fn export_of_a_live_snapshot() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let dir = std::env::temp_dir().join(format!("mmportlens_headless_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut cfg = Config {
            resolve_addresses: false,
            ..Default::default()
        };
        cfg.export.columns = tabexport::ColumnSet::All;
        let p = FilterPreset {
            column_filters: vec![
                (ColumnId::LocalPort, format!("={port}")),
                (ColumnId::Pid, format!("={}", std::process::id())),
            ],
            ..Default::default()
        };
        let out = dir.join("x.jsonl");
        let msg = export(&out, Some(&p), &cfg).unwrap();
        assert!(msg.contains('1'), "{msg}");
        let text = std::fs::read_to_string(&out).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 1, "{text}");
        assert!(
            lines[0].contains(&format!("\"local_port\":{port}")),
            "{}",
            lines[0]
        );
        assert!(export(&dir.join("x.pdf"), None, &cfg).is_err());
        std::fs::remove_dir_all(&dir).ok();
        drop(listener);
    }
}
