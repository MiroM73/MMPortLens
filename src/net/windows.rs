//! Windows backend: the TCP and UDP tables from iphlpapi.dll, decoded by [`super::winrows`].
//!
//! The four functions are declared directly (no binding crate): the API has been stable since
//! Windows XP SP2 and only these two entry points are needed.

use super::winrows::{decode_tcp4, decode_tcp6, decode_udp4, decode_udp6};
use super::{Connection, NetError};
use std::ffi::c_void;

const AF_INET: u32 = 2;
const AF_INET6: u32 = 23;
/// `TCP_TABLE_OWNER_PID_ALL`: listeners and connections with the owning PID.
const TCP_TABLE_OWNER_PID_ALL: i32 = 5;
/// `UDP_TABLE_OWNER_PID`.
const UDP_TABLE_OWNER_PID: i32 = 1;
const NO_ERROR: u32 = 0;
const ERROR_INSUFFICIENT_BUFFER: u32 = 122;

#[link(name = "iphlpapi")]
unsafe extern "system" {
    fn GetExtendedTcpTable(
        table: *mut c_void,
        size: *mut u32,
        order: i32,
        af: u32,
        class: i32,
        reserved: u32,
    ) -> u32;
    fn GetExtendedUdpTable(
        table: *mut c_void,
        size: *mut u32,
        order: i32,
        af: u32,
        class: i32,
        reserved: u32,
    ) -> u32;
}

type TableFn = unsafe extern "system" fn(*mut c_void, *mut u32, i32, u32, i32, u32) -> u32;

/// Calls one of the table functions, growing the buffer while the table grows between calls.
fn read_table(f: TableFn, af: u32, class: i32) -> Result<Vec<u8>, NetError> {
    let mut size: u32 = 16 * 1024;
    for _ in 0..8 {
        // u32 elements keep the buffer 4-byte aligned, as the API expects.
        let mut buf = vec![0u32; (size as usize).div_ceil(4)];
        let mut len = (buf.len() * 4) as u32;
        // SAFETY: `buf` is valid for `len` bytes; the API writes at most `len` bytes and sets
        // `len` to the needed size when the buffer is too small.
        let rc = unsafe { f(buf.as_mut_ptr().cast(), &mut len, 0, af, class, 0) };
        match rc {
            NO_ERROR => {
                let bytes: Vec<u8> = buf.iter().flat_map(|d| d.to_ne_bytes()).collect();
                return Ok(bytes);
            }
            ERROR_INSUFFICIENT_BUFFER => size = len.max(size) + 4096,
            e => return Err(NetError(format!("network table query failed (error {e})"))),
        }
    }
    Err(NetError("network table keeps growing, giving up".into()))
}

pub fn snapshot() -> Result<Vec<Connection>, NetError> {
    let mut out = decode_tcp4(&read_table(
        GetExtendedTcpTable,
        AF_INET,
        TCP_TABLE_OWNER_PID_ALL,
    )?);
    out.extend(decode_tcp6(&read_table(
        GetExtendedTcpTable,
        AF_INET6,
        TCP_TABLE_OWNER_PID_ALL,
    )?));
    out.extend(decode_udp4(&read_table(
        GetExtendedUdpTable,
        AF_INET,
        UDP_TABLE_OWNER_PID,
    )?));
    out.extend(decode_udp6(&read_table(
        GetExtendedUdpTable,
        AF_INET6,
        UDP_TABLE_OWNER_PID,
    )?));
    Ok(out)
}
