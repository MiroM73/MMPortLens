//! Tests of the recording database (#3): write → read round trips on an in-memory database,
//! the replay state, the event list, the schema check and a busy hour (ignored).
use super::*;

fn event(time: u64, kind: EventKind, port: u16, state: Option<TcpState>, name: &str) -> Event {
    Event {
        time,
        kind,
        conn: Connection {
            proto: Protocol::Tcp,
            local: format!("10.0.0.1:{port}").parse().unwrap(),
            remote: Some("[2001:db8::1]:443".parse().unwrap()),
            state,
            pid: 4321,
            created: Some(time - 5),
            module: None,
            stats: None,
        },
        state_before: match kind {
            EventKind::Changed => Some(TcpState::Established),
            EventKind::Closed => state.or(Some(TcpState::TimeWait)),
            _ => None,
        },
        process: Arc::new(ProcessInfo {
            name: name.into(),
            path: format!("C:\\Apps\\{name}"),
            started: Some(100),
            ..Default::default()
        }),
    }
}

fn closed(time: u64, port: u16) -> Event {
    let mut e = event(time, EventKind::Closed, port, None, "app.exe");
    e.state_before = Some(TcpState::TimeWait);
    e
}

/// Starting state at 1000: ports 1 and 2; port 3 new at 2000, port 1 changes at 3000,
/// port 2 closes at 4000.
fn sample() -> RecordingReader {
    let mut w = RecordingWriter::in_memory().unwrap();
    w.set_meta("start_ms", "1000").unwrap();
    let est = Some(TcpState::Established);
    w.write(&[
        event(1000, EventKind::Present, 1, est, "app.exe"),
        event(1000, EventKind::Present, 2, est, "app.exe"),
    ])
    .unwrap();
    w.write(&[]).unwrap();
    w.write(&[event(
        2000,
        EventKind::New,
        3,
        Some(TcpState::SynSent),
        "other.exe",
    )])
    .unwrap();
    w.write(&[event(
        3000,
        EventKind::Changed,
        1,
        Some(TcpState::TimeWait),
        "app.exe",
    )])
    .unwrap();
    w.write(&[closed(4000, 2)]).unwrap();
    assert_eq!(w.events(), 5);
    w.into_reader()
}

fn ports(rows: &[Row]) -> Vec<(u16, Mark, Option<TcpState>)> {
    let mut v: Vec<_> = rows
        .iter()
        .map(|r| (r.conn.local.port(), r.mark, r.conn.state))
        .collect();
    v.sort_by_key(|x| x.0);
    v
}

#[test]
fn round_trip_of_events() {
    let mut r = sample();
    r.check().unwrap();
    let info = r.info().unwrap();
    assert_eq!(info.events, 5);
    assert_eq!((info.first, info.last), (Some(1000), Some(4000)));
    assert_eq!(info.meta("start_ms"), Some("1000"));
    assert_eq!(info.meta("format"), None);
    let all = r.events(&EventQuery::default(), 0, 100).unwrap();
    assert_eq!(all.len(), 5);
    let est = Some(TcpState::Established);
    assert_eq!(all[0], event(1000, EventKind::Present, 1, est, "app.exe"));
    assert_eq!(
        all[3],
        event(
            3000,
            EventKind::Changed,
            1,
            Some(TcpState::TimeWait),
            "app.exe"
        )
    );
    assert_eq!(all[4], closed(4000, 2));
    assert_eq!(all[4].shown_state(), Some(TcpState::TimeWait));
    // The IPv6 remote address and the process survive; a process is stored once.
    assert_eq!(
        all[2].conn.remote,
        Some("[2001:db8::1]:443".parse().unwrap())
    );
    assert_eq!(all[2].process.name, "other.exe");
    assert!(Arc::ptr_eq(&all[0].process, &all[1].process));
    let procs: i64 =
        r.db.query_row("SELECT count(*) FROM processes", [], |x| x.get(0))
            .unwrap();
    assert_eq!(procs, 2);
    // The readable view works.
    let ev: String =
        r.db.query_row("SELECT event FROM events_readable WHERE id = 5", [], |x| {
            x.get(0)
        })
        .unwrap();
    assert_eq!(ev, "closed");
}

#[test]
fn event_list_filters_pages_and_order() {
    let mut r = sample();
    let q = |text: &str| EventQuery {
        text: text.into(),
        ..Default::default()
    };
    assert_eq!(r.count(&q("")).unwrap(), 5);
    assert_eq!(r.count(&q("OTHER")).unwrap(), 1);
    assert_eq!(r.count(&q("2001:db8")).unwrap(), 5);
    assert_eq!(r.count(&q("4321")).unwrap(), 5);
    assert_eq!(r.count(&q("50%")).unwrap(), 0);
    assert_eq!(r.count(&q("a_p")).unwrap(), 0, "_ is no wildcard");
    let no_start = EventQuery {
        present: false,
        kinds: EventKinds {
            new: true,
            closed: false,
            changed: true,
        },
        ..Default::default()
    };
    assert_eq!(r.count(&no_start).unwrap(), 2);
    let page = r.events(&q(""), 1, 2).unwrap();
    assert_eq!(
        page.iter().map(|e| e.time).collect::<Vec<_>>(),
        [1000, 2000]
    );
    let newest = EventQuery {
        newest_first: true,
        ..Default::default()
    };
    let page = r.events(&newest, 0, 2).unwrap();
    assert_eq!(
        page.iter().map(|e| e.time).collect::<Vec<_>>(),
        [4000, 3000]
    );
}

#[test]
fn replay_state_at_a_time() {
    let mut r = sample();
    use Mark::*;
    let est = Some(TcpState::Established);
    assert!(r.state_at(999, 500).unwrap().is_empty());
    assert_eq!(
        ports(&r.state_at(1000, 500).unwrap()),
        [(1, None, est), (2, None, est)]
    );
    // Port 3 is new for 500 ms after 2000.
    let at = r.state_at(2100, 500).unwrap();
    assert_eq!(ports(&at)[2], (3, New, Some(TcpState::SynSent)));
    assert_eq!(
        at.iter()
            .find(|x| x.conn.local.port() == 3)
            .unwrap()
            .first_seen,
        2000
    );
    assert_eq!(ports(&r.state_at(2600, 500).unwrap())[2].1, None);
    // Port 1 changed at 3000.
    assert_eq!(
        ports(&r.state_at(3200, 500).unwrap())[0],
        (1, Changed, Some(TcpState::TimeWait))
    );
    // Port 2 is shown as closed (with its last state) for 500 ms, then gone.
    let at = ports(&r.state_at(4100, 500).unwrap());
    assert_eq!(at[1], (2, Closed, Some(TcpState::TimeWait)));
    let at = ports(&r.state_at(4600, 500).unwrap());
    assert_eq!(at.iter().map(|x| x.0).collect::<Vec<_>>(), [1, 3]);
    // The first port keeps the time it was first seen.
    let row = r.state_at(5000, 500).unwrap();
    assert_eq!(
        row.iter()
            .find(|x| x.conn.local.port() == 1)
            .unwrap()
            .first_seen,
        1000
    );
    assert_eq!(r.neighbours(2500).unwrap(), (Some(2000), Some(3000)));
    assert_eq!(r.neighbours(1000).unwrap(), (Option::None, Some(2000)));
    assert_eq!(r.neighbours(4000).unwrap(), (Some(3000), Option::None));
}

#[test]
fn schema_version_and_format_are_checked() {
    let r = sample();
    r.check().unwrap();
    r.db.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
        .unwrap();
    assert!(matches!(r.check(), Err(RecError::Newer(v)) if v == SCHEMA_VERSION + 1));
    r.db.pragma_update(None, "user_version", 0).unwrap();
    assert!(matches!(r.check(), Err(RecError::NotRecording)));
    // Another SQLite database is not a recording.
    let other = RecordingReader {
        db: Db::open_in_memory().unwrap(),
        processes: HashMap::new(),
    };
    assert!(matches!(other.check(), Err(RecError::NotRecording)));
}

#[test]
fn file_round_trip_leaves_a_single_file() {
    let dir = std::env::temp_dir().join(format!("mmportlens-rec-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test.plrec");
    std::fs::write(&path, b"old content").unwrap();
    let mut w = RecordingWriter::create(&path).unwrap();
    w.write(&[event(1000, EventKind::New, 7, None, "x.exe")])
        .unwrap();
    assert_eq!(w.finish(2000).unwrap(), 1);
    let names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["test.plrec"]);
    let mut r = RecordingReader::open(&path).unwrap();
    let info = r.info().unwrap();
    assert_eq!(info.meta("end_ms"), Some("2000"));
    assert_eq!(r.events(&EventQuery::default(), 0, 10).unwrap().len(), 1);
    drop(r);
    // A file that is not a recording is refused.
    std::fs::write(&path, b"not a database").unwrap();
    assert!(RecordingReader::open(&path).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Size and replay speed of a busy hour (run by hand: `cargo test --release busy_hour --
/// --ignored --nocapture`): 3600 refreshes, 2000 endpoints at the start, 30 events per
/// refresh.
#[test]
#[ignore]
fn busy_hour() {
    let dir = std::env::temp_dir().join(format!("mmportlens-busy-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("busy.plrec");
    let mut w = RecordingWriter::create(&path).unwrap();
    let names: Vec<Arc<ProcessInfo>> = (0..80)
        .map(|i| {
            Arc::new(ProcessInfo {
                name: format!("process{i}.exe"),
                path: format!("C:\\Program Files\\Vendor {i}\\process{i}.exe"),
                started: Some(1_700_000_000_000 + i),
                ..Default::default()
            })
        })
        .collect();
    let mk = |n: u64, time: u64, kind: EventKind| {
        let mut e = event(
            time,
            kind,
            (n % 60000) as u16 + 1,
            Some(TcpState::Established),
            "",
        );
        e.conn.remote = Some(SocketAddr::new(
            IpAddr::from([20, (n % 200) as u8, (n / 200 % 250) as u8, 7]),
            443,
        ));
        e.conn.pid = 1000 + (n % 80) as u32;
        e.process = names[(n % 80) as usize].clone();
        e
    };
    let t0 = 1_700_000_000_000u64;
    let start = std::time::Instant::now();
    w.write(
        &(0..2000)
            .map(|n| mk(n, t0, EventKind::Present))
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let mut n = 2000;
    for s in 1..=3600u64 {
        let batch: Vec<Event> = (0..30)
            .map(|i| {
                n += 1;
                let kind = [EventKind::New, EventKind::Changed, EventKind::Closed][i % 3];
                mk(n - (i as u64 % 3) * 7, t0 + s * 1000, kind)
            })
            .collect();
        w.write(&batch).unwrap();
    }
    let events = w.finish(t0 + 3_601_000).unwrap();
    let took = start.elapsed();
    let size = std::fs::metadata(&path).unwrap().len();
    eprintln!(
        "{events} events in {took:?}, {:.1} MB, {:.0} B/event",
        size as f64 / 1e6,
        size as f64 / events as f64
    );
    let mut r = RecordingReader::open(&path).unwrap();
    for at in [t0 + 60_000, t0 + 1_800_000, t0 + 3_600_000] {
        let start = std::time::Instant::now();
        let rows = r.state_at(at, 3000).unwrap();
        eprintln!(
            "state at +{} s: {} rows in {:?}",
            (at - t0) / 1000,
            rows.len(),
            start.elapsed()
        );
    }
    let start = std::time::Instant::now();
    let q = EventQuery {
        text: "process7".into(),
        ..Default::default()
    };
    let c = r.count(&q).unwrap();
    let page = r.events(&q, c / 2, 200).unwrap();
    eprintln!(
        "filtered count {c} + page {} in {:?}",
        page.len(),
        start.elapsed()
    );
    assert!(size < 50_000_000);
    drop(r);
    std::fs::remove_dir_all(&dir).unwrap();
}
