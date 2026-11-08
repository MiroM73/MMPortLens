//! Export of the connection table (#9): which rows, which columns and the typed value of every
//! cell, written by one of the writers (`csv`, `xlsx`, `jsonl`, `html`). Pure – no egui; the UI
//! part is `app/export.rs`.
//!
//! The writers and the cell / sink core come from RusTrace (`src/tabexport/`, its #32); this file
//! is new: RusTrace exports log entries, here the rows are connections (`model::Row`) copied out
//! of the view, so the export thread never touches the live table.

pub mod csv;
pub mod html;
pub mod jsonl;
pub mod xlsx;
mod zip;

use crate::columns::{ColumnId, cell_text};
use crate::model::{Mark, Row};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Which rows are exported.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// The rows shown in the table (switches, text filter and column filters), in table order.
    #[default]
    Visible,
    /// The selected rows (#17).
    Selected,
    /// Every endpoint, whatever the filters (still in table order).
    All,
}

/// Which columns are exported.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum ColumnSet {
    /// The visible columns in their table order.
    #[default]
    Visible,
    /// Every column: the visible ones first (table order), then the hidden ones.
    All,
}

/// File format of the export.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum TableFormat {
    #[default]
    Csv,
    Xlsx,
    /// JSON Lines – one object per row.
    Jsonl,
    /// A self-contained HTML report.
    Html,
}

impl TableFormat {
    pub const ALL: [TableFormat; 4] = [
        TableFormat::Csv,
        TableFormat::Xlsx,
        TableFormat::Jsonl,
        TableFormat::Html,
    ];

    pub fn extension(self) -> &'static str {
        match self {
            TableFormat::Csv => "csv",
            TableFormat::Xlsx => "xlsx",
            TableFormat::Jsonl => "jsonl",
            TableFormat::Html => "html",
        }
    }

    /// The format of a file name by its extension (the save dialog, later `--export`, #16).
    pub fn from_path(path: &Path) -> Option<TableFormat> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "csv" | "txt" => Some(TableFormat::Csv),
            "xlsx" => Some(TableFormat::Xlsx),
            "jsonl" | "ndjson" => Some(TableFormat::Jsonl),
            "html" | "htm" => Some(TableFormat::Html),
            _ => None,
        }
    }
}

/// Settings of the export (saved in the config).
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Settings {
    pub scope: Scope,
    pub columns: ColumnSet,
    pub format: TableFormat,
    /// CSV separator; `None` = the list separator of Windows (`;` for SK / CS / DE).
    pub csv_separator: Option<char>,
    pub csv_encoding: csv::CsvEncoding,
    pub header: bool,
    /// Values starting with `=`, `+`, `-`, `@` get a leading `'` (CSV / formula injection).
    pub safe_for_excel: bool,
    /// Excel / HTML: rows colored like the new / changed / closed rows of the table.
    pub colors: bool,
    /// Open the file with its default program after the export.
    pub open_after: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            scope: Scope::Visible,
            columns: ColumnSet::Visible,
            format: TableFormat::Csv,
            csv_separator: None,
            csv_encoding: csv::CsvEncoding::Utf8Bom,
            header: true,
            safe_for_excel: true,
            colors: true,
            open_after: false,
        }
    }
}

/// Name of this computer (heading of the HTML report, recording meta).
pub fn computer_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_default()
}

/// The exported columns: the visible ones, with `ColumnSet::All` followed by the hidden ones.
pub fn export_columns(
    set: ColumnSet,
    visible: &[ColumnId],
    all_in_order: &[ColumnId],
) -> Vec<ColumnId> {
    let mut cols = visible.to_vec();
    if set == ColumnSet::All {
        cols.extend(all_in_order.iter().filter(|c| !visible.contains(c)));
    }
    cols
}

/// A column with its header text and its JSON key.
#[derive(Clone, Debug)]
pub struct ColumnSpec {
    pub col: ColumnId,
    pub title: String,
}

impl ColumnSpec {
    /// The column with its translated title; the unit is added where the value differs from the
    /// table text (raw bytes instead of `1.5 KB`, seconds instead of `3 min 05 s`).
    pub fn new(col: ColumnId) -> Self {
        let title = match unit(col) {
            Some(u) => format!("{} ({u})", col.title()),
            None => col.title(),
        };
        Self { col, title }
    }

    /// Stable machine name for JSON Lines (`local_address`, `bytes_sent` …).
    pub fn json_key(&self) -> &'static str {
        self.col
            .key()
            .strip_prefix("column.")
            .unwrap_or(self.col.key())
    }
}

fn unit(col: ColumnId) -> Option<&'static str> {
    match col {
        ColumnId::BytesSent | ColumnId::BytesReceived => Some("B"),
        ColumnId::RateSent | ColumnId::RateReceived => Some("B/s"),
        ColumnId::Duration => Some("s"),
        _ => None,
    }
}

/// The value of a cell.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell<'a> {
    Empty,
    Text(Cow<'a, str>),
    Int(i64),
    /// A duration in seconds.
    Seconds(f64),
    /// A date/time: `ms` = the local wall-clock time in ms since 1970 (what Excel shows), `text`
    /// = the same as ISO `2026-11-08 12:43:55.123` (sorts as text, too).
    Date {
        ms: i64,
        text: String,
    },
}

impl Cell<'_> {
    /// Text of the cell; `decimal` = the decimal separator of numbers with a fraction.
    pub fn text(&self, decimal: char) -> Cow<'_, str> {
        match self {
            Cell::Empty => Cow::Borrowed(""),
            Cell::Text(t) => Cow::Borrowed(t.as_ref()),
            Cell::Int(i) => Cow::Owned(i.to_string()),
            Cell::Seconds(s) => {
                let t = format!("{s:.3}");
                Cow::Owned(if decimal == '.' {
                    t
                } else {
                    t.replace('.', &decimal.to_string())
                })
            }
            Cell::Date { text, .. } => Cow::Borrowed(text.as_str()),
        }
    }
}

/// Colors of a row (as in the table: new / changed / closed, light palette).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct RowLook {
    pub fg: Option<[u8; 4]>,
    pub bg: Option<[u8; 4]>,
}

impl RowLook {
    /// The light-theme background of a marked row (`app::mark_color`).
    pub fn of_mark(mark: Mark) -> Self {
        let bg = match mark {
            Mark::None => return Self::default(),
            Mark::New => [190, 240, 190, 255],
            Mark::Changed => [250, 235, 160, 255],
            Mark::Closed => [250, 190, 190, 255],
        };
        Self {
            fg: None,
            bg: Some(bg),
        }
    }
}

/// Receives the rows (one writer per file format).
pub trait TableSink {
    fn header(&mut self, cols: &[ColumnSpec]) -> io::Result<()>;
    fn row(&mut self, cells: &[Cell<'_>], look: RowLook) -> io::Result<()>;
}

/// Progress and cancel flag shared with the UI.
#[derive(Clone, Default)]
pub struct Control {
    pub done: Arc<AtomicUsize>,
    pub cancel: Arc<AtomicBool>,
}

impl Control {
    fn check(&self, done: usize) -> io::Result<()> {
        if done.is_multiple_of(256) {
            self.done.store(done, Ordering::Relaxed);
            if self.cancel.load(Ordering::Relaxed) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
        }
        Ok(())
    }
}

/// Local wall-clock ms and ISO text of a time in ms since the Unix epoch.
fn local_time(ms: u64) -> Option<(i64, String)> {
    use chrono::{Local, TimeZone};
    let t = match Local.timestamp_millis_opt(ms as i64) {
        chrono::LocalResult::Single(t) | chrono::LocalResult::Ambiguous(t, _) => t,
        chrono::LocalResult::None => return None,
    };
    let naive = t.naive_local();
    Some((
        naive.and_utc().timestamp_millis(),
        naive.format("%Y-%m-%d %H:%M:%S%.3f").to_string(),
    ))
}

fn date(ms: Option<u64>) -> Cell<'static> {
    match ms.and_then(local_time) {
        Some((ms, text)) => Cell::Date { ms, text },
        None => Cell::Empty,
    }
}

fn int(v: Option<u64>) -> Cell<'static> {
    v.map_or(Cell::Empty, |v| Cell::Int(v.min(i64::MAX as u64) as i64))
}

/// The typed value of a cell: numbers as numbers, times as dates, raw byte counts; everything
/// else is the text of the table. `now` = the time of the export (durations).
pub fn cell(row: &Row, col: ColumnId, now: u64) -> Cell<'static> {
    let c = &row.conn;
    match col {
        ColumnId::Pid => Cell::Int(c.pid as i64),
        ColumnId::LocalPort => Cell::Int(c.local.port() as i64),
        ColumnId::RemotePort => int(c.remote.map(|r| r.port() as u64)),
        ColumnId::FirstSeen => date(Some(row.first_seen)),
        ColumnId::Created => date(c.created),
        ColumnId::Duration => c.created.map_or(Cell::Empty, |t| {
            Cell::Seconds(now.saturating_sub(t) as f64 / 1000.0)
        }),
        ColumnId::BytesSent => int(c.stats.map(|s| s.bytes_out)),
        ColumnId::BytesReceived => int(c.stats.map(|s| s.bytes_in)),
        ColumnId::RateSent => int(row.rate_out.map(|r| r.round().max(0.0) as u64)),
        ColumnId::RateReceived => int(row.rate_in.map(|r| r.round().max(0.0) as u64)),
        ColumnId::PacketsSent => int(c.stats.map(|s| s.segs_out)),
        ColumnId::PacketsReceived => int(c.stats.map(|s| s.segs_in)),
        _ => {
            let t = cell_text(row, col);
            if t.is_empty() {
                Cell::Empty
            } else {
                Cell::Text(Cow::Owned(t))
            }
        }
    }
}

/// Writes the header and every row into `sink`, rows colored by their marks; returns the number
/// of rows. Cancelling (`ctl.cancel`) ends with an `Interrupted` error.
#[cfg(test)]
pub fn write_table(
    rows: &[Row],
    cols: &[ColumnSpec],
    header: bool,
    now: u64,
    sink: &mut dyn TableSink,
    ctl: &Control,
) -> io::Result<usize> {
    write_rows(rows, &[], cols, header, now, sink, ctl)
}

/// [`write_table`] with the colors of the rows given (`looks[i]` for `rows[i]`, e.g. the color
/// rules of #10); rows without a look are colored by their marks.
pub fn write_rows(
    rows: &[Row],
    looks: &[RowLook],
    cols: &[ColumnSpec],
    header: bool,
    now: u64,
    sink: &mut dyn TableSink,
    ctl: &Control,
) -> io::Result<usize> {
    if header {
        sink.header(cols)?;
    }
    let mut cells = Vec::with_capacity(cols.len());
    for (n, r) in rows.iter().enumerate() {
        cells.clear();
        cells.extend(cols.iter().map(|c| cell(r, c.col, now)));
        let look = looks
            .get(n)
            .copied()
            .unwrap_or_else(|| RowLook::of_mark(r.mark));
        sink.row(&cells, look)?;
        ctl.check(n + 1)?;
    }
    ctl.done.store(rows.len(), Ordering::Relaxed);
    Ok(rows.len())
}

/// Everything one export needs (moved into the export thread).
pub struct Job {
    pub rows: Vec<Row>,
    /// The colors of the rows (empty = by their marks).
    pub looks: Vec<RowLook>,
    pub cols: Vec<ColumnSpec>,
    pub format: TableFormat,
    pub header: bool,
    /// Time of the export (durations, the HTML heading).
    pub now: u64,
    pub csv: csv::CsvOptions,
    pub xlsx: xlsx::XlsxOptions,
    /// HTML report: heading, the lines under it, row colors.
    pub html_title: String,
    pub html_info: Vec<String>,
    pub html_colors: bool,
}

/// Writes the export to `path`; returns (rows, cells cut at the Excel limit). A failed or
/// cancelled export leaves no file behind.
pub fn run(job: &Job, path: &Path, ctl: &Control) -> io::Result<(usize, usize)> {
    use std::io::Write as _;
    let res = (|| -> io::Result<(usize, usize)> {
        let file = io::BufWriter::new(std::fs::File::create(path)?);
        let table = |sink: &mut dyn TableSink| {
            write_rows(
                &job.rows, &job.looks, &job.cols, job.header, job.now, sink, ctl,
            )
        };
        match job.format {
            TableFormat::Csv => {
                let mut w = csv::CsvWriter::new(file, job.csv)?;
                let n = table(&mut w)?;
                w.finish()?;
                Ok((n, 0))
            }
            TableFormat::Xlsx => {
                let mut w = xlsx::XlsxWriter::new(file, job.xlsx.clone());
                let n = table(&mut w)?;
                let cut = w.truncated;
                w.finish()?.flush()?;
                Ok((n, cut))
            }
            TableFormat::Jsonl => {
                let keys = job.cols.iter().map(|c| c.json_key().to_owned()).collect();
                let mut w = jsonl::JsonlWriter::new(file, keys);
                let n = table(&mut w)?;
                w.finish()?;
                Ok((n, 0))
            }
            TableFormat::Html => {
                let mut w =
                    html::HtmlWriter::new(file, &job.html_title, &job.html_info, job.html_colors)?;
                let n = table(&mut w)?;
                w.finish()?;
                Ok((n, 0))
            }
        }
    })();
    if res.is_err() {
        let _ = std::fs::remove_file(path);
    }
    res
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::net::estats::TcpStats;
    use crate::net::{Connection, Protocol, TcpState};
    use crate::process::ProcessInfo;

    /// 2026-11-08 12:00:00 UTC.
    pub const NOON: u64 = 1_794_139_200_000;

    /// Three rows: an established TCP connection with traffic (new), a UDP endpoint, a process
    /// whose name looks like a formula and contains the separator (closed).
    pub fn sample() -> Vec<Row> {
        let row = |conn: Connection, name: &str, mark: Mark| Row {
            conn,
            process: Arc::new(ProcessInfo {
                name: name.into(),
                ..Default::default()
            }),
            first_seen: NOON,
            mark,
            mark_until: 0,
            local_host: None,
            remote_host: None,
            rate_out: Some(1536.4),
            rate_in: None,
            updated: NOON,
        };
        vec![
            row(
                Connection {
                    proto: Protocol::Tcp,
                    local: "10.0.0.1:50000".parse().unwrap(),
                    remote: Some("1.2.3.4:443".parse().unwrap()),
                    state: Some(TcpState::Established),
                    pid: 1234,
                    created: Some(NOON - 1500),
                    module: None,
                    stats: Some(TcpStats {
                        bytes_out: 1_500_000,
                        bytes_in: 42,
                        segs_out: 7,
                        segs_in: 9,
                    }),
                },
                "chrome.exe",
                Mark::New,
            ),
            row(
                Connection {
                    proto: Protocol::Udp,
                    local: "0.0.0.0:5353".parse().unwrap(),
                    remote: None,
                    state: None,
                    pid: 4,
                    created: None,
                    module: None,
                    stats: None,
                },
                "System",
                Mark::None,
            ),
            row(
                Connection {
                    proto: Protocol::Tcp6,
                    local: "[::1]:8080".parse().unwrap(),
                    remote: None,
                    state: Some(TcpState::Listen),
                    pid: 77,
                    created: None,
                    module: None,
                    stats: None,
                },
                "=evil;\"x\"",
                Mark::Closed,
            ),
        ]
    }

    pub fn specs(cols: &[ColumnId]) -> Vec<ColumnSpec> {
        cols.iter().map(|&c| ColumnSpec::new(c)).collect()
    }

    #[test]
    fn typed_cells() {
        let rows = sample();
        let now = NOON;
        assert_eq!(cell(&rows[0], ColumnId::Pid, now), Cell::Int(1234));
        assert_eq!(cell(&rows[0], ColumnId::RemotePort, now), Cell::Int(443));
        assert_eq!(cell(&rows[1], ColumnId::RemotePort, now), Cell::Empty);
        assert_eq!(
            cell(&rows[0], ColumnId::BytesSent, now),
            Cell::Int(1_500_000)
        );
        assert_eq!(cell(&rows[0], ColumnId::RateSent, now), Cell::Int(1536));
        assert_eq!(cell(&rows[0], ColumnId::Duration, now), Cell::Seconds(1.5));
        assert_eq!(cell(&rows[1], ColumnId::Duration, now), Cell::Empty);
        assert_eq!(
            cell(&rows[0], ColumnId::Process, now),
            Cell::Text("chrome.exe".into())
        );
        match cell(&rows[0], ColumnId::FirstSeen, now) {
            Cell::Date { ms, text } => {
                // The local wall-clock time, whatever the time zone of the test machine.
                assert_eq!(ms % 1000, 0);
                assert!(text.ends_with(":00:00.000"), "{text}");
                assert_eq!(text.len(), 23);
            }
            c => panic!("{c:?}"),
        }
        assert_eq!(
            ColumnSpec::new(ColumnId::BytesSent).json_key(),
            "bytes_sent"
        );
        assert!(ColumnSpec::new(ColumnId::BytesSent).title.ends_with(" (B)"));
        assert!(!ColumnSpec::new(ColumnId::Pid).title.contains('('));
    }

    #[test]
    fn column_sets() {
        use ColumnId::*;
        let visible = [Pid, Process];
        let all = [Process, Pid, State, Protocol];
        assert_eq!(
            export_columns(ColumnSet::Visible, &visible, &all),
            [Pid, Process]
        );
        assert_eq!(
            export_columns(ColumnSet::All, &visible, &all),
            [Pid, Process, State, Protocol]
        );
    }

    #[test]
    fn row_colors_follow_the_marks() {
        assert_eq!(RowLook::of_mark(Mark::None), RowLook::default());
        assert!(RowLook::of_mark(Mark::New).bg.is_some());
        assert_ne!(RowLook::of_mark(Mark::New), RowLook::of_mark(Mark::Closed));
    }

    #[test]
    fn formats_by_extension() {
        for f in TableFormat::ALL {
            let p = format!("out.{}", f.extension().to_uppercase());
            assert_eq!(TableFormat::from_path(Path::new(&p)), Some(f));
        }
        assert_eq!(TableFormat::from_path(Path::new("x.doc")), None);
    }

    #[test]
    fn run_writes_and_cancel_leaves_no_file() {
        let dir = std::env::temp_dir().join(format!("mmportlens-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let job = |format| Job {
            rows: sample(),
            looks: Vec::new(),
            cols: specs(&[ColumnId::Process, ColumnId::Pid]),
            format,
            header: true,
            now: NOON,
            csv: csv::CsvOptions::new(',', csv::CsvEncoding::Utf8, true),
            xlsx: xlsx::XlsxOptions::default(),
            html_title: "t".into(),
            html_info: Vec::new(),
            html_colors: true,
        };
        for f in TableFormat::ALL {
            let path = dir.join(format!("out.{}", f.extension()));
            let (n, cut) = run(&job(f), &path, &Control::default()).unwrap();
            assert_eq!((n, cut), (3, 0));
            assert!(std::fs::metadata(&path).unwrap().len() > 0, "{f:?}");
        }
        // A cancelled export removes its file.
        let mut big = job(TableFormat::Csv);
        big.rows = (0..1000).map(|_| sample()[0].clone()).collect();
        let ctl = Control::default();
        ctl.cancel.store(true, Ordering::Relaxed);
        let path = dir.join("cancelled.csv");
        let err = run(&big, &path, &ctl).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Interrupted);
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
