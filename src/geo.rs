//! Country and network owner (ASN) of remote addresses from offline databases (#11) – nothing is
//! sent anywhere.
//!
//! Two kinds of files, chosen by the user (`Config::geo`), plain or gzip-compressed:
//! - **ip2asn** (<https://iptoasn.com>, `ip2asn-combined.tsv.gz`, also the v4 / v6 files):
//!   tab-separated `range_start range_end AS_number country_code AS_description` – the ASN and
//!   organization, and a country when no country file is set;
//! - **DB-IP IP to Country Lite** (<https://db-ip.com/db/download/ip-to-country-lite>, CSV):
//!   `range_start,range_end,country_code`.
//!
//! A file is read in a background thread ([`start_loading`]) into sorted range tables; a lookup
//! is a binary search (a few hundred thousand ranges, ~20 steps), cheap enough for the cell texts
//! the table, the filters and the sorting ask for. The loaded database is global ([`current`]),
//! so the table, the export and the command line see the same. Private, loopback, link-local
//! and multicast addresses get their class instead of a lookup.

use crate::t;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Read};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

/// The database files (`Config::geo`).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug, Default)]
#[serde(default)]
pub struct GeoSettings {
    /// ip2asn TSV: ASN, organization (and country).
    pub asn_file: Option<PathBuf>,
    /// DB-IP country CSV.
    pub country_file: Option<PathBuf>,
}

impl GeoSettings {
    pub fn any(&self) -> bool {
        self.asn_file.is_some() || self.country_file.is_some()
    }
}

/// One address range of a table.
#[derive(Clone, Debug)]
struct Range {
    start: u128,
    end: u128,
    asn: u32,
    country: [u8; 2],
    /// Index into `Table::names` (`u32::MAX` = none).
    name: u32,
}

/// The ranges of one file, IPv4 (as IPv4-mapped values) and IPv6 together, sorted by start.
#[derive(Default, Debug)]
pub struct Table {
    ranges: Vec<Range>,
    names: Vec<Box<str>>,
}

impl Table {
    pub fn len(&self) -> usize {
        self.ranges.len()
    }

    fn find(&self, ip: IpAddr) -> Option<&Range> {
        let v = key(ip);
        let i = self.ranges.partition_point(|r| r.start <= v);
        let r = self.ranges.get(i.checked_sub(1)?)?;
        (v <= r.end).then_some(r)
    }
}

/// An address as one number: IPv4 mapped into IPv6 (`::ffff:a.b.c.d`), so both fit one table.
fn key(ip: IpAddr) -> u128 {
    match ip.to_canonical() {
        IpAddr::V4(v4) => u128::from(v4.to_ipv6_mapped()),
        IpAddr::V6(v6) => u128::from(v6),
    }
}

/// What a lookup found.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct GeoInfo {
    /// Two-letter country code (`US`).
    pub country: Option<String>,
    pub asn: Option<u32>,
    pub org: Option<String>,
}

/// The loaded database: the ASN table and the country table, each optional.
#[derive(Default, Debug)]
pub struct GeoDb {
    pub asn: Option<Table>,
    pub country: Option<Table>,
}

impl GeoDb {
    pub fn lookup(&self, ip: IpAddr) -> GeoInfo {
        let mut info = GeoInfo::default();
        if let Some(r) = self.asn.as_ref().and_then(|t| {
            let r = t.find(ip)?;
            Some((r, t))
        }) {
            let (r, t) = r;
            if r.asn != 0 {
                info.asn = Some(r.asn);
                info.org = t.names.get(r.name as usize).map(|s| s.to_string());
            }
            info.country = country_text(r.country);
        }
        if let Some(r) = self.country.as_ref().and_then(|t| t.find(ip))
            && let Some(c) = country_text(r.country)
        {
            info.country = Some(c);
        }
        info
    }
}

fn country_text(cc: [u8; 2]) -> Option<String> {
    (cc[0].is_ascii_alphabetic() && cc[1].is_ascii_alphabetic())
        .then(|| String::from_utf8_lossy(&cc).to_ascii_uppercase())
}

/// The class of an address that is not looked up, as its translation key.
pub fn special_class(ip: IpAddr) -> Option<&'static str> {
    let ip = ip.to_canonical();
    if ip.is_unspecified() {
        return Some("");
    }
    if ip.is_loopback() {
        return Some("geo.loopback");
    }
    if ip.is_multicast() {
        return Some("geo.multicast");
    }
    let private = match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            if v4.is_link_local() {
                return Some("geo.link_local");
            }
            v4.is_private() || (o[0] == 100 && (o[1] & 0xC0) == 64) || v4.is_broadcast()
        }
        IpAddr::V6(v6) => {
            let s = v6.segments()[0];
            if (s & 0xFFC0) == 0xFE80 {
                return Some("geo.link_local");
            }
            (s & 0xFE00) == 0xFC00
        }
    };
    private.then_some("geo.private")
}

/// Parses one file: ip2asn TSV or DB-IP CSV, recognized per line by the separator.
pub fn parse(reader: impl BufRead) -> Result<Table, String> {
    let mut table = Table::default();
    let mut names: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let mut bad = 0usize;
    for (n, line) in reader.lines().enumerate() {
        let line = line.map_err(|e| format!("line {}: {e}", n + 1))?;
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = if line.contains('\t') {
            line.split('\t').collect()
        } else {
            line.split(',')
                .map(|f| f.trim().trim_matches('"'))
                .collect()
        };
        let parsed = (|| {
            let start: IpAddr = fields.first()?.trim().parse().ok()?;
            let end: IpAddr = fields.get(1)?.trim().parse().ok()?;
            let (asn, cc, name) = if fields.len() >= 5 {
                (
                    fields[2].trim().parse::<u32>().ok()?,
                    fields[3],
                    Some(fields[4]),
                )
            } else {
                (0, *fields.get(2)?, None)
            };
            let b = cc.trim().as_bytes();
            let country = if b.len() == 2 { [b[0], b[1]] } else { [0, 0] };
            Some((key(start), key(end), asn, country, name))
        })();
        let Some((start, end, asn, country, name)) = parsed else {
            bad += 1;
            if bad > 10 && table.ranges.is_empty() {
                return Err(t!("geo.error.format", line = n + 1));
            }
            continue;
        };
        let name = match name
            .map(str::trim)
            .filter(|s| !s.is_empty() && *s != "Not routed")
        {
            Some(s) => *names.entry(s.to_owned()).or_insert_with(|| {
                table.names.push(s.into());
                (table.names.len() - 1) as u32
            }),
            None => u32::MAX,
        };
        table.ranges.push(Range {
            start,
            end,
            asn,
            country,
            name,
        });
    }
    if table.ranges.is_empty() {
        return Err(t!("geo.error.empty"));
    }
    table.ranges.sort_by_key(|r| r.start);
    Ok(table)
}

/// Reads a file (gzip when it starts with the gzip magic).
pub fn read_file(path: &Path) -> Result<Table, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut reader = std::io::BufReader::with_capacity(1 << 20, file);
    let gz = reader
        .fill_buf()
        .map(|b| b.starts_with(&[0x1f, 0x8b]))
        .unwrap_or(false);
    let res = if gz {
        let dec: Box<dyn Read> = Box::new(flate2::read::MultiGzDecoder::new(reader));
        parse(std::io::BufReader::with_capacity(1 << 20, dec))
    } else {
        parse(reader)
    };
    res.map_err(|e| format!("{}: {e}", path.display()))
}

/// Reads the files of the settings.
pub fn load(settings: &GeoSettings) -> Result<GeoDb, String> {
    Ok(GeoDb {
        asn: settings.asn_file.as_deref().map(read_file).transpose()?,
        country: settings
            .country_file
            .as_deref()
            .map(read_file)
            .transpose()?,
    })
}

/// The state of the database.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum Status {
    /// No file chosen.
    #[default]
    None,
    Loading,
    /// Loaded: the number of ranges of the ASN and the country table.
    Ready(usize, usize),
    Failed(String),
}

static DB: RwLock<Option<Arc<GeoDb>>> = RwLock::new(None);
static STATUS: Mutex<Status> = Mutex::new(Status::None);
/// Counts the loads that finished, so the UI notices a new database.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// The loaded database, if any.
pub fn current() -> Option<Arc<GeoDb>> {
    DB.read().ok()?.clone()
}

pub fn status() -> Status {
    STATUS.lock().map(|s| s.clone()).unwrap_or_default()
}

pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

fn finish(res: Result<GeoDb, String>) {
    let status = match res {
        Ok(db) => {
            let st = Status::Ready(
                db.asn.as_ref().map_or(0, Table::len),
                db.country.as_ref().map_or(0, Table::len),
            );
            if let Ok(mut g) = DB.write() {
                *g = Some(Arc::new(db));
            }
            st
        }
        Err(e) => {
            if let Ok(mut g) = DB.write() {
                *g = None;
            }
            Status::Failed(e)
        }
    };
    if let Ok(mut s) = STATUS.lock() {
        *s = status;
    }
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// Loads the files of `settings` in a background thread (`done` is called afterwards); without
/// files the database is dropped at once.
pub fn start_loading(settings: &GeoSettings, done: impl FnOnce() + Send + 'static) {
    if !settings.any() {
        if let Ok(mut g) = DB.write() {
            *g = None;
        }
        if let Ok(mut s) = STATUS.lock() {
            *s = Status::None;
        }
        GENERATION.fetch_add(1, Ordering::Relaxed);
        done();
        return;
    }
    if let Ok(mut s) = STATUS.lock() {
        *s = Status::Loading;
    }
    let settings = settings.clone();
    let spawned = std::thread::Builder::new()
        .name("geo".into())
        .spawn(move || {
            finish(load(&settings));
            done();
        });
    if let Err(e) = spawned {
        finish(Err(e.to_string()));
    }
}

/// Loads the files in this thread (the runs without a window, #16).
pub fn load_now(settings: &GeoSettings) -> Result<(), String> {
    if !settings.any() {
        return Ok(());
    }
    let res = load(settings);
    let err = res.as_ref().err().cloned();
    finish(res);
    err.map_or(Ok(()), Err)
}

/// The Country cell of an address: its class (private, loopback …) or the country code; empty
/// without a database or when the address is not in it.
pub fn country_cell(ip: IpAddr) -> String {
    if let Some(k) = special_class(ip) {
        return if k.is_empty() { String::new() } else { t!(k) };
    }
    current()
        .and_then(|db| db.lookup(ip).country)
        .unwrap_or_default()
}

/// The ASN cell: `AS8075 MICROSOFT-CORP-MSN-AS-BLOCK`, the class of a special address, or empty.
pub fn asn_cell(ip: IpAddr) -> String {
    if let Some(k) = special_class(ip) {
        return if k.is_empty() { String::new() } else { t!(k) };
    }
    let Some(db) = current() else {
        return String::new();
    };
    let info = db.lookup(ip);
    match (info.asn, info.org) {
        (Some(a), Some(o)) => format!("AS{a} {o}"),
        (Some(a), None) => format!("AS{a}"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASN: &str = "1.0.0.0\t1.0.0.255\t13335\tUS\tCLOUDFLARENET\n\
        1.0.1.0\t1.0.3.255\t0\tNone\tNot routed\n\
        13.64.0.0\t13.107.255.255\t8075\tUS\tMICROSOFT-CORP-MSN-AS-BLOCK\n\
        2001:4860::\t2001:4860:ffff:ffff:ffff:ffff:ffff:ffff\t15169\tUS\tGOOGLE\n\
        8.8.8.0\t8.8.8.255\t15169\tUS\tGOOGLE\n";
    const COUNTRY: &str = "\"1.0.0.0\",\"1.0.0.255\",\"AU\"\n13.64.0.0,13.107.255.255,NL\n";

    fn db() -> GeoDb {
        GeoDb {
            asn: Some(parse(ASN.as_bytes()).unwrap()),
            country: None,
        }
    }

    #[test]
    fn lookups_in_ip2asn() {
        let d = db();
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        let g = d.lookup(ip("8.8.8.8"));
        assert_eq!(
            (g.asn, g.org.as_deref(), g.country.as_deref()),
            (Some(15169), Some("GOOGLE"), Some("US"))
        );
        assert_eq!(d.lookup(ip("13.107.4.50")).asn, Some(8075));
        assert_eq!(d.lookup(ip("2001:4860:4860::8888")).asn, Some(15169));
        // IPv4-mapped IPv6 is found as IPv4.
        assert_eq!(d.lookup(ip("::ffff:8.8.4.4")).asn, None);
        assert_eq!(d.lookup(ip("::ffff:8.8.8.4")).asn, Some(15169));
        // Not routed, outside of every range, at the edges.
        assert_eq!(d.lookup(ip("1.0.2.1")), GeoInfo::default());
        assert_eq!(d.lookup(ip("9.9.9.9")), GeoInfo::default());
        assert_eq!(d.lookup(ip("1.0.0.0")).asn, Some(13335));
        assert_eq!(d.lookup(ip("1.0.0.255")).asn, Some(13335));
        assert_eq!(d.lookup(ip("0.255.255.255")).asn, None);
    }

    #[test]
    fn the_country_file_wins() {
        let d = GeoDb {
            country: Some(parse(COUNTRY.as_bytes()).unwrap()),
            ..db()
        };
        let g = d.lookup("13.64.1.1".parse().unwrap());
        assert_eq!((g.country.as_deref(), g.asn), (Some("NL"), Some(8075)));
        assert_eq!(
            d.lookup("1.0.0.1".parse().unwrap()).country.as_deref(),
            Some("AU")
        );
    }

    #[test]
    fn special_addresses_are_not_looked_up() {
        let c = |s: &str| special_class(s.parse().unwrap());
        assert_eq!(c("127.0.0.1"), Some("geo.loopback"));
        assert_eq!(c("::1"), Some("geo.loopback"));
        assert_eq!(c("10.1.2.3"), Some("geo.private"));
        assert_eq!(c("192.168.50.1"), Some("geo.private"));
        assert_eq!(c("100.64.0.1"), Some("geo.private"));
        assert_eq!(c("fd00::1"), Some("geo.private"));
        assert_eq!(c("169.254.1.1"), Some("geo.link_local"));
        assert_eq!(c("fe80::1"), Some("geo.link_local"));
        assert_eq!(c("224.0.0.251"), Some("geo.multicast"));
        assert_eq!(c("0.0.0.0"), Some(""));
        assert_eq!(c("8.8.8.8"), None);
        assert_eq!(c("::ffff:10.0.0.1"), Some("geo.private"));
    }

    #[test]
    fn gzip_files_and_errors() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("mmportlens-geo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let gz = dir.join("ip2asn.tsv.gz");
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(ASN.as_bytes()).unwrap();
        std::fs::write(&gz, enc.finish().unwrap()).unwrap();
        assert_eq!(read_file(&gz).unwrap().len(), 5);
        let bad = dir.join("bad.txt");
        std::fs::write(&bad, "hello\nworld\n".repeat(10)).unwrap();
        assert!(read_file(&bad).is_err());
        assert!(read_file(&dir.join("missing.tsv")).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
