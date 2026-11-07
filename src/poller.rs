//! Background thread that reads the connection tables and resolves process names, so the UI
//! thread never waits for the system. The UI controls it through [`PollControl`] and receives
//! [`Snapshot`]s over a channel.

use crate::net::{self, Connection, NetError};
use crate::process::{ProcessCache, ProcessInfo};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

pub struct Snapshot {
    pub rows: Vec<(Connection, Arc<ProcessInfo>)>,
    /// ms since the Unix epoch.
    pub taken: u64,
    /// How long reading the tables took (shown in the status bar).
    pub took: Duration,
}

pub type PollResult = Result<Snapshot, NetError>;

/// Shared switches between the UI and the poll thread.
pub struct PollControl {
    pub interval_ms: AtomicU64,
    pub paused: AtomicBool,
    /// Set by the UI for an immediate refresh (also while paused).
    pub refresh_now: AtomicBool,
    pub stop: AtomicBool,
}

/// Starts the thread; `wake` is called after every snapshot (the UI's repaint request).
pub fn start(
    interval_ms: u64,
    wake: impl Fn() + Send + 'static,
) -> (Arc<PollControl>, mpsc::Receiver<PollResult>) {
    let ctl = Arc::new(PollControl {
        interval_ms: AtomicU64::new(interval_ms),
        paused: AtomicBool::new(false),
        refresh_now: AtomicBool::new(true),
        stop: AtomicBool::new(false),
    });
    let (tx, rx) = mpsc::channel();
    let c = ctl.clone();
    std::thread::Builder::new()
        .name("poll".into())
        .spawn(move || run(&c, &tx, wake))
        .expect("starting the poll thread");
    (ctl, rx)
}

fn run(ctl: &PollControl, tx: &mpsc::Sender<PollResult>, wake: impl Fn()) {
    let mut cache = ProcessCache::default();
    let mut last: Option<Instant> = None;
    while !ctl.stop.load(Ordering::Relaxed) {
        let interval = Duration::from_millis(ctl.interval_ms.load(Ordering::Relaxed));
        let due = last.is_none_or(|l| l.elapsed() >= interval);
        let now_requested = ctl.refresh_now.swap(false, Ordering::Relaxed);
        if now_requested || (due && !ctl.paused.load(Ordering::Relaxed)) {
            last = Some(Instant::now());
            let res = poll_once(&mut cache);
            if tx.send(res).is_err() {
                return; // the UI is gone
            }
            wake();
        }
        // Short sleeps keep pause / interval changes / "refresh now" responsive.
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn poll_once(cache: &mut ProcessCache) -> PollResult {
    let start = Instant::now();
    let conns = net::snapshot()?;
    let pids: HashSet<u32> = conns.iter().map(|c| c.pid).collect();
    cache.retain(&pids);
    cache.refresh_services();
    let rows = conns
        .into_iter()
        .map(|c| {
            let p = cache.get(c.pid);
            (c, p)
        })
        .collect();
    Ok(Snapshot {
        rows,
        taken: crate::model::now_ms(),
        took: start.elapsed(),
    })
}
