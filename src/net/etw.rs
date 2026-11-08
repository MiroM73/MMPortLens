//! Short-lived connections from Event Tracing for Windows (#4).
//!
//! Polling the tables misses every connection that opens and closes between two refreshes.
//! A real-time ETW session on the provider *Microsoft-Windows-Kernel-Network* sees them all:
//! TCP connect / accept / disconnect / send / receive and UDP send / receive, each with the
//! process, the addresses and the size.
//!
//! This file is the pure part, tested on every platform:
//!
//! * [`decode`] – one event (id + payload bytes) into an [`EtwEvent`],
//! * [`Collector`] – the events of the poll thread between two snapshots, aggregated into one
//!   [`Seen`] per endpoint when a snapshot is taken.
//!
//! The session itself (`StartTraceW` / `OpenTraceW` / `ProcessTrace`, Windows only, needs an
//! administrator) is in `etw_win.rs`; [`start`] hides the platform.

// The decoder is only called by the Windows session (and tested on every platform).
#![cfg_attr(not(windows), allow(dead_code))]

use super::{ConnKey, Protocol};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

/// `{7DD42A49-5329-4832-8DFD-43D979153A88}` – Microsoft-Windows-Kernel-Network.
pub const PROVIDER: (u32, u16, u16, [u8; 8]) = (
    0x7DD4_2A49,
    0x5329,
    0x4832,
    [0x8D, 0xFD, 0x43, 0xD9, 0x79, 0x15, 0x3A, 0x88],
);
/// Keywords `KERNEL_NETWORK_KEYWORD_IPV4` | `KERNEL_NETWORK_KEYWORD_IPV6`.
pub const KEYWORDS: u64 = 0x10 | 0x20;

/// What an event says happened.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    Send,
    Recv,
    /// The local side connected (client).
    Connect,
    /// A listener accepted the connection (server).
    Accept,
    Disconnect,
    /// Retransmit, reconnect attempt, copy on behalf of the user: the connection exists.
    Other,
}

/// One decoded event of the kernel network provider.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EtwEvent {
    /// ms since the Unix epoch.
    pub time: u64,
    pub op: Op,
    pub proto: Protocol,
    pub local: SocketAddr,
    pub remote: SocketAddr,
    /// The owning process (0 when the kernel does not know it).
    pub pid: u32,
    /// Bytes of a send / receive.
    pub size: u32,
}

/// The protocol and operation of an event id (from the provider manifest:
/// `wevtutil gp Microsoft-Windows-Kernel-Network /ge /gm`).
fn kind(id: u16) -> Option<(Protocol, Op)> {
    use Op::*;
    use Protocol::*;
    Some(match id {
        10 => (Tcp, Send),
        11 => (Tcp, Recv),
        12 => (Tcp, Connect),
        13 => (Tcp, Disconnect),
        14 | 16 | 18 => (Tcp, Other),
        15 => (Tcp, Accept),
        26 => (Tcp6, Send),
        27 => (Tcp6, Recv),
        28 => (Tcp6, Connect),
        29 => (Tcp6, Disconnect),
        30 | 32 | 34 => (Tcp6, Other),
        31 => (Tcp6, Accept),
        42 => (Udp, Send),
        43 => (Udp, Recv),
        58 => (Udp6, Send),
        59 => (Udp6, Recv),
        // 17 / 49 (connection attempt failed) carry no addresses.
        _ => return None,
    })
}

fn dword(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Ports are in network byte order (`win:Port`).
fn port(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

/// Decodes one event. Every event of the provider starts with the same fields:
/// `PID: u32, size: u32, daddr, saddr, dport: u16, sport: u16` – `daddr` / `dport` are the
/// remote side and `saddr` / `sport` the local one for sends and receives alike; the addresses
/// are 4 bytes (IPv4) or 16 bytes (IPv6) as they go over the wire.
pub fn decode(id: u16, time: u64, data: &[u8]) -> Option<EtwEvent> {
    let (proto, op) = kind(id)?;
    let pid = dword(data, 0)?;
    let size = dword(data, 4)?;
    let (remote_ip, local_ip, ports): (IpAddr, IpAddr, usize) = if proto.is_v6() {
        let d: [u8; 16] = data.get(8..24)?.try_into().ok()?;
        let s: [u8; 16] = data.get(24..40)?.try_into().ok()?;
        (Ipv6Addr::from(d).into(), Ipv6Addr::from(s).into(), 40)
    } else {
        let d: [u8; 4] = data.get(8..12)?.try_into().ok()?;
        let s: [u8; 4] = data.get(12..16)?.try_into().ok()?;
        (Ipv4Addr::from(d).into(), Ipv4Addr::from(s).into(), 16)
    };
    Some(EtwEvent {
        time,
        op,
        proto,
        local: SocketAddr::new(local_ip, port(data, ports + 2)?),
        remote: SocketAddr::new(remote_ip, port(data, ports)?),
        pid,
        size,
    })
}

/// A FILETIME (100 ns since 1601) as ms since the Unix epoch.
pub fn filetime_ms(ft: i64) -> u64 {
    const EPOCH_DIFF: i64 = 116_444_736_000_000_000;
    (ft.saturating_sub(EPOCH_DIFF).max(0) / 10_000) as u64
}

/// What ETW saw of one endpoint between two snapshots.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Seen {
    /// Protocol, local and remote side (UDP: the peer of the datagrams) and process.
    pub key: ConnKey,
    /// The first and the last event.
    pub first: u64,
    pub last: u64,
    /// Time of the connect / accept (TCP): the connection was opened within the period.
    pub opened: Option<u64>,
    /// Time of the disconnect (TCP).
    pub closed: Option<u64>,
    pub bytes_out: u64,
    pub bytes_in: u64,
    /// Send / receive events (UDP: datagrams).
    pub sends: u64,
    pub recvs: u64,
}

/// The endpoint of an event: protocol + local + remote, without the process (the kernel reports
/// some events with PID 0).
type Endpoint = (Protocol, SocketAddr, SocketAddr);

/// The events received from the session, until the poll thread takes them with a snapshot.
#[derive(Default)]
pub struct Collector {
    pending: Vec<EtwEvent>,
}

/// More events than this are not kept between two snapshots (a paused poll thread, a flood);
/// the oldest are dropped.
const MAX_PENDING: usize = 500_000;

impl Collector {
    pub fn push(&mut self, e: EtwEvent) {
        if self.pending.len() >= MAX_PENDING {
            self.pending.drain(..MAX_PENDING / 10);
        }
        self.pending.push(e);
    }

    pub fn clear(&mut self) {
        self.pending.clear();
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// Takes the events older than `cutoff` (the time the tables were read: later events belong
    /// to the next snapshot) as one [`Seen`] per endpoint, in the order of their first event.
    /// The PID is the first non-zero one of the endpoint's events.
    pub fn take(&mut self, cutoff: u64) -> Vec<Seen> {
        let (old, keep): (Vec<EtwEvent>, Vec<EtwEvent>) =
            self.pending.drain(..).partition(|e| e.time < cutoff);
        self.pending = keep;
        let mut index: HashMap<Endpoint, usize> = HashMap::new();
        let mut out: Vec<Seen> = Vec::new();
        for e in old {
            let ep = (e.proto, e.local, e.remote);
            let i = *index.entry(ep).or_insert_with(|| {
                out.push(Seen {
                    key: ConnKey {
                        proto: e.proto,
                        local: e.local,
                        remote: Some(e.remote),
                        pid: e.pid,
                    },
                    first: e.time,
                    last: e.time,
                    opened: None,
                    closed: None,
                    bytes_out: 0,
                    bytes_in: 0,
                    sends: 0,
                    recvs: 0,
                });
                out.len() - 1
            });
            let s = &mut out[i];
            if s.key.pid == 0 {
                s.key.pid = e.pid;
            }
            s.first = s.first.min(e.time);
            s.last = s.last.max(e.time);
            match e.op {
                Op::Send => {
                    s.bytes_out += e.size as u64;
                    s.sends += 1;
                }
                Op::Recv => {
                    s.bytes_in += e.size as u64;
                    s.recvs += 1;
                }
                Op::Connect | Op::Accept => {
                    s.opened = Some(s.opened.map_or(e.time, |t| t.min(e.time)));
                }
                Op::Disconnect => s.closed = Some(s.closed.map_or(e.time, |t| t.max(e.time))),
                Op::Other => {}
            }
        }
        out.sort_by_key(|s| s.first);
        out
    }
}

/// Why the session does not run.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum EtwError {
    /// Only administrators may start a trace session.
    Denied,
    /// Not Windows.
    #[cfg_attr(windows, allow(dead_code))]
    Unsupported,
    Failed(String),
}

impl std::fmt::Display for EtwError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EtwError::Denied => f.write_str("access denied"),
            EtwError::Unsupported => f.write_str("ETW exists only on Windows"),
            EtwError::Failed(e) => f.write_str(e),
        }
    }
}

/// The translated message of an error (status bar, console).
pub fn error_text(e: &EtwError) -> String {
    match e {
        EtwError::Denied => crate::t!("etw.denied"),
        e => crate::t!("etw.failed", error = e),
    }
}

/// The state of the capture, reported with every snapshot.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub enum EtwStatus {
    /// Not wanted.
    #[default]
    Off,
    Running,
    Error(EtwError),
}

#[cfg(windows)]
pub use super::etw_win::Session;

/// Starts a real-time session; the events go to `tx`.
#[cfg(windows)]
pub fn start(tx: std::sync::mpsc::Sender<EtwEvent>) -> Result<Session, EtwError> {
    Session::start(tx)
}

/// No ETW outside Windows.
#[cfg(not(windows))]
pub struct Session;

#[cfg(not(windows))]
impl Session {
    pub fn stop(self) {}
}

#[cfg(not(windows))]
pub fn start(_tx: std::sync::mpsc::Sender<EtwEvent>) -> Result<Session, EtwError> {
    Err(EtwError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A payload in the provider layout.
    fn v4(pid: u32, size: u32, remote: [u8; 4], local: [u8; 4], rport: u16, lport: u16) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend(pid.to_le_bytes());
        b.extend(size.to_le_bytes());
        b.extend(remote);
        b.extend(local);
        b.extend(rport.to_be_bytes());
        b.extend(lport.to_be_bytes());
        // The rest of the template (seqnum, connid …) is not read.
        b.extend([0u8; 16]);
        b
    }

    #[test]
    fn decodes_ipv4_and_ipv6() {
        let data = v4(1234, 517, [93, 184, 216, 34], [192, 168, 1, 5], 443, 50123);
        let e = decode(10, 5000, &data).unwrap();
        assert_eq!(
            e,
            EtwEvent {
                time: 5000,
                op: Op::Send,
                proto: Protocol::Tcp,
                local: "192.168.1.5:50123".parse().unwrap(),
                remote: "93.184.216.34:443".parse().unwrap(),
                pid: 1234,
                size: 517,
            }
        );
        assert_eq!(decode(15, 0, &data).unwrap().op, Op::Accept);
        assert_eq!(decode(43, 0, &data).unwrap().proto, Protocol::Udp);
        assert_eq!(decode(17, 0, &data), None);
        assert_eq!(decode(12, 0, &data[..18]), None);

        let mut b = Vec::new();
        b.extend(7u32.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        b.extend("2001:db8::1".parse::<Ipv6Addr>().unwrap().octets());
        b.extend("fe80::5".parse::<Ipv6Addr>().unwrap().octets());
        b.extend(53u16.to_be_bytes());
        b.extend(61000u16.to_be_bytes());
        let e = decode(58, 1, &b).unwrap();
        assert_eq!((e.proto, e.op, e.pid), (Protocol::Udp6, Op::Send, 7));
        assert_eq!(e.local, "[fe80::5]:61000".parse().unwrap());
        assert_eq!(e.remote, "[2001:db8::1]:53".parse().unwrap());
        assert_eq!(decode(29, 1, &b).unwrap().op, Op::Disconnect);
    }

    #[test]
    fn filetime() {
        assert_eq!(filetime_ms(116_444_736_000_000_000), 0);
        assert_eq!(filetime_ms(116_444_736_000_000_000 + 12_345_678), 1234);
        assert_eq!(filetime_ms(0), 0);
    }

    fn ev(time: u64, op: Op, lport: u16, pid: u32, size: u32) -> EtwEvent {
        EtwEvent {
            time,
            op,
            proto: Protocol::Tcp,
            local: SocketAddr::from(([10, 0, 0, 1], lport)),
            remote: SocketAddr::from(([10, 0, 0, 2], 80)),
            pid,
            size,
        }
    }

    #[test]
    fn collector_aggregates_until_the_cutoff() {
        let mut c = Collector::default();
        c.push(ev(100, Op::Connect, 1, 0, 0));
        c.push(ev(110, Op::Send, 1, 50, 300));
        c.push(ev(120, Op::Recv, 1, 50, 1000));
        c.push(ev(105, Op::Accept, 2, 60, 0));
        c.push(ev(130, Op::Disconnect, 1, 50, 0));
        c.push(ev(200, Op::Send, 2, 60, 10));
        let seen = c.take(150);
        assert_eq!(seen.len(), 2);
        let a = &seen[0];
        assert_eq!(a.key.local.port(), 1);
        // The first non-zero PID.
        assert_eq!(a.key.pid, 50);
        assert_eq!((a.first, a.last), (100, 130));
        assert_eq!((a.opened, a.closed), (Some(100), Some(130)));
        assert_eq!(
            (a.bytes_out, a.bytes_in, a.sends, a.recvs),
            (300, 1000, 1, 1)
        );
        assert_eq!(seen[1].key.local.port(), 2);
        assert_eq!(seen[1].opened, Some(105));
        // The event after the cutoff waits for the next snapshot.
        assert_eq!(c.len(), 1);
        let next = c.take(1000);
        assert_eq!(next[0].bytes_out, 10);
        assert_eq!(next[0].opened, None);
        assert!(c.take(2000).is_empty());
    }
}
