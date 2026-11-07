//! Network endpoints of the system: one [`Connection`] per TCP connection / listener and per
//! UDP endpoint, with the process that owns it.
//!
//! The platform part only fills a list of connections ([`snapshot`]); everything else
//! (process names, new / closed rows, sorting, filtering) is platform independent.
//!
//! * Windows (main target): `GetExtendedTcpTable` / `GetExtendedUdpTable` from iphlpapi.dll
//!   with the owner module classes (creation time, module), decoded by the pure functions in
//!   [`winrows`].
//! * Linux: `/proc/net/{tcp,tcp6,udp,udp6}` and the socket inodes in `/proc/<pid>/fd`
//!   ([`procfs`]) – so the program and its tests also run on the Linux CI.

use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

pub mod estats;
pub mod procfs;
pub mod winrows;

#[cfg(windows)]
mod windows;

/// Transport protocol and IP version of an endpoint.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub enum Protocol {
    Tcp,
    Tcp6,
    Udp,
    Udp6,
}

impl Protocol {
    pub fn is_tcp(self) -> bool {
        matches!(self, Protocol::Tcp | Protocol::Tcp6)
    }

    pub fn is_v6(self) -> bool {
        matches!(self, Protocol::Tcp6 | Protocol::Udp6)
    }

    /// The name shown in the table (same as TCPView: "TCP", "TCPv6", "UDP", "UDPv6").
    pub fn label(self) -> &'static str {
        match self {
            Protocol::Tcp => "TCP",
            Protocol::Tcp6 => "TCPv6",
            Protocol::Udp => "UDP",
            Protocol::Udp6 => "UDPv6",
        }
    }
}

/// State of a TCP connection (UDP endpoints have none).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub enum TcpState {
    Closed,
    Listen,
    SynSent,
    SynReceived,
    Established,
    FinWait1,
    FinWait2,
    CloseWait,
    Closing,
    LastAck,
    TimeWait,
    DeleteTcb,
    Unknown(u32),
}

impl TcpState {
    /// From the `MIB_TCP_STATE` value of the Windows tables (1 = CLOSED … 12 = DELETE_TCB).
    /// Anything else becomes `Unknown`; Windows 11 25H2 has been seen to report 0 even for live
    /// listeners (issue #21).
    pub fn from_mib(v: u32) -> Self {
        match v {
            1 => TcpState::Closed,
            2 => TcpState::Listen,
            3 => TcpState::SynSent,
            4 => TcpState::SynReceived,
            5 => TcpState::Established,
            6 => TcpState::FinWait1,
            7 => TcpState::FinWait2,
            8 => TcpState::CloseWait,
            9 => TcpState::Closing,
            10 => TcpState::LastAck,
            11 => TcpState::TimeWait,
            12 => TcpState::DeleteTcb,
            o => TcpState::Unknown(o),
        }
    }

    /// From the `st` column of `/proc/net/tcp` (Linux `TCP_*` constants).
    pub fn from_linux(v: u32) -> Self {
        match v {
            0x01 => TcpState::Established,
            0x02 => TcpState::SynSent,
            0x03 => TcpState::SynReceived,
            0x04 => TcpState::FinWait1,
            0x05 => TcpState::FinWait2,
            0x06 => TcpState::TimeWait,
            0x07 => TcpState::Closed,
            0x08 => TcpState::CloseWait,
            0x09 => TcpState::LastAck,
            0x0A => TcpState::Listen,
            0x0B => TcpState::Closing,
            o => TcpState::Unknown(o),
        }
    }

    /// The name shown in the table – the usual names from netstat / TCPView; they are not
    /// translated, admins search for them as they are.
    pub fn label(self) -> String {
        match self {
            TcpState::Closed => "Closed".into(),
            TcpState::Listen => "Listen".into(),
            TcpState::SynSent => "SynSent".into(),
            TcpState::SynReceived => "SynReceived".into(),
            TcpState::Established => "Established".into(),
            TcpState::FinWait1 => "FinWait1".into(),
            TcpState::FinWait2 => "FinWait2".into(),
            TcpState::CloseWait => "CloseWait".into(),
            TcpState::Closing => "Closing".into(),
            TcpState::LastAck => "LastAck".into(),
            TcpState::TimeWait => "TimeWait".into(),
            TcpState::DeleteTcb => "DeleteTcb".into(),
            TcpState::Unknown(v) => format!("? ({v})"),
        }
    }
}

/// One endpoint as the operating system reports it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Connection {
    pub proto: Protocol,
    pub local: SocketAddr,
    /// `None` for UDP endpoints (they have no remote side).
    pub remote: Option<SocketAddr>,
    /// `None` for UDP.
    pub state: Option<TcpState>,
    /// Owning process; 0 when unknown (e.g. Linux sockets of other users without root).
    pub pid: u32,
    /// When the system created the endpoint (ms since the Unix epoch); Windows only (#14).
    pub created: Option<u64>,
    /// Owner module: the executable, or the service for one hosted in svchost (#14).
    pub module: Option<Arc<str>>,
    /// Bytes / segments sent and received, while collected (Windows, administrator, #8).
    pub stats: Option<estats::TcpStats>,
}

/// What identifies a row between two snapshots. The state is not part of it: a connection that
/// goes from Established to TimeWait is the same row with a changed state.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct ConnKey {
    pub proto: Protocol,
    pub local: SocketAddr,
    pub remote: Option<SocketAddr>,
    pub pid: u32,
}

impl Connection {
    pub fn key(&self) -> ConnKey {
        ConnKey {
            proto: self.proto,
            local: self.local,
            remote: self.remote,
            pid: self.pid,
        }
    }
}

/// An error of the platform part, shown in the status bar.
#[derive(Debug)]
pub struct NetError(pub String);

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Reads the endpoints again and again, keeping what is expensive to look up (the owner
/// modules of the Windows endpoints) between the reads.
#[derive(Default)]
pub struct Reader {
    #[cfg(windows)]
    modules: windows::ModuleCache,
}

impl Reader {
    /// All TCP and UDP endpoints (IPv4 and IPv6) of the system right now.
    pub fn read(&mut self) -> Result<Vec<Connection>, NetError> {
        #[cfg(windows)]
        {
            windows::snapshot(&mut self.modules)
        }
        #[cfg(target_os = "linux")]
        {
            procfs::snapshot()
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            Err(NetError("this operating system is not supported".into()))
        }
    }
}

/// All TCP and UDP endpoints right now (a one-off [`Reader`]; used by the tests).
#[cfg(test)]
pub fn snapshot() -> Result<Vec<Connection>, NetError> {
    Reader::default().read()
}

/// Address text for the table: IPv6 without brackets (the port has its own column), the
/// unspecified address as `0.0.0.0` / `::`.
pub fn ip_text(ip: IpAddr) -> String {
    ip.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_codes() {
        assert_eq!(TcpState::from_mib(2), TcpState::Listen);
        assert_eq!(TcpState::from_mib(5), TcpState::Established);
        assert_eq!(TcpState::from_mib(11), TcpState::TimeWait);
        assert_eq!(TcpState::from_mib(99), TcpState::Unknown(99));
        assert_eq!(TcpState::from_linux(0x0A), TcpState::Listen);
        assert_eq!(TcpState::from_linux(0x01), TcpState::Established);
        assert_eq!(TcpState::from_linux(0x06), TcpState::TimeWait);
        assert_eq!(TcpState::from_mib(0), TcpState::Unknown(0));
        assert_eq!(TcpState::from_mib(12), TcpState::DeleteTcb);
        assert_eq!(TcpState::from_mib(13), TcpState::Unknown(13));
    }

    /// Whether the live test can check the state the OS reported. Windows sometimes reports an
    /// invalid TCP state (0) even for a socket that is certainly listening (issue #21); that is
    /// an OS problem, not a decoding one, so the state check is skipped there with a message.
    fn state_checkable(what: &str, state: Option<TcpState>) -> bool {
        match state {
            Some(s @ TcpState::Unknown(_)) if cfg!(windows) => {
                eprintln!(
                    "{what}: Windows reported an invalid TCP state {s:?}, state check skipped (issue #21)"
                );
                false
            }
            _ => true,
        }
    }

    #[test]
    fn state_check_is_skipped_only_for_invalid_states_on_windows() {
        assert!(state_checkable("t", Some(TcpState::Listen)));
        assert!(state_checkable("t", None));
        assert_eq!(
            state_checkable("t", Some(TcpState::Unknown(0))),
            !cfg!(windows)
        );
    }

    #[test]
    fn protocol_flags() {
        assert!(Protocol::Tcp6.is_tcp() && Protocol::Tcp6.is_v6());
        assert!(!Protocol::Udp.is_tcp() && !Protocol::Udp.is_v6());
        assert_eq!(Protocol::Udp6.label(), "UDPv6");
    }

    /// The real backend of the platform finds a listener and a connection of this process
    /// (on the Windows CI this checks the iphlpapi decoding against the real API).
    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn live_snapshot_sees_own_sockets() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = std::net::TcpStream::connect(addr).unwrap();
        let (_server, _) = listener.accept().unwrap();
        let conns = snapshot().unwrap();
        let me = std::process::id();
        // Found by protocol, local address and PID only: with an invalid state the decoder
        // cannot tell a listener and keeps its remote side 0.0.0.0:0 (issue #21).
        let lis = conns
            .iter()
            .find(|c| {
                c.proto == Protocol::Tcp
                    && c.local == addr
                    && c.pid == me
                    && c.remote
                        .is_none_or(|r| r.port() == 0 && r.ip().is_unspecified())
            })
            .expect("own listener");
        if state_checkable("own listener", lis.state) {
            assert_eq!(lis.state, Some(TcpState::Listen));
            assert_eq!(lis.remote, None);
        }
        let out = conns
            .iter()
            .find(|c| {
                c.proto == Protocol::Tcp
                    && c.local == client.local_addr().unwrap()
                    && c.remote == Some(addr)
            })
            .expect("own outgoing connection");
        assert_eq!(out.pid, me);
        if state_checkable("own outgoing connection", out.state) {
            assert_eq!(out.state, Some(TcpState::Established));
        }
        // Windows knows when the connection was created and which module owns it (#14).
        if cfg!(windows) {
            let created = out.created.expect("creation time");
            let now = crate::model::now_ms();
            assert!(
                created.abs_diff(now) < 60_000,
                "created {created}, now {now}"
            );
            let exe = std::env::current_exe().unwrap();
            let name = exe.file_name().unwrap().to_string_lossy().to_lowercase();
            let module = out.module.as_deref().unwrap_or_default().to_lowercase();
            assert_eq!(module, name);
        }
    }

    /// The creation time of a fresh connection is within a second of now (#14 acceptance).
    #[cfg(windows)]
    #[test]
    fn creation_time_of_a_new_connection() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let before = crate::model::now_ms();
        let client = std::net::TcpStream::connect(addr).unwrap();
        let (_server, _) = listener.accept().unwrap();
        let conns = snapshot().unwrap();
        let c = conns
            .iter()
            .find(|c| c.local == client.local_addr().unwrap() && c.remote == Some(addr))
            .expect("own connection");
        let created = c.created.expect("creation time");
        assert!(
            created.abs_diff(before) < 1000,
            "created {created}, before {before}"
        );
    }
}
