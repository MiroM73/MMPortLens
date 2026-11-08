//! Filters of single table columns – the filter row under the column headers (#1). The core
//! (alternatives, exact match, negation, numeric ranges) is taken from RusTrace's `colfilter.rs`;
//! regular expressions, `a-b` ranges and the address alternatives are added here.
//!
//! Syntax of a filter text (always case-insensitive):
//! * `text` – the value contains `text`,
//! * `=text` – the value is exactly `text`,
//! * `/regex/` – the value matches the regular expression,
//! * `a|b|c` – any of the alternatives (each one may be of any kind; a `|` inside `/…/` belongs
//!   to the regex),
//! * `!…` at the start – negation: the value matches none of the alternatives.
//!
//! Number columns (PID, ports) also understand `>n`, `>=n`, `<n`, `<=n`, `a-b` and `a..b`
//! (inclusive, a `..` bound may be left out); a value that is not a number never matches them.
//!
//! Address columns also understand a network in CIDR notation (`10.0.0.0/8`, `fe80::/10`) and the
//! keywords `loopback`, `private` and `public`; an empty address never matches them.
//!
//! Spaces around alternatives are ignored; an empty filter (or only `!`) filters nothing. An
//! invalid regular expression or network is an error: the caller shows it and does not filter.

use regex::{Regex, RegexBuilder};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::ops::{Bound, RangeBounds};

/// Numbers matched by a numeric alternative.
type NumRange = (Bound<f64>, Bound<f64>);

/// What the values of a column are, i.e. which alternatives its filter understands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FilterKind {
    Text,
    Number,
    Address,
}

/// Why a filter text could not be used.
#[derive(Clone, PartialEq, Debug)]
pub enum FilterError {
    /// `/…/` that is not a valid regular expression (the message of the regex crate).
    Regex(String),
    /// An address with a `/` whose prefix length is not valid (e.g. `10.0.0.0/33`).
    Network(String),
}

impl FilterError {
    /// The translated message (tooltip of the field, command line errors).
    pub fn text(&self) -> String {
        match self {
            FilterError::Regex(error) => crate::t!("filter.error.regex", error = error),
            FilterError::Network(value) => crate::t!("filter.error.network", value = value),
        }
    }
}

/// An address alternative.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Net {
    /// Network address and prefix length.
    Cidr(IpAddr, u8),
    Loopback,
    Private,
    Public,
}

#[derive(Clone, Debug)]
pub struct ColumnFilter {
    /// The text alternatives (contains / exact / regex); `None` when there are none.
    re: Option<Regex>,
    ranges: Vec<NumRange>,
    nets: Vec<Net>,
    negate: bool,
}

/// One alternative of a filter text: the text and whether it must match exactly (`=`).
struct Alternative<'a> {
    text: &'a str,
    exact: bool,
}

/// The parts of a filter text between `|`, except that a `|` inside a `/regex/` alternative
/// belongs to the regex (`/^(a|b)/` is one alternative).
fn split_alternatives(body: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut in_regex = false;
    let mut alt_start = true;
    for (i, c) in body.char_indices() {
        match c {
            '|' if !in_regex => {
                parts.push(&body[start..i]);
                start = i + 1;
                alt_start = true;
                continue;
            }
            '/' if alt_start => in_regex = true,
            // A "/" followed by spaces and a "|" (or the end) closes the regex.
            '/' if in_regex => {
                let rest = body[i + 1..].trim_start();
                if rest.is_empty() || rest.starts_with('|') {
                    in_regex = false;
                }
            }
            _ => {}
        }
        if !c.is_whitespace() {
            alt_start = false;
        }
    }
    parts.push(&body[start..]);
    // An unclosed "/…" is no regex: split it at every "|" like text.
    if in_regex {
        let last = parts.pop().unwrap_or_default();
        parts.extend(last.split('|'));
    }
    parts
}

/// Splits a filter text into the negation flag and its alternatives (none = filters nothing).
fn split(input: &str) -> (bool, Vec<Alternative<'_>>) {
    let input = input.trim();
    let (negate, body) = match input.strip_prefix('!') {
        Some(rest) => (true, rest),
        None => (false, input),
    };
    let alternatives = split_alternatives(body)
        .into_iter()
        .map(str::trim)
        .filter(|a| !a.is_empty() && *a != "=")
        .map(|a| match a.strip_prefix('=') {
            Some(exact) => Alternative {
                text: exact.trim(),
                exact: true,
            },
            None => Alternative {
                text: a,
                exact: false,
            },
        })
        .collect();
    (negate, alternatives)
}

/// A number of a filter or a cell: optional sign, digits, optional decimal point (`-1.5`).
fn number(s: &str) -> Option<f64> {
    let s = s.trim();
    let digits = s.strip_prefix(['-', '+']).unwrap_or(s);
    let valid = digits.bytes().any(|b| b.is_ascii_digit())
        && digits.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        && digits.bytes().filter(|&b| b == b'.').count() <= 1;
    valid.then(|| s.parse().ok()).flatten()
}

/// Only digits (a bound of `a-b`).
fn unsigned(s: &str) -> Option<f64> {
    let s = s.trim();
    (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        .then(|| s.parse().ok())
        .flatten()
}

/// The numbers selected by a numeric alternative (`>n`, `>=n`, `<n`, `<=n`, `a-b`, `a..b`).
fn number_range(text: &str) -> Option<NumRange> {
    use Bound::{Excluded, Included, Unbounded};
    if let Some(n) = text.strip_prefix(">=") {
        return number(n).map(|n| (Included(n), Unbounded));
    }
    if let Some(n) = text.strip_prefix('>') {
        return number(n).map(|n| (Excluded(n), Unbounded));
    }
    if let Some(n) = text.strip_prefix("<=") {
        return number(n).map(|n| (Unbounded, Included(n)));
    }
    if let Some(n) = text.strip_prefix('<') {
        return number(n).map(|n| (Unbounded, Excluded(n)));
    }
    if let Some((from, to)) = text.split_once('-')
        && let (Some(from), Some(to)) = (unsigned(from), unsigned(to))
    {
        return Some((Included(from), Included(to)));
    }
    let (from, to) = text.split_once("..")?;
    let bound = |s: &str| match s.trim() {
        "" => Some(Unbounded),
        s => number(s).map(Included),
    };
    match (bound(from)?, bound(to)?) {
        (Unbounded, Unbounded) => None,
        range => Some(range),
    }
}

/// An address alternative; `Ok(None)` when the text is no address alternative (it is then
/// matched as text).
fn net(text: &str) -> Result<Option<Net>, FilterError> {
    match text.to_ascii_lowercase().as_str() {
        "loopback" => return Ok(Some(Net::Loopback)),
        "private" => return Ok(Some(Net::Private)),
        "public" => return Ok(Some(Net::Public)),
        _ => {}
    }
    let Some((addr, prefix)) = text.split_once('/') else {
        return Ok(None);
    };
    let Ok(addr) = addr.trim().parse::<IpAddr>() else {
        return Ok(None);
    };
    let max = if addr.is_ipv4() { 32 } else { 128 };
    match prefix.trim().parse::<u8>() {
        Ok(p) if p <= max => Ok(Some(Net::Cidr(addr.to_canonical(), p))),
        _ => Err(FilterError::Network(text.to_owned())),
    }
}

/// The first `prefix` bits of two addresses of the same family are equal.
fn same_prefix(a: IpAddr, b: IpAddr, prefix: u8) -> bool {
    let (a, b, bits) = match (a, b) {
        (IpAddr::V4(a), IpAddr::V4(b)) => (u32::from(a) as u128, u32::from(b) as u128, 32),
        (IpAddr::V6(a), IpAddr::V6(b)) => (u128::from(a), u128::from(b), 128),
        _ => return false,
    };
    let shift = bits - u32::from(prefix);
    shift >= bits || (a >> shift) == (b >> shift)
}

fn is_private_v4(ip: Ipv4Addr) -> bool {
    // RFC 1918, shared address space (CGNAT, RFC 6598), link-local.
    let o = ip.octets();
    ip.is_private() || ip.is_link_local() || (o[0] == 100 && (o[1] & 0xC0) == 64)
}

fn is_private_v6(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    // Unique local fc00::/7, link-local fe80::/10.
    (first & 0xFE00) == 0xFC00 || (first & 0xFFC0) == 0xFE80
}

impl Net {
    fn contains(self, ip: IpAddr) -> bool {
        let ip = ip.to_canonical();
        match self {
            Net::Cidr(net, prefix) => same_prefix(net, ip, prefix),
            Net::Loopback => ip.is_loopback(),
            Net::Private => match ip {
                IpAddr::V4(v4) => is_private_v4(v4),
                IpAddr::V6(v6) => is_private_v6(v6),
            },
            Net::Public => {
                let special = ip.is_loopback() || ip.is_unspecified() || ip.is_multicast();
                let private = match ip {
                    IpAddr::V4(v4) => is_private_v4(v4) || v4.is_broadcast(),
                    IpAddr::V6(v6) => is_private_v6(v6),
                };
                !special && !private
            }
        }
    }
}

impl ColumnFilter {
    /// Compiles a filter text for a column of the given kind; `Ok(None)` when it filters nothing.
    pub fn parse(input: &str, kind: FilterKind) -> Result<Option<ColumnFilter>, FilterError> {
        let (negate, alternatives) = split(input);
        if alternatives.is_empty() {
            return Ok(None);
        }
        let (mut ranges, mut nets, mut texts) = (Vec::new(), Vec::new(), Vec::new());
        for a in &alternatives {
            if a.exact {
                texts.push(format!("^{}$", regex::escape(a.text)));
                continue;
            }
            if let Some(re) = a.text.strip_prefix('/').and_then(|r| r.strip_suffix('/')) {
                // Checked on its own, so the error points at this alternative.
                RegexBuilder::new(re)
                    .build()
                    .map_err(|e| FilterError::Regex(e.to_string()))?;
                texts.push(format!("(?:{re})"));
                continue;
            }
            if kind == FilterKind::Number
                && let Some(range) = number_range(a.text)
            {
                ranges.push(range);
                continue;
            }
            if kind == FilterKind::Address
                && let Some(n) = net(a.text)?
            {
                nets.push(n);
                continue;
            }
            texts.push(regex::escape(a.text));
        }
        let re = match texts.is_empty() {
            true => None,
            false => Some(
                RegexBuilder::new(&texts.join("|"))
                    .case_insensitive(true)
                    .build()
                    .map_err(|e| FilterError::Regex(e.to_string()))?,
            ),
        };
        Ok(Some(ColumnFilter {
            re,
            ranges,
            nets,
            negate,
        }))
    }

    pub fn matches(&self, value: &str) -> bool {
        let hit = self.re.as_ref().is_some_and(|re| re.is_match(value))
            || (!self.ranges.is_empty()
                && number(value).is_some_and(|v| self.ranges.iter().any(|r| r.contains(&v))))
            || (!self.nets.is_empty()
                && value
                    .trim()
                    .parse::<IpAddr>()
                    .is_ok_and(|ip| self.nets.iter().any(|n| n.contains(ip))));
        hit != self.negate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(filter: &str, value: &str) -> bool {
        m_kind(filter, FilterKind::Text, value)
    }

    fn num(filter: &str, value: &str) -> bool {
        m_kind(filter, FilterKind::Number, value)
    }

    fn addr(filter: &str, value: &str) -> bool {
        m_kind(filter, FilterKind::Address, value)
    }

    fn m_kind(filter: &str, kind: FilterKind, value: &str) -> bool {
        ColumnFilter::parse(filter, kind)
            .unwrap()
            .unwrap()
            .matches(value)
    }

    #[test]
    fn syntax() {
        for empty in ["", "  ! ", "| ="] {
            assert!(
                ColumnFilter::parse(empty, FilterKind::Text)
                    .unwrap()
                    .is_none()
            );
        }
        // contains, case-insensitive
        assert!(m("chrome", "Chrome.exe"));
        assert!(!m("chrome", "firefox.exe"));
        // alternatives with spaces around them
        assert!(m("chrome | edge", "msedge.exe"));
        assert!(!m("chrome|edge", "svchost.exe"));
        // exact match
        assert!(m("=listen", "Listen"));
        assert!(!m("=listen", "Listening"));
        // negation applies to all alternatives
        assert!(m("!svchost", "System"));
        assert!(!m("!svchost|system", "System"));
        assert!(m("!=listen", "Established"));
        // special characters are literal
        assert!(m("a.b(", "xa.b(y"));
        assert!(!m("a.b", "axb"));
    }

    #[test]
    fn regular_expressions() {
        assert!(m("/^svc.*\\.exe$/", "svchost.exe"));
        assert!(!m("/^svc.*\\.exe$/", "my svchost.exe"));
        // case-insensitive, combined with other alternatives and negation
        assert!(m("/^CHROME/|edge", "chrome.exe"));
        assert!(m("/^CHROME/|edge", "msedge.exe"));
        assert!(!m("!/^c/", "chrome.exe"));
        // "/" alone or "//" are no regex: "//" is the empty regex, "/" text
        assert!(m("/", "a/b"));
        assert!(!m("/", "ab"));
        // invalid regex: an error, not a filter
        assert!(matches!(
            ColumnFilter::parse("ok|/(/", FilterKind::Text),
            Err(FilterError::Regex(_))
        ));
        // "|" inside a regex belongs to it; around it separates alternatives
        assert!(m("/^a/|/b$/", "xb"));
        assert!(m("/^(code|claude)/", "Claude.exe"));
        assert!(!m("/^(code|claude)/", "vscode.exe"));
        assert!(m(r"svchost | /^(code|claude)\.exe$/ | edge", "code.exe"));
        assert!(m(r"svchost | /^(code|claude)\.exe$/ | edge", "msedge.exe"));
        assert!(m("!/^(code|claude)/", "svchost.exe"));
        // "a/b" inside a regex does not close it, an unclosed "/…" is text split at "|"
        assert!(m("/^c:/win|x/", "C:/WINDOWS"));
        assert!(m("/abc|def", "def"));
        assert!(m("/abc|def", "x/abc"));
        assert_eq!(
            split_alternatives("a| /x|y/ |b|/z"),
            ["a", " /x|y/ ", "b", "/z"]
        );
    }

    #[test]
    fn numeric_conditions() {
        // every operator
        assert!(num(">1024", "49152"));
        assert!(!num(">1024", "1024"));
        assert!(num(">=1024", "1024"));
        assert!(num("<1024", "443"));
        assert!(!num("<1024", "1024"));
        assert!(num("<=3", "3"));
        assert!(num("> 10", " 11 "));
        // numbers, not texts: 9 < 10, while "9" > "10" as text
        assert!(num("<10", "9"));
        assert!(!num(">10", "9"));
        // a-b and a..b ranges (inclusive), open bounds
        assert!(num("1000-2000", "1000"));
        assert!(num("1000 - 2000", "2000"));
        assert!(!num("1000-2000", "2001"));
        assert!(num("500..599", "599"));
        assert!(num("49152..", "65535"));
        assert!(num("..1023", "80"));
        // exact port vs. contains
        assert!(num("=443", "443"));
        assert!(!num("=443", "4430"));
        assert!(num("443", "4430"));
        // combined with alternatives and negation
        assert!(num("80|443|8000-8100", "8080"));
        assert!(!num("80|443|8000-8100", "22"));
        assert!(num("!0", "5"));
        assert!(!num("!<1024", "135"));
        // a cell that is not a number never matches a numeric alternative
        for cell in ["", "-", "abc", "1.2.3"] {
            assert!(!num(">=0|<0", cell), "{cell:?}");
            assert!(num("!>=0|<0", cell), "{cell:?}");
        }
        // in a text column the same input is text
        assert!(m("1000-2000", "x1000-2000"));
        assert!(!m(">10", "11"));
    }

    #[test]
    fn address_alternatives() {
        // CIDR, IPv4 and IPv6
        assert!(addr("10.0.0.0/8", "10.20.30.40"));
        assert!(!addr("10.0.0.0/8", "11.0.0.1"));
        assert!(addr("192.168.1.0/24", "192.168.1.255"));
        assert!(!addr("192.168.1.0/24", "192.168.2.1"));
        assert!(addr("0.0.0.0/0", "8.8.8.8"));
        assert!(!addr("0.0.0.0/0", "::1"));
        assert!(addr("fe80::/10", "fe80::1c2b:3a4d"));
        assert!(!addr("fe80::/10", "fec0::1"));
        assert!(addr("2001:db8::/32", "2001:db8:1::5"));
        assert!(addr("10.1.2.3/32", "10.1.2.3"));
        // an IPv4-mapped IPv6 address belongs to the IPv4 network
        assert!(addr("10.0.0.0/8", "::ffff:10.1.1.1"));
        // keywords
        assert!(addr("loopback", "127.0.0.1"));
        assert!(addr("LOOPBACK", "::1"));
        assert!(!addr("loopback", "10.0.0.1"));
        for private in [
            "10.1.1.1",
            "172.16.5.4",
            "192.168.0.10",
            "169.254.1.1",
            "100.64.0.1",
        ] {
            assert!(addr("private", private), "{private}");
            assert!(!addr("public", private), "{private}");
        }
        assert!(addr("private", "fd12:3456::1"));
        assert!(addr("private", "fe80::1"));
        for public in ["8.8.8.8", "140.82.112.3", "2606:4700::1111"] {
            assert!(addr("public", public), "{public}");
            assert!(!addr("private", public), "{public}");
        }
        for neither in [
            "0.0.0.0",
            "::",
            "127.0.0.1",
            "224.0.0.251",
            "255.255.255.255",
        ] {
            assert!(!addr("public", neither), "{neither}");
        }
        // empty remote address (listener, UDP): no address alternative matches
        assert!(!addr("public|private|loopback|0.0.0.0/0", ""));
        assert!(addr("!public", ""));
        // combined with text and negation
        assert!(addr("!loopback|private", "8.8.8.8"));
        assert!(!addr("!loopback|private", "192.168.1.1"));
        assert!(addr("public|=::", "::"));
        assert!(addr("192.168", "192.168.1.1"));
        // invalid networks: an error
        for bad in ["10.0.0.0/33", "fe80::/129", "10.0.0.0/x", "10.0.0.0/"] {
            assert!(
                matches!(
                    ColumnFilter::parse(bad, FilterKind::Address),
                    Err(FilterError::Network(_))
                ),
                "{bad}"
            );
        }
        // not an address before the "/": text
        assert!(addr("abc/8", "x abc/8"));
        // keywords are text in other columns
        assert!(m("private", "private.exe"));
    }

    #[test]
    fn five_thousand_rows_are_fast() {
        let f = ColumnFilter::parse("!loopback|10.0.0.0/8|/^19/", FilterKind::Address)
            .unwrap()
            .unwrap();
        let values: Vec<String> = (0..5000u32)
            .map(|i| std::net::Ipv4Addr::from(i.wrapping_mul(2_654_435_761)).to_string())
            .collect();
        let start = std::time::Instant::now();
        let hits = values.iter().filter(|v| f.matches(v)).count();
        let took = start.elapsed();
        assert!(hits > 0);
        // A frame is 16 ms; even a debug build stays far below.
        assert!(took.as_millis() < 50, "{took:?}");
    }
}
