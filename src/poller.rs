//! Background thread that reads the connection tables and resolves process names, so the UI
//! thread never waits for the system. The UI controls it through [`PollControl`] and receives
//! [`Snapshot`]s over a channel.
//!
//! While [`PollControl::etw`] is set it also runs the ETW session of #4: the events are
//! collected between two refreshes and every snapshot carries what ETW saw before the tables
//! were read ([`Snapshot::etw`]).

use crate::net::estats::Collector;
use crate::net::etw::{self, EtwEvent, EtwStatus, Seen};
use crate::net::{Connection, NetError, Reader};
use crate::process::{ProcessCache, ProcessInfo};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

pub struct Snapshot {
    pub rows: Vec<(Connection, Arc<ProcessInfo>)>,
    /// ms since the Unix epoch.
    pub taken: u64,
    /// How long reading the tables took (shown in the status bar).
    pub took: Duration,
    /// The traffic counters were wanted but Windows refused them (not elevated, #8).
    pub stats_denied: bool,
    /// The endpoints ETW saw since the previous snapshot, before the tables were read (#4).
    pub etw: Vec<(Seen, Arc<ProcessInfo>)>,
    pub etw_status: EtwStatus,
}

pub type PollResult = Result<Snapshot, NetError>;

/// Shared switches between the UI and the poll thread.
pub struct PollControl {
    pub interval_ms: AtomicU64,
    pub paused: AtomicBool,
    /// Set by the UI for an immediate refresh (also while paused).
    pub refresh_now: AtomicBool,
    pub stop: AtomicBool,
    /// Read the traffic counters of the TCP connections (a traffic column is visible, #8).
    pub collect_stats: AtomicBool,
    /// Capture short connections with ETW (#4).
    pub etw: AtomicBool,
    /// The running ETW session; here so that [`PollControl::shutdown`] can stop it from the UI
    /// thread before the process ends (a session outlives its process otherwise).
    etw_session: Mutex<Option<etw::Session>>,
}

impl PollControl {
    /// Ends the poll thread and stops the ETW session at once (the program is closing).
    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::Relaxed);
        self.stop_etw();
    }

    fn stop_etw(&self) {
        let session = self
            .etw_session
            .lock()
            .map(|mut s| s.take())
            .unwrap_or(None);
        if let Some(s) = session {
            s.stop();
        }
    }
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
        collect_stats: AtomicBool::new(false),
        etw: AtomicBool::new(false),
        etw_session: Mutex::new(None),
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
    let mut reader = Reader::default();
    let mut stats = Collector::default();
    let mut etw = Etw::default();
    let mut last: Option<Instant> = None;
    while !ctl.stop.load(Ordering::Relaxed) {
        let interval = Duration::from_millis(ctl.interval_ms.load(Ordering::Relaxed));
        let due = last.is_none_or(|l| l.elapsed() >= interval);
        let now_requested = ctl.refresh_now.swap(false, Ordering::Relaxed);
        let paused = ctl.paused.load(Ordering::Relaxed);
        etw.control(ctl);
        etw.drain();
        if paused && !now_requested {
            // A paused table shows nothing new; the events would only pile up.
            etw.collector.clear();
        }
        if now_requested || (due && !paused) {
            last = Some(Instant::now());
            let collect = ctl.collect_stats.load(Ordering::Relaxed);
            let res = poll_once(
                &mut reader,
                &mut cache,
                collect.then_some(&mut stats),
                Some(&mut etw),
            );
            if tx.send(res).is_err() {
                break; // the UI is gone
            }
            wake();
        }
        // Short sleeps keep pause / interval changes / "refresh now" responsive.
        std::thread::sleep(Duration::from_millis(25));
    }
    ctl.stop_etw();
}

/// The ETW side of the poll thread (#4).
#[derive(Default)]
struct Etw {
    rx: Option<mpsc::Receiver<EtwEvent>>,
    collector: etw::Collector,
    status: EtwStatus,
}

impl Etw {
    /// Starts or stops the session as [`PollControl::etw`] says. A failed start is not
    /// repeated until the switch goes off and on again.
    fn control(&mut self, ctl: &PollControl) {
        let want = ctl.etw.load(Ordering::Relaxed);
        if want && self.status == EtwStatus::Off {
            let (tx, rx) = mpsc::channel();
            match etw::start(tx) {
                Ok(session) => {
                    if let Ok(mut slot) = ctl.etw_session.lock() {
                        *slot = Some(session);
                    }
                    self.rx = Some(rx);
                    self.status = EtwStatus::Running;
                }
                Err(e) => self.status = EtwStatus::Error(e),
            }
        } else if !want && self.status != EtwStatus::Off {
            ctl.stop_etw();
            self.rx = None;
            self.collector.clear();
            self.status = EtwStatus::Off;
        }
    }

    /// Moves the delivered events into the collector.
    fn drain(&mut self) {
        if let Some(rx) = &self.rx {
            while let Ok(e) = rx.try_recv() {
                self.collector.push(e);
            }
        }
    }
}

/// One reading without the poll thread (`--export` of the command line, #16).
pub fn snapshot() -> PollResult {
    poll_once(
        &mut Reader::default(),
        &mut ProcessCache::default(),
        None,
        None,
    )
}

fn poll_once(
    reader: &mut Reader,
    cache: &mut ProcessCache,
    stats: Option<&mut Collector>,
    etw: Option<&mut Etw>,
) -> PollResult {
    let start = Instant::now();
    // ETW events from before the tables were read; later ones belong to the next snapshot.
    let cutoff = crate::model::now_ms();
    let mut conns = reader.read()?;
    let stats_denied = match stats {
        Some(s) => s.fill(&mut conns).is_err(),
        None => false,
    };
    let (seen, etw_status) = match etw {
        Some(e) => {
            e.drain();
            (e.collector.take(cutoff), e.status.clone())
        }
        None => (Vec::new(), EtwStatus::Off),
    };
    let pids: HashSet<u32> = conns
        .iter()
        .map(|c| c.pid)
        .chain(seen.iter().map(|s| s.key.pid))
        .collect();
    cache.retain(&pids);
    cache.refresh_services();
    let rows = conns
        .into_iter()
        .map(|c| {
            let p = cache.get(c.pid);
            (c, p)
        })
        .collect();
    let etw = seen
        .into_iter()
        .map(|s| {
            let p = cache.get(s.key.pid);
            (s, p)
        })
        .collect();
    Ok(Snapshot {
        rows,
        taken: crate::model::now_ms(),
        took: start.elapsed(),
        stats_denied,
        etw,
        etw_status,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Tracker;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};

    /// The acceptance test of #4: 100 connections, each opened and closed within 50 ms, all show
    /// up in the tracker – most only through ETW. Needs an administrator (the Windows CI runs
    /// elevated); skipped otherwise.
    #[cfg(windows)]
    #[test]
    fn etw_sees_short_connections() {
        if !crate::elevation::is_elevated() {
            eprintln!("skipped: ETW needs an administrator");
            return;
        }
        const HOLD: u64 = 600_000;
        let (ctl, rx) = start(500, || {});
        ctl.etw.store(true, Ordering::Relaxed);
        let mut tracker = Tracker::default();
        let merge = |tracker: &mut Tracker, timeout: Duration| -> Option<EtwStatus> {
            let snap = rx.recv_timeout(timeout).ok()?.ok()?;
            tracker.update_with_events(snap.rows, snap.taken, HOLD, None);
            tracker.merge_etw(snap.etw, snap.taken, HOLD, None);
            Some(snap.etw_status)
        };
        // Running, and two snapshots merged (ETW rows come from the second one on).
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut running = 0;
        while running < 2 && Instant::now() < deadline {
            match merge(&mut tracker, Duration::from_secs(2)) {
                Some(EtwStatus::Running) => running += 1,
                Some(EtwStatus::Error(e)) => panic!("ETW did not start: {e}"),
                _ => {}
            }
        }
        assert_eq!(running, 2, "no snapshots with ETW running");

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let server = listener.local_addr().unwrap().port();
        let accept = std::thread::spawn(move || {
            for _ in 0..100 {
                let (mut s, _) = listener.accept().unwrap();
                let mut b = [0u8; 1];
                let _ = s.read(&mut b);
            }
        });
        let mut clients = Vec::new();
        for _ in 0..100 {
            let t = Instant::now();
            let mut s = TcpStream::connect(("127.0.0.1", server)).unwrap();
            clients.push(s.local_addr().unwrap().port());
            s.write_all(b"x").unwrap();
            drop(s);
            assert!(t.elapsed() < Duration::from_millis(50));
        }
        accept.join().unwrap();
        // ETW delivers its buffers about once a second.
        let until = Instant::now() + Duration::from_secs(4);
        while Instant::now() < until {
            merge(&mut tracker, Duration::from_millis(600));
        }
        ctl.shutdown();

        let me = std::process::id();
        let missing: Vec<u16> = clients
            .iter()
            .copied()
            .filter(|&p| {
                !tracker.rows().any(|r| {
                    r.conn.local.port() == p && r.conn.remote.map(|a| a.port()) == Some(server)
                })
            })
            .collect();
        assert!(missing.is_empty(), "client ports never seen: {missing:?}");
        // The client side (TimeWait with PID 0 in the table) and the server side: by ETW.
        let etw_client = tracker
            .rows()
            .filter(|r| {
                r.etw && r.conn.pid == me && r.conn.remote.map(|a| a.port()) == Some(server)
            })
            .count();
        let etw_server = tracker
            .rows()
            .filter(|r| r.etw && r.conn.pid == me && r.conn.local.port() == server)
            .count();
        eprintln!("ETW rows: {etw_client} client, {etw_server} server");
        // Every client sent one byte and received none: a receive belongs to the server side.
        let received = |client: bool| {
            tracker
                .rows()
                .filter(|r| r.etw && r.conn.pid == me)
                .filter(|r| (r.conn.local.port() == server) != client)
                .filter_map(|r| r.conn.stats.map(|s| s.bytes_in))
                .sum::<u64>()
        };
        eprintln!(
            "ETW bytes received: {} client, {} server",
            received(true),
            received(false)
        );
        assert_eq!(received(true), 0, "receives taken for the client side");
        assert!(
            etw_client + etw_server >= 100,
            "{etw_client} + {etw_server}"
        );
    }
}
