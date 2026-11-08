//! Actions on rows (#6): close a TCP connection, end a process.
//!
//! * Close connection: `SetTcpEntry` with `MIB_TCP_STATE_DELETE_TCB` – IPv4 only, Windows has no
//!   IPv6 equivalent; UDP endpoints and listeners cannot be closed this way.
//! * End process: `OpenProcess(PROCESS_TERMINATE)` + `TerminateProcess` (Linux: `kill`
//!   with SIGTERM).
//!
//! Both usually need administrator rights for other users' targets; the error of the system is
//! returned as [`ActionError`] for the status bar. What can be done with a row is decided by pure
//! functions ([`can_close`], [`can_kill`]) and the `MIB_TCPROW` passed to the API is built by the
//! pure [`tcp_row`] – both tested everywhere.

use crate::net::{Connection, Protocol, TcpState};
use std::net::SocketAddr;

/// Why a connection cannot be closed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseBlock {
    /// UDP has no connections.
    Udp,
    /// Windows can only delete IPv4 TCP entries.
    Ipv6,
    /// A listener (or a row without a remote side).
    Listener,
    /// The connection is already closed (TimeWait, Closed, DeleteTcb).
    AlreadyClosed,
    /// Not supported on this platform.
    Platform,
}

/// Can `SetTcpEntry` close this connection?
pub fn can_close(c: &Connection) -> Result<(), CloseBlock> {
    if !cfg!(windows) {
        return Err(CloseBlock::Platform);
    }
    match c.proto {
        Protocol::Udp | Protocol::Udp6 => return Err(CloseBlock::Udp),
        Protocol::Tcp6 => return Err(CloseBlock::Ipv6),
        Protocol::Tcp => {}
    }
    let Some(remote) = c.remote else {
        return Err(CloseBlock::Listener);
    };
    if remote.port() == 0 || c.state == Some(TcpState::Listen) {
        return Err(CloseBlock::Listener);
    }
    if matches!(
        c.state,
        Some(TcpState::TimeWait | TcpState::Closed | TcpState::DeleteTcb)
    ) {
        return Err(CloseBlock::AlreadyClosed);
    }
    Ok(())
}

/// Why a process cannot be ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KillBlock {
    /// System Idle Process / System / unknown owner.
    System,
    /// MM PortLens itself.
    Own,
}

pub fn can_kill(pid: u32) -> Result<(), KillBlock> {
    if pid == 0 || (cfg!(windows) && pid == 4) {
        Err(KillBlock::System)
    } else if pid == std::process::id() {
        Err(KillBlock::Own)
    } else {
        Ok(())
    }
}

/// An error of the system.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ActionError {
    /// Access denied – administrator rights needed.
    AccessDenied,
    /// Any other error with the text of the system.
    Other(String),
}

/// `MIB_TCP_STATE_DELETE_TCB`.
const DELETE_TCB: u32 = 12;

/// `MIB_TCPROW` for `SetTcpEntry` with the state DELETE_TCB (the layout is built by
/// `net::estats::tcp_row`). `None` for anything but IPv4.
pub fn tcp_row(local: SocketAddr, remote: SocketAddr) -> Option<[u8; 20]> {
    crate::net::estats::tcp_row(DELETE_TCB, local, remote)
}

/// Closes a TCP connection (resets it).
pub fn close_connection(c: &Connection) -> Result<(), ActionError> {
    can_close(c).map_err(|b| ActionError::Other(format!("{b:?}")))?;
    let row = c
        .remote
        .and_then(|r| tcp_row(c.local, r))
        .ok_or_else(|| ActionError::Other("IPv6".into()))?;
    platform::set_tcp_entry(&row)
}

/// Ends a process.
pub fn kill_process(pid: u32) -> Result<(), ActionError> {
    can_kill(pid).map_err(|b| ActionError::Other(format!("{b:?}")))?;
    platform::terminate(pid)
}

#[cfg(windows)]
mod platform {
    use super::ActionError;
    use std::ffi::c_void;

    const PROCESS_TERMINATE: u32 = 0x0001;
    const ERROR_ACCESS_DENIED: u32 = 5;
    /// What `SetTcpEntry` returns without administrator rights.
    const ERROR_MR_MID_NOT_FOUND: u32 = 317;
    const FORMAT_MESSAGE_FROM_SYSTEM: u32 = 0x1000;
    const FORMAT_MESSAGE_IGNORE_INSERTS: u32 = 0x200;

    #[link(name = "iphlpapi")]
    unsafe extern "system" {
        fn SetTcpEntry(row: *const u8) -> u32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn TerminateProcess(process: *mut c_void, exit_code: u32) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
        fn GetLastError() -> u32;
        fn FormatMessageW(
            flags: u32,
            source: *const c_void,
            id: u32,
            lang: u32,
            buf: *mut u16,
            size: u32,
            args: *mut c_void,
        ) -> u32;
    }

    pub fn set_tcp_entry(row: &[u8; 20]) -> Result<(), ActionError> {
        // `SetTcpEntry` reads the DWORDs of MIB_TCPROW; copy them into an aligned buffer.
        let mut aligned = [0u32; 5];
        for (i, d) in aligned.iter_mut().enumerate() {
            *d = u32::from_ne_bytes([row[i * 4], row[i * 4 + 1], row[i * 4 + 2], row[i * 4 + 3]]);
        }
        // SAFETY: `aligned` is a complete, 4-byte aligned MIB_TCPROW that lives for the call.
        let rc = unsafe { SetTcpEntry(aligned.as_ptr().cast()) };
        match rc {
            0 => Ok(()),
            ERROR_ACCESS_DENIED | ERROR_MR_MID_NOT_FOUND => Err(ActionError::AccessDenied),
            e => Err(ActionError::Other(error_text(e))),
        }
    }

    pub fn terminate(pid: u32) -> Result<(), ActionError> {
        // SAFETY: plain Win32 calls; the handle is checked and closed before returning.
        unsafe {
            let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if h.is_null() {
                return Err(last_error());
            }
            let ok = TerminateProcess(h, 1);
            let err = (ok == 0).then(last_error);
            CloseHandle(h);
            err.map_or(Ok(()), Err)
        }
    }

    fn last_error() -> ActionError {
        // SAFETY: reads the calling thread's last error code.
        match unsafe { GetLastError() } {
            ERROR_ACCESS_DENIED => ActionError::AccessDenied,
            e => ActionError::Other(error_text(e)),
        }
    }

    /// The system's text of an error code ("The parameter is incorrect. (87)").
    fn error_text(code: u32) -> String {
        let mut buf = [0u16; 512];
        // SAFETY: `buf` is valid for its length; no inserts are used.
        let n = unsafe {
            FormatMessageW(
                FORMAT_MESSAGE_FROM_SYSTEM | FORMAT_MESSAGE_IGNORE_INSERTS,
                std::ptr::null(),
                code,
                0,
                buf.as_mut_ptr(),
                buf.len() as u32,
                std::ptr::null_mut(),
            )
        };
        let text = String::from_utf16_lossy(&buf[..n as usize]);
        format!("{} ({code})", text.trim())
    }
}

#[cfg(not(windows))]
mod platform {
    use super::ActionError;

    const SIGTERM: i32 = 15;
    const EPERM: i32 = 1;

    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }

    pub fn set_tcp_entry(_row: &[u8; 20]) -> Result<(), ActionError> {
        Err(ActionError::Other("not supported on this system".into()))
    }

    pub fn terminate(pid: u32) -> Result<(), ActionError> {
        let pid = i32::try_from(pid).map_err(|e| ActionError::Other(e.to_string()))?;
        // SAFETY: sends a signal; no memory is involved.
        if unsafe { kill(pid, SIGTERM) } == 0 {
            return Ok(());
        }
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(EPERM) {
            Err(ActionError::AccessDenied)
        } else {
            Err(ActionError::Other(err.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(
        proto: Protocol,
        local: &str,
        remote: Option<&str>,
        state: Option<TcpState>,
    ) -> Connection {
        Connection {
            proto,
            local: local.parse().unwrap(),
            remote: remote.map(|r| r.parse().unwrap()),
            state,
            pid: 1234,
            created: None,
            module: None,
            stats: None,
        }
    }

    #[test]
    fn what_can_be_closed() {
        let est = conn(
            Protocol::Tcp,
            "10.0.0.1:50000",
            Some("1.2.3.4:443"),
            Some(TcpState::Established),
        );
        if cfg!(windows) {
            assert_eq!(can_close(&est), Ok(()));
            let mut cw = est.clone();
            cw.state = Some(TcpState::CloseWait);
            assert_eq!(can_close(&cw), Ok(()));
            // Windows sometimes reports state 0 for live connections (#21): still closable.
            cw.state = Some(TcpState::Unknown(0));
            assert_eq!(can_close(&cw), Ok(()));
            let mut tw = est.clone();
            tw.state = Some(TcpState::TimeWait);
            assert_eq!(can_close(&tw), Err(CloseBlock::AlreadyClosed));
            let v6 = conn(
                Protocol::Tcp6,
                "[::1]:5000",
                Some("[::1]:6000"),
                Some(TcpState::Established),
            );
            assert_eq!(can_close(&v6), Err(CloseBlock::Ipv6));
            let udp = conn(Protocol::Udp, "0.0.0.0:53", None, None);
            assert_eq!(can_close(&udp), Err(CloseBlock::Udp));
            let lis = conn(Protocol::Tcp, "0.0.0.0:445", None, Some(TcpState::Listen));
            assert_eq!(can_close(&lis), Err(CloseBlock::Listener));
            // A listener with an invalid state has no remote side since #24; a row with remote
            // port 0 (from another backend) is refused as well.
            let lis0 = conn(
                Protocol::Tcp,
                "0.0.0.0:445",
                None,
                Some(TcpState::Unknown(0)),
            );
            assert_eq!(can_close(&lis0), Err(CloseBlock::Listener));
            let port0 = conn(
                Protocol::Tcp,
                "0.0.0.0:445",
                Some("0.0.0.0:0"),
                Some(TcpState::Unknown(0)),
            );
            assert_eq!(can_close(&port0), Err(CloseBlock::Listener));
        } else {
            assert_eq!(can_close(&est), Err(CloseBlock::Platform));
        }
    }

    #[test]
    fn what_can_be_killed() {
        assert_eq!(can_kill(0), Err(KillBlock::System));
        assert_eq!(can_kill(std::process::id()), Err(KillBlock::Own));
        assert_eq!(can_kill(123_456), Ok(()));
    }

    #[test]
    fn tcp_row_layout() {
        let row = tcp_row(
            "192.168.1.10:51234".parse().unwrap(),
            "140.82.112.3:443".parse().unwrap(),
        )
        .unwrap();
        assert_eq!(&row[0..4], &12u32.to_le_bytes());
        assert_eq!(&row[4..8], &[192, 168, 1, 10]);
        assert_eq!(&row[8..12], &[0xC8, 0x22, 0, 0]); // 51234 = 0xC822
        assert_eq!(&row[12..16], &[140, 82, 112, 3]);
        assert_eq!(&row[16..20], &[0x01, 0xBB, 0, 0]); // 443 = 0x01BB
        assert!(tcp_row("[::1]:1".parse().unwrap(), "[::1]:2".parse().unwrap()).is_none());
    }

    /// Ends a child process of the test (no administrator rights needed for an own child).
    #[test]
    fn kill_own_child() {
        let mut child = if cfg!(windows) {
            std::process::Command::new("ping")
                .args(["-n", "30", "127.0.0.1"])
                .stdout(std::process::Stdio::null())
                .spawn()
                .unwrap()
        } else {
            std::process::Command::new("sleep")
                .arg("30")
                .spawn()
                .unwrap()
        };
        kill_process(child.id()).unwrap();
        let status = child.wait().unwrap();
        assert!(!status.success());
    }

    /// With administrator rights: an own loopback connection disappears after it is closed.
    /// Without them the call must fail with AccessDenied (nothing else).
    #[cfg(windows)]
    #[test]
    fn close_own_connection() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let client = std::net::TcpStream::connect(addr).unwrap();
        let (_server, _) = listener.accept().unwrap();
        let c = conn(
            Protocol::Tcp,
            &client.local_addr().unwrap().to_string(),
            Some(&addr.to_string()),
            Some(TcpState::Established),
        );
        match close_connection(&c) {
            Ok(()) => {
                let still = crate::net::snapshot().unwrap().into_iter().any(|s| {
                    s.local == c.local
                        && s.remote == c.remote
                        && !matches!(s.state, Some(TcpState::TimeWait | TcpState::Closed))
                });
                assert!(!still, "the closed connection is still reported");
            }
            Err(ActionError::AccessDenied) => {
                eprintln!("close_own_connection: not elevated, only the access error checked");
            }
            Err(e) => panic!("{e:?}"),
        }
    }
}
