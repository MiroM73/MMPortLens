//! The threads of the recording (#3): the writer thread owns the database while recording and
//! receives one batch of events per refresh; the replay worker owns an opened recording and
//! answers the queries of the replay (state at a time, pages of the event list). Neither ever
//! runs SQL on the UI thread.

use super::Event;
use super::db::{EventQuery, Info, RecordingReader, RecordingWriter};
use crate::model::Row;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;

/// A running recording.
pub struct Recorder {
    tx: Option<mpsc::Sender<Vec<Event>>>,
    thread: Option<JoinHandle<Result<u64, String>>>,
    /// Events written so far (status bar).
    pub written: Arc<AtomicU64>,
    pub path: PathBuf,
}

impl Recorder {
    /// Starts the writer thread: creates `path`, writes `meta` and the starting state `first`.
    /// `wake` is called when the thread ends (an error stops the recording).
    pub fn start(
        path: PathBuf,
        meta: Vec<(String, String)>,
        first: Vec<Event>,
        wake: impl Fn() + Send + 'static,
    ) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel::<Vec<Event>>();
        let written = Arc::new(AtomicU64::new(0));
        let (count, file) = (written.clone(), path.clone());
        let thread = std::thread::Builder::new()
            .name("mmportlens-record".into())
            .spawn(move || {
                let res = (|| {
                    let mut w = RecordingWriter::create(&file)?;
                    for (k, v) in &meta {
                        w.set_meta(k, v)?;
                    }
                    w.write(&first)?;
                    count.store(w.events(), Ordering::Relaxed);
                    while let Ok(batch) = rx.recv() {
                        w.write(&batch)?;
                        count.store(w.events(), Ordering::Relaxed);
                    }
                    w.finish(crate::model::now_ms())
                })()
                .map_err(|e| e.to_string());
                wake();
                res
            })?;
        Ok(Self {
            tx: Some(tx),
            thread: Some(thread),
            written,
            path,
        })
    }

    /// Hands the events of one refresh to the writer thread.
    pub fn send(&self, events: Vec<Event>) {
        if let Some(tx) = &self.tx {
            // A thread that ended with an error is reported by `finished`.
            let _ = tx.send(events);
        }
    }

    /// Ends the recording: the writer writes what it has, finishes the file and ends.
    pub fn stop(&mut self) {
        self.tx = None;
    }

    pub fn stopping(&self) -> bool {
        self.tx.is_none()
    }

    /// The result once the writer thread ended (after [`Recorder::stop`] or an error): the
    /// number of events written.
    pub fn finished(&mut self) -> Option<Result<u64, String>> {
        if !self.thread.as_ref()?.is_finished() {
            return None;
        }
        Some(self.join())
    }

    /// Stops and waits for the writer (the program is closing).
    pub fn wait(mut self) -> Result<u64, String> {
        self.stop();
        self.join()
    }

    fn join(&mut self) -> Result<u64, String> {
        match self.thread.take().map(|t| t.join()) {
            Some(Ok(res)) => res,
            Some(Err(_)) => Err("the recording thread crashed".into()),
            None => Err("the recording already ended".into()),
        }
    }
}

/// Events per page of the event list.
pub const PAGE: u64 = 200;

/// A question to the replay worker.
pub enum Request {
    /// The endpoints at the time `at` (rows marked `hold` ms after their event).
    State { at: u64, hold: u64 },
    /// One page of the event list; `generation` changes with the query.
    Events {
        generation: u64,
        query: EventQuery,
        page: u64,
    },
}

/// An answer of the replay worker.
pub enum Reply {
    Opened(Result<Info, String>),
    State {
        at: u64,
        rows: Vec<Row>,
        prev: Option<u64>,
        next: Option<u64>,
    },
    Events {
        generation: u64,
        page: u64,
        total: u64,
        events: Vec<Event>,
    },
    Error(String),
}

/// The thread that reads an opened recording.
pub struct ReplayWorker {
    tx: mpsc::Sender<Request>,
    pub rx: mpsc::Receiver<Reply>,
}

impl ReplayWorker {
    /// Opens `path` in a new thread; the first reply is [`Reply::Opened`].
    pub fn open(path: PathBuf, wake: impl Fn() + Send + 'static) -> std::io::Result<Self> {
        let (tx, requests) = mpsc::channel();
        let (replies, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("mmportlens-replay".into())
            .spawn(move || run(&path, &requests, &replies, &wake))?;
        Ok(Self { tx, rx })
    }

    pub fn ask(&self, r: Request) {
        let _ = self.tx.send(r);
    }
}

fn run(
    path: &std::path::Path,
    requests: &mpsc::Receiver<Request>,
    replies: &mpsc::Sender<Reply>,
    wake: &dyn Fn(),
) {
    let send = |r: Reply| {
        let ok = replies.send(r).is_ok();
        wake();
        ok
    };
    let mut reader = match RecordingReader::open(path).and_then(|r| Ok((r.info()?, r))) {
        Ok((info, r)) => {
            if !send(Reply::Opened(Ok(info))) {
                return;
            }
            r
        }
        Err(e) => {
            send(Reply::Opened(Err(e.to_string())));
            return;
        }
    };
    // The count of the current event query (asked once per generation).
    let mut counted: Option<(u64, u64)> = None;
    while let Ok(first) = requests.recv() {
        // Everything queued: only the newest state counts (a dragged slider), and only the
        // pages of the newest query.
        let mut queued = vec![first];
        queued.extend(requests.try_iter());
        let state = queued.iter().rev().find_map(|r| match r {
            Request::State { at, hold } => Some((*at, *hold)),
            _ => None,
        });
        let newest = queued
            .iter()
            .filter_map(|r| match r {
                Request::Events { generation, .. } => Some(*generation),
                _ => None,
            })
            .max();
        let mut pages: Vec<(u64, EventQuery)> = Vec::new();
        for r in queued {
            if let Request::Events {
                generation,
                query,
                page,
            } = r
                && Some(generation) == newest
                && !pages.iter().any(|(p, _)| *p == page)
            {
                pages.push((page, query));
            }
        }
        if let Some((at, hold)) = state {
            let reply = reader
                .state_at(at, hold)
                .and_then(|rows| {
                    let (prev, next) = reader.neighbours(at)?;
                    Ok(Reply::State {
                        at,
                        rows,
                        prev,
                        next,
                    })
                })
                .unwrap_or_else(|e| Reply::Error(e.to_string()));
            if !send(reply) {
                return;
            }
        }
        let Some(generation) = newest else { continue };
        for (page, query) in pages {
            let total = match counted {
                Some((g, n)) if g == generation => Ok(n),
                _ => reader.count(&query),
            };
            let reply = total
                .and_then(|total| {
                    counted = Some((generation, total));
                    let events = reader.events(&query, page * PAGE, PAGE)?;
                    Ok(Reply::Events {
                        generation,
                        page,
                        total,
                        events,
                    })
                })
                .unwrap_or_else(|e| Reply::Error(e.to_string()));
            if !send(reply) {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::{Connection, Protocol};
    use crate::process::ProcessInfo;
    use crate::recording::EventKind;
    use std::time::Duration;

    fn event(time: u64, kind: EventKind, port: u16) -> Event {
        Event {
            time,
            kind,
            conn: Connection {
                proto: Protocol::Udp,
                local: format!("127.0.0.1:{port}").parse().unwrap(),
                remote: None,
                state: None,
                pid: 1,
                created: None,
                module: None,
                stats: None,
            },
            state_before: None,
            process: Arc::new(ProcessInfo::default()),
        }
    }

    /// The writer thread writes the starting state and the batches; the replay worker reads
    /// them back (state and event pages).
    #[test]
    fn record_and_replay_through_the_threads() {
        let dir = std::env::temp_dir().join(format!("mmportlens-thr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.plrec");
        let rec = Recorder::start(
            path.clone(),
            vec![("interval_ms".into(), "1000".into())],
            vec![event(1000, EventKind::Present, 1)],
            || {},
        )
        .unwrap();
        rec.send(vec![event(2000, EventKind::New, 2)]);
        rec.send(vec![event(3000, EventKind::Closed, 1)]);
        assert_eq!(rec.wait(), Ok(3));

        let w = ReplayWorker::open(path, || {}).unwrap();
        let wait = || w.rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let Reply::Opened(Ok(info)) = wait() else {
            panic!("not opened")
        };
        assert_eq!(
            (info.events, info.first, info.last),
            (3, Some(1000), Some(3000))
        );
        assert_eq!(info.meta("interval_ms"), Some("1000"));
        w.ask(Request::State {
            at: 2500,
            hold: 100,
        });
        let Reply::State {
            rows, prev, next, ..
        } = wait()
        else {
            panic!("no state")
        };
        assert_eq!(rows.len(), 2);
        assert_eq!((prev, next), (Some(2000), Some(3000)));
        w.ask(Request::Events {
            generation: 1,
            query: EventQuery::default(),
            page: 0,
        });
        let Reply::Events { total, events, .. } = wait() else {
            panic!("no events")
        };
        assert_eq!((total, events.len()), (3, 3));
        assert_eq!(events[2].kind, EventKind::Closed);
        drop(w);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn opening_a_missing_file_fails() {
        let w = ReplayWorker::open(PathBuf::from("does/not/exist.plrec"), || {}).unwrap();
        let reply = w.rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(matches!(reply, Reply::Opened(Err(_))));
    }
}
