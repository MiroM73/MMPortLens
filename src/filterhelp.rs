//! Examples for the column filter fields (#36): two to four filters per column with the key of
//! their translated explanation, and the fixed values of the State and Protocol columns. Shown in
//! the tooltip of a filter field and of the pattern of a color rule; the filter texts stay
//! literal, only the explanations are translated.

use crate::columns::ColumnId;
use crate::net::TcpState;
use crate::t;

/// The examples of a column: (filter text, `t!` key of the explanation).
pub fn examples(col: ColumnId) -> &'static [(&'static str, &'static str)] {
    use ColumnId::*;
    match col {
        Process => &[
            ("chrome|firefox", "fex.either"),
            ("svc&!svchost", "fex.svc_not_svchost"),
            ("!svchost", "fex.not_svchost"),
            ("/^ms/", "fex.starts_ms"),
        ],
        Pid => &[("=4", "fex.pid_system"), (">1000", "fex.pid_above")],
        Protocol => &[
            ("tcp", "fex.tcp"),
            ("=UDPv6", "fex.udp6"),
            ("v6", "fex.ipv6"),
        ],
        State => &[
            ("Established|Listen", "fex.est_or_listen"),
            ("!Listen", "fex.not_listen"),
            ("!Listen&!Established", "fex.neither_state"),
            (r"/\(\d+\)/", "fex.invalid_state"),
        ],
        LocalAddress => &[
            ("=0.0.0.0|=::", "fex.all_interfaces"),
            ("!loopback", "fex.not_loopback"),
            ("10.0.0.0/8", "fex.network"),
        ],
        RemoteAddress => &[
            ("public", "fex.public"),
            ("!loopback", "fex.not_loopback"),
            ("10.0.0.0/8", "fex.network"),
            ("public&!13.64.0.0/11", "fex.public_except"),
        ],
        LocalPort | RemotePort => &[
            ("443|80", "fex.https_http"),
            (">=49152", "fex.dynamic_ports"),
            ("1-1023", "fex.well_known_ports"),
            (">0&!=443", "fex.except_443"),
        ],
        RemoteHost | LocalHost => &[
            ("microsoft", "fex.host_contains"),
            ("!localhost", "fex.not_localhost"),
            (".com&!microsoft", "fex.com_not_microsoft"),
        ],
        Service => &[
            ("https|http", "fex.web"),
            ("dns|mdns|llmnr", "fex.name_resolution"),
        ],
        WinServices => &[("dnscache|nlasvc", "fex.either"), ("/./", "fex.not_empty")],
        Module => &[
            ("dnscache", "fex.module_service"),
            ("=svchost.exe", "fex.module_svchost"),
        ],
        ProcessPath => &[
            (r"\windows\", "fex.in_windows"),
            (r"!\windows\&!\program files", "fex.outside_windows"),
        ],
        User => &[
            ("system", "fex.user_system"),
            ("!nt authority", "fex.user_normal"),
        ],
        CommandLine => &[
            ("-k netsvcs", "fex.svchost_group"),
            ("powershell&-enc", "fex.powershell_enc"),
        ],
        Company => &[
            ("microsoft", "fex.company_ms"),
            ("/./&!microsoft", "fex.company_other"),
        ],
        Description => &[("update", "fex.description_update")],
        FirstSeen => &[("/^09:/", "fex.seen_hour")],
        Created => &[
            ("/^09:/", "fex.created_hour"),
            ("-", "fex.created_before_today"),
        ],
        Duration => &[("/ s$/&!min", "fex.under_minute"), ("h|d", "fex.over_hour")],
        Source => &[("=ETW", "fex.etw")],
        BytesSent | BytesReceived => &[("/./&!=0 B", "fex.traffic"), ("MB|GB", "fex.megabytes")],
        RateSent | RateReceived => &[("/./&!=0 B/s", "fex.rate_now"), ("MB/s", "fex.rate_mb")],
        PacketsSent | PacketsReceived => &[(">0", "fex.packets"), (">1000", "fex.packets_many")],
    }
}

/// The values a column can have, for the columns with a fixed set (State, Protocol).
pub fn values(col: ColumnId) -> Option<Vec<String>> {
    match col {
        ColumnId::State => {
            let mut v: Vec<String> = (1..=12)
                .map(|n| crate::columns::state_text(TcpState::from_mib(n)))
                .collect();
            v.push(t!("state.invalid", value = "n"));
            Some(v)
        }
        ColumnId::Protocol => Some(["TCP", "TCPv6", "UDP", "UDPv6"].map(str::to_owned).to_vec()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::colfilter::ColumnFilter;

    fn filter(col: ColumnId, text: &str) -> ColumnFilter {
        ColumnFilter::parse(text, col.filter_kind())
            .unwrap_or_else(|e| panic!("{col:?} {text}: {e:?}"))
            .unwrap_or_else(|| panic!("{col:?} {text}: filters nothing"))
    }

    #[test]
    fn every_column_has_working_examples() {
        for col in ColumnId::ALL {
            let ex = examples(col);
            assert!(!ex.is_empty(), "{col:?}");
            for (text, key) in ex {
                filter(col, text);
                let what = t!(key);
                assert!(!what.starts_with("fex."), "missing text {key}");
            }
        }
    }

    #[test]
    fn examples_select_what_they_say() {
        use crate::columns::{bytes, rate};
        use crate::timefmt::duration;
        let hit = |col, text, value: &str| filter(col, text).matches(value);
        // State: the invalid states, whatever the language of "Invalid".
        let state = |s| crate::columns::state_text(s);
        assert!(hit(
            ColumnId::State,
            r"/\(\d+\)/",
            &state(TcpState::Unknown(0))
        ));
        assert!(!hit(ColumnId::State, r"/\(\d+\)/", "Listen"));
        assert!(hit(ColumnId::State, "!Listen&!Established", "TimeWait"));
        // Protocol.
        assert!(hit(ColumnId::Protocol, "tcp", "TCPv6"));
        assert!(!hit(ColumnId::Protocol, "=UDPv6", "UDP"));
        // Ports.
        assert!(hit(ColumnId::LocalPort, ">=49152", "50000"));
        assert!(hit(ColumnId::RemotePort, ">0&!=443", "4430"));
        assert!(!hit(ColumnId::RemotePort, ">0&!=443", "443"));
        // Durations as the table shows them.
        let under = |ms| hit(ColumnId::Duration, "/ s$/&!min", &duration(ms));
        assert!(under(5_123) && under(42_500) && !under(185_000) && !under(7_500_000));
        let over = |ms| hit(ColumnId::Duration, "h|d", &duration(ms));
        assert!(!over(185_000) && over(7_620_000) && over(360_000_000));
        // Bytes and rates as the table shows them; empty cells (no counters) do not match.
        let traffic = |v: &str| hit(ColumnId::BytesSent, "/./&!=0 B", v);
        assert!(traffic(&bytes(1536)) && !traffic(&bytes(0)) && !traffic(""));
        assert!(hit(ColumnId::BytesReceived, "MB|GB", &bytes(12_900_000)));
        let now = |v: &str| hit(ColumnId::RateSent, "/./&!=0 B/s", v);
        assert!(now(&rate(2048.0)) && !now(&rate(0.0)) && !now(""));
        // Paths, created times.
        assert!(hit(
            ColumnId::ProcessPath,
            r"!\windows\&!\program files",
            r"D:\Tools\x.exe"
        ));
        assert!(!hit(
            ColumnId::ProcessPath,
            r"!\windows\&!\program files",
            r"C:\Windows\System32\svchost.exe"
        ));
        assert!(hit(ColumnId::Created, "-", "2026-11-07 10:00:00"));
        assert!(!hit(ColumnId::Created, "-", "20:13:31.009"));
        assert!(hit(ColumnId::Created, "/^09:/", "09:13:31.009"));
    }

    #[test]
    fn fixed_values() {
        let states = values(ColumnId::State).unwrap();
        assert_eq!(states.len(), 13);
        assert_eq!(states[0], "Closed");
        assert!(states.contains(&"TimeWait".to_owned()));
        assert!(states[12].contains('n'), "{}", states[12]);
        assert_eq!(values(ColumnId::Protocol).unwrap().len(), 4);
        assert!(values(ColumnId::Process).is_none());
    }
}
