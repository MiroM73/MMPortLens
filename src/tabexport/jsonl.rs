//! JSON Lines writer of the table export (#9, from RusTrace): one JSON object per row – for
//! scripts, Log Analytics or Splunk ingestion. Keys are stable machine names (`process`, `pid`,
//! `local_address` …); dates are ISO local time; numbers are numbers; empty cells are left out.

use super::{Cell, ColumnSpec, RowLook, TableSink};
use std::io::{self, Write};

pub struct JsonlWriter<W: Write> {
    out: W,
    keys: Vec<String>,
    line: String,
}

impl<W: Write> JsonlWriter<W> {
    /// `keys` = the JSON key of every column (`ColumnSpec::json_key`).
    pub fn new(out: W, keys: Vec<String>) -> Self {
        Self {
            out,
            keys,
            line: String::new(),
        }
    }

    pub fn finish(mut self) -> io::Result<W> {
        self.out.flush()?;
        Ok(self.out)
    }
}

fn json_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

impl<W: Write> TableSink for JsonlWriter<W> {
    fn header(&mut self, _cols: &[ColumnSpec]) -> io::Result<()> {
        Ok(())
    }

    fn row(&mut self, cells: &[Cell<'_>], _look: RowLook) -> io::Result<()> {
        self.line.clear();
        self.line.push('{');
        let mut first = true;
        for (key, c) in self.keys.iter().zip(cells) {
            let value = match c {
                Cell::Empty => continue,
                Cell::Text(t) => json_str(t),
                Cell::Int(i) => i.to_string(),
                Cell::Seconds(s) => format!("{s:.3}"),
                Cell::Date { text, .. } => json_str(text),
            };
            if !first {
                self.line.push(',');
            }
            first = false;
            self.line.push_str(&json_str(key));
            self.line.push(':');
            self.line.push_str(&value);
        }
        self.line.push_str("}\n");
        self.out.write_all(self.line.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{NOON, sample, specs};
    use super::super::*;
    use super::*;

    #[test]
    fn one_object_per_row() {
        use ColumnId::*;
        let cols = specs(&[Process, Pid, RemotePort, Created, Duration, BytesSent]);
        let keys = cols.iter().map(|c| c.json_key().to_owned()).collect();
        let mut w = JsonlWriter::new(Vec::new(), keys);
        write_table(&sample(), &cols, true, NOON, &mut w, &Control::default()).unwrap();
        let text = String::from_utf8(w.finish().unwrap()).unwrap();
        let lines: Vec<serde_json::Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0]["process"], "chrome.exe");
        assert_eq!(lines[0]["pid"], 1234);
        assert_eq!(lines[0]["remote_port"], 443);
        assert_eq!(lines[0]["duration"], 1.5);
        assert_eq!(lines[0]["bytes_sent"], 1_500_000);
        assert!(lines[0]["created"].as_str().unwrap().ends_with(":58.500"));
        assert!(lines[1].get("remote_port").is_none());
        assert_eq!(lines[2]["process"], "=evil;\"x\"");
    }
}
