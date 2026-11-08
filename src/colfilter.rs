//! Filters of single table columns – the filter row under the column headers (#1). The core
//! (alternatives, exact match, negation, numeric ranges) is taken from RusTrace's `colfilter.rs`;
//! regular expressions, `a-b` ranges and the address alternatives are added here.
//!
//! Syntax of a filter text (always case-insensitive), a list of terms:
//! * `text` – the value contains `text`,
//! * `=text` – the value is exactly `text`,
//! * `/regex/` – the value matches the regular expression,
//! * `!term` – NOT: the value does not match the term (#35),
//! * `a&b` – AND: both terms match (#35),
//! * `a|b` – OR: any of the alternatives.
//!
//! Precedence `!` > `&` > `|`: `a&b|c` = (a AND b) OR c; no parentheses (a regex covers the rest).
//! A `|` or `&` inside `/…/` belongs to the regex.
//!
//! Number columns (PID, ports) also understand `>n`, `>=n`, `<n`, `<=n`, `a-b` and `a..b`
//! (inclusive, a `..` bound may be left out); a value that is not a number never matches them.
//!
//! Address columns also understand a network in CIDR notation (`10.0.0.0/8`, `fe80::/10`) and the
//! keywords `loopback`, `private` and `public`; an empty address never matches them.
//!
//! Spaces around `|`, `&` and after `!` are ignored; empty terms are ignored (`a&` = `a`), an empty
//! filter (or only `!`) filters nothing. An invalid regular expression or network is an error: the
//! caller shows it and does not filter.
//!
//! Before #35 a leading `!` negated the whole filter (`!a|b` = neither a nor b); [`migrate_v1`]
//! rewrites such saved texts to the new form (`!a&!b`).

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

/// The version of the filter syntax written by this program: 1 = AND / per-term NOT (#35).
pub const SYNTAX_VERSION: u32 = 1;

/// One term of a filter.
#[derive(Clone, Debug)]
enum Term {
    /// Contains / exact / regex, compiled case-insensitive.
    Text(Regex),
    Range(NumRange),
    Net(Net),
}

impl Term {
    fn matches(&self, value: &str) -> bool {
        match self {
            Term::Text(re) => re.is_match(value),
            Term::Range(r) => number(value).is_some_and(|v| r.contains(&v)),
            Term::Net(n) => value
                .trim()
                .parse::<IpAddr>()
                .is_ok_and(|ip| n.contains(ip)),
        }
    }
}

/// A compiled filter: OR of groups, each group an AND of (negated?) terms.
#[derive(Clone, Debug)]
pub struct ColumnFilter {
    groups: Vec<Vec<(bool, Term)>>,
}

/// The parts of a filter text between the separator `sep` (`|` or `&`), except that a separator
/// inside a `/regex/` term belongs to the regex (`/^(a|b)/` is one term). A regex starts with a
/// `/` at the start of a term (after spaces and `!`) and ends with a `/` followed by spaces and a
/// `|`, a `&` or the end.
fn split_outside_regex(body: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut in_regex = false;
    let mut term_start = true;
    for (i, c) in body.char_indices() {
        if !in_regex && (c == '|' || c == '&') {
            if c == sep {
                parts.push(&body[start..i]);
                start = i + 1;
            }
            term_start = true;
            continue;
        }
        match c {
            '/' if term_start => in_regex = true,
            '/' if in_regex => {
                let rest = body[i + 1..].trim_start();
                if rest.is_empty() || rest.starts_with(['|', '&']) {
                    in_regex = false;
                }
            }
            _ => {}
        }
        if !c.is_whitespace() && c != '!' {
            term_start = false;
        }
    }
    parts.push(&body[start..]);
    // An unclosed "/…" is no regex: split it at every separator like text.
    if in_regex {
        let last = parts.pop().unwrap_or_default();
        parts.extend(last.split(sep));
    }
    parts
}

/// The alternatives (`|`) of a filter text, each a list of AND terms: (negated, text, exact).
/// Empty terms and groups are left out.
fn split(input: &str) -> Vec<Vec<(bool, &str, bool)>> {
    split_outside_regex(input, '|')
        .into_iter()
        .map(|alt| {
            split_outside_regex(alt, '&')
                .into_iter()
                .filter_map(|t| {
                    let mut t = t.trim();
                    let mut negate = false;
                    while let Some(rest) = t.strip_prefix('!') {
                        negate = !negate;
                        t = rest.trim_start();
                    }
                    let (text, exact) = match t.strip_prefix('=') {
                        Some(e) => (e.trim(), true),
                        None => (t, false),
                    };
                    (!text.is_empty()).then_some((negate, text, exact))
                })
                .collect::<Vec<_>>()
        })
        .filter(|g| !g.is_empty())
        .collect()
}

/// Rewrites a filter text saved before #35 (syntax 0) to the current syntax: a leading `!`
/// negated all alternatives, so `!a|b` (neither a nor b) becomes `!a&!b`. A leading `!` with one
/// term, and every text without a leading `!`, stay as they are; a `|` inside `/regex/` is not
/// split.
pub fn migrate_v1(text: &str) -> String {
    let trimmed = text.trim();
    let Some(body) = trimmed.strip_prefix('!') else {
        return text.to_owned();
    };
    let parts: Vec<&str> = split_outside_regex(body, '|')
        .into_iter()
        .map(str::trim)
        .filter(|p| !p.is_empty() && *p != "=")
        .collect();
    if parts.len() < 2 {
        return text.to_owned();
    }
    parts
        .iter()
        .map(|p| format!("!{p}"))
        .collect::<Vec<_>>()
        .join("&")
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
        let mut groups = Vec::new();
        for alt in split(input) {
            let mut terms = Vec::with_capacity(alt.len());
            for (negate, text, exact) in alt {
                terms.push((negate, term(text, exact, kind)?));
            }
            groups.push(terms);
        }
        Ok((!groups.is_empty()).then_some(ColumnFilter { groups }))
    }

    pub fn matches(&self, value: &str) -> bool {
        self.groups
            .iter()
            .any(|g| g.iter().all(|(negate, t)| t.matches(value) != *negate))
    }
}

/// Compiles one term.
fn term(text: &str, exact: bool, kind: FilterKind) -> Result<Term, FilterError> {
    let pattern = if exact {
        format!("^{}$", regex::escape(text))
    } else if let Some(re) = text.strip_prefix('/').and_then(|r| r.strip_suffix('/')) {
        re.to_owned()
    } else if kind == FilterKind::Number
        && let Some(range) = number_range(text)
    {
        return Ok(Term::Range(range));
    } else if kind == FilterKind::Address
        && let Some(n) = net(text)?
    {
        return Ok(Term::Net(n));
    } else {
        regex::escape(text)
    };
    RegexBuilder::new(&pattern)
        .case_insensitive(true)
        .build()
        .map(Term::Text)
        .map_err(|e| FilterError::Regex(e.to_string()))
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
        // negation of one term (#35): "!a|b" = (not a) or b
        assert!(m("!svchost", "System"));
        assert!(m("!svchost|system", "System"));
        assert!(!m("!svchost|system", "svchost.exe"));
        assert!(!m("!svchost&!system", "System"));
        assert!(m("!=listen", "Established"));
        assert!(m("! = listen", "Established"));
        assert!(m("!!listen", "Listen"), "double negation");
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
            split_outside_regex("a| /x|y/ |b|/z", '|'),
            ["a", " /x|y/ ", "b", "/z"]
        );
        // "&" inside a regex belongs to it too, also after "!"
        assert_eq!(
            split_outside_regex("!/a&b/ & c|d", '&'),
            ["!/a&b/ ", " c|d"]
        );
        assert!(m("/^at&t/", "AT&T"));
        assert!(m("!/^(a|b)/&x", "x"));
        assert!(!m("!/^(a|b)/&x", "ax"));
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
            assert!(num("!>=0&!<0", cell), "{cell:?}");
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
        assert!(addr("!loopback&!private", "8.8.8.8"));
        assert!(!addr("!loopback&!private", "192.168.1.1"));
        assert!(addr("!loopback|private", "192.168.1.1"));
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
    fn and_not_and_precedence() {
        // The examples of #35 (also in the filter tooltips of #36).
        // Ports 1-99: AND of two ranges (OR would match every port).
        for (port, hit) in [("0", false), ("1", true), ("99", true), ("100", false)] {
            assert_eq!(num(">0&<100", port), hit, "{port}");
            assert!(num(">0|<100", port), "{port}");
        }
        // Every port except 443 (`!=443`; `!443` excludes every port containing 443).
        assert!(num(">0&!=443", "4430") && !num(">0&!=443", "443"));
        assert!(!num(">0&!443", "443") && !num(">0&!443", "4430"));
        assert!(num(">=49152&!50000-50100", "49152"));
        assert!(!num(">=49152&!50000-50100", "50050"));
        assert!(num(">=49152&!50000-50100", "50101"));
        // State: everything except Listen and Established.
        assert!(m("!Listen&!Established", "TimeWait"));
        assert!(!m("!Listen&!Established", "Listen"));
        assert!(!m("!Listen&!Established", "Established"));
        // Process: contains svc but is not svchost.
        assert!(m("svc&!svchost", "MsMpEngSvc.exe"));
        assert!(!m("svc&!svchost", "svchost.exe"));
        // Remote address: public, but not in 13.64.0.0/11.
        assert!(addr("public&!13.64.0.0/11", "8.8.8.8"));
        assert!(!addr("public&!13.64.0.0/11", "13.70.1.1"));
        assert!(!addr("public&!13.64.0.0/11", "10.0.0.1"));
        // Precedence: "a&b|c" = (a and b) or c.
        assert!(m("443&public|=80", "443 public"));
        assert!(!m("443&public|=80", "443"));
        assert!(m("443&public|=80", "80"));
        // Spaces and empty terms are ignored.
        assert!(m(" a & ! b ", "a"));
        assert!(m("a&", "xa") && m("&a&&", "xa"));
        assert!(
            ColumnFilter::parse(" & | ! & ", FilterKind::Text)
                .unwrap()
                .is_none()
        );
        // An error names its term.
        assert!(matches!(
            ColumnFilter::parse("x&10.0.0.0/40", FilterKind::Address),
            Err(FilterError::Network(v)) if v == "10.0.0.0/40"
        ));
    }

    #[test]
    fn migration_of_old_negations() {
        assert_eq!(migrate_v1("!a|b"), "!a&!b");
        assert_eq!(migrate_v1(" ! svchost | =System |"), "!svchost&!=System");
        assert_eq!(
            migrate_v1("!/^(code|claude)/|edge"),
            "!/^(code|claude)/&!edge"
        );
        // One term, no leading "!", empty: unchanged.
        for same in ["!a", "! /x|y/", "a|b", "a|!b", "", "!", "!|"] {
            assert_eq!(migrate_v1(same), same, "{same}");
        }
        // The rewritten filter selects the same values as the old one did.
        for v in ["svchost.exe", "System", "chrome.exe"] {
            let old_hit = !(v.to_lowercase().contains("svchost") || v == "System");
            assert_eq!(m(&migrate_v1("!svchost|=system"), v), old_hit, "{v}");
        }
    }

    #[test]
    fn five_thousand_rows_are_fast() {
        let f = ColumnFilter::parse("!loopback&!10.0.0.0/8|/^19/", FilterKind::Address)
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
