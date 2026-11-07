//! Windows backend: the TCP and UDP tables from iphlpapi.dll with the owner module classes
//! (#14), decoded by [`super::winrows`], plus the owner module (module or service name) of each
//! endpoint from `GetOwnerModuleFrom*Entry`.
//!
//! The functions are declared directly (no binding crate): the API has been stable since
//! Windows Vista.

use super::winrows::{
    ROWS_AT, TCP4_ROW, TCP6_ROW, UDP4_ROW, UDP6_ROW, decode_tcp4, decode_tcp6, decode_udp4,
    decode_udp6,
};
use super::{ConnKey, Connection, NetError};
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::sync::Arc;

const AF_INET: u32 = 2;
const AF_INET6: u32 = 23;
/// `TCP_TABLE_OWNER_MODULE_ALL`: listeners and connections with PID, create time, module info.
const TCP_TABLE_OWNER_MODULE_ALL: i32 = 8;
/// `UDP_TABLE_OWNER_MODULE`.
const UDP_TABLE_OWNER_MODULE: i32 = 2;
/// `TCPIP_OWNER_MODULE_INFO_BASIC`: module name + path.
const OWNER_MODULE_INFO_BASIC: i32 = 0;
const NO_ERROR: u32 = 0;
const ERROR_INSUFFICIENT_BUFFER: u32 = 122;

type ModuleFn = unsafe extern "system" fn(*const c_void, i32, *mut c_void, *mut u32) -> u32;

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
    fn GetOwnerModuleFromTcpEntry(
        row: *const c_void,
        class: i32,
        buf: *mut c_void,
        size: *mut u32,
    ) -> u32;
    fn GetOwnerModuleFromTcp6Entry(
        row: *const c_void,
        class: i32,
        buf: *mut c_void,
        size: *mut u32,
    ) -> u32;
    fn GetOwnerModuleFromUdpEntry(
        row: *const c_void,
        class: i32,
        buf: *mut c_void,
        size: *mut u32,
    ) -> u32;
    fn GetOwnerModuleFromUdp6Entry(
        row: *const c_void,
        class: i32,
        buf: *mut c_void,
        size: *mut u32,
    ) -> u32;
}

type TableFn = unsafe extern "system" fn(*mut c_void, *mut u32, i32, u32, i32, u32) -> u32;

/// One table: reader, address family, class, row size, decoder, owner module function.
type TableSpec = (
    TableFn,
    u32,
    i32,
    usize,
    fn(&[u8]) -> Vec<Connection>,
    ModuleFn,
);

/// Calls one of the table functions, growing the buffer while the table grows between calls.
fn read_table(f: TableFn, af: u32, class: i32) -> Result<Vec<u8>, NetError> {
    let mut size: u32 = 64 * 1024;
    for _ in 0..8 {
        // u64 elements keep the buffer 8-byte aligned (the rows contain 8-byte fields).
        let mut buf = vec![0u64; (size as usize).div_ceil(8)];
        let mut len = (buf.len() * 8) as u32;
        // SAFETY: `buf` is valid for `len` bytes; the API writes at most `len` bytes and sets
        // `len` to the needed size when the buffer is too small.
        let rc = unsafe { f(buf.as_mut_ptr().cast(), &mut len, 0, af, class, 0) };
        match rc {
            NO_ERROR => {
                let bytes: Vec<u8> = buf.iter().flat_map(|d| d.to_ne_bytes()).collect();
                return Ok(bytes);
            }
            ERROR_INSUFFICIENT_BUFFER => size = len.max(size) + 16 * 1024,
            e => return Err(NetError(format!("network table query failed (error {e})"))),
        }
    }
    Err(NetError("network table keeps growing, giving up".into()))
}

/// The module name of one row (`MIB_*ROW_OWNER_MODULE` bytes): the executable or, for a service
/// hosted in svchost, the service name. `None` when Windows does not know it or refuses.
fn owner_module(f: ModuleFn, row: &[u8]) -> Option<Arc<str>> {
    // The API reads the whole row including the 8-byte fields: copy it into an aligned buffer.
    let mut aligned = vec![0u64; row.len().div_ceil(8)];
    for (d, chunk) in aligned.iter_mut().zip(row.chunks(8)) {
        let mut q = [0u8; 8];
        q[..chunk.len()].copy_from_slice(chunk);
        *d = u64::from_ne_bytes(q);
    }
    let mut size: u32 = 1024;
    for _ in 0..3 {
        let mut buf = vec![0u64; (size as usize).div_ceil(8)];
        let mut len = (buf.len() * 8) as u32;
        // SAFETY: `aligned` holds a complete row; `buf` is valid and aligned for `len` bytes.
        let rc = unsafe {
            f(
                aligned.as_ptr().cast(),
                OWNER_MODULE_INFO_BASIC,
                buf.as_mut_ptr().cast(),
                &mut len,
            )
        };
        match rc {
            NO_ERROR => {
                // TCPIP_OWNER_MODULE_BASIC_INFO: pModuleName, pModulePath – pointers into `buf`.
                let name = buf[0] as usize as *const u16;
                let base = buf.as_ptr() as usize;
                let end = base + buf.len() * 8;
                if name.is_null() || (name as usize) < base || name as usize >= end {
                    return None;
                }
                let max = (end - name as usize) / 2;
                // SAFETY: `name` lies inside `buf`; at most `max` units are read.
                let units = unsafe { std::slice::from_raw_parts(name, max) };
                let n = units.iter().position(|&c| c == 0)?;
                let s = String::from_utf16_lossy(&units[..n]);
                return (!s.is_empty()).then(|| Arc::from(s));
            }
            ERROR_INSUFFICIENT_BUFFER if len > size => size = len,
            _ => return None,
        }
    }
    None
}

/// Owner modules already looked up, by endpoint and creation time (a new connection with the
/// same addresses has another creation time).
#[derive(Default)]
pub struct ModuleCache {
    map: HashMap<(ConnKey, Option<u64>), Option<Arc<str>>>,
}

impl ModuleCache {
    /// Fills `module` of the connections decoded from `buf` (rows of `row_size` bytes).
    fn fill(&mut self, conns: &mut [Connection], buf: &[u8], row_size: usize, f: ModuleFn) {
        for (i, c) in conns.iter_mut().enumerate() {
            let key = (c.key(), c.created);
            let module = self.map.entry(key).or_insert_with(|| {
                let at = ROWS_AT + i * row_size;
                buf.get(at..at + row_size)
                    .and_then(|row| owner_module(f, row))
            });
            c.module = module.clone();
        }
    }

    fn retain(&mut self, alive: &HashSet<(ConnKey, Option<u64>)>) {
        self.map.retain(|k, _| alive.contains(k));
    }
}

pub fn snapshot(modules: &mut ModuleCache) -> Result<Vec<Connection>, NetError> {
    let mut out = Vec::new();
    let tables: [TableSpec; 4] = [
        (
            GetExtendedTcpTable,
            AF_INET,
            TCP_TABLE_OWNER_MODULE_ALL,
            TCP4_ROW,
            decode_tcp4,
            GetOwnerModuleFromTcpEntry,
        ),
        (
            GetExtendedTcpTable,
            AF_INET6,
            TCP_TABLE_OWNER_MODULE_ALL,
            TCP6_ROW,
            decode_tcp6,
            GetOwnerModuleFromTcp6Entry,
        ),
        (
            GetExtendedUdpTable,
            AF_INET,
            UDP_TABLE_OWNER_MODULE,
            UDP4_ROW,
            decode_udp4,
            GetOwnerModuleFromUdpEntry,
        ),
        (
            GetExtendedUdpTable,
            AF_INET6,
            UDP_TABLE_OWNER_MODULE,
            UDP6_ROW,
            decode_udp6,
            GetOwnerModuleFromUdp6Entry,
        ),
    ];
    for (table, af, class, row, decode, module) in tables {
        let buf = read_table(table, af, class)?;
        let mut conns = decode(&buf);
        modules.fill(&mut conns, &buf, row, module);
        out.extend(conns);
    }
    let alive: HashSet<_> = out.iter().map(|c| (c.key(), c.created)).collect();
    modules.retain(&alive);
    Ok(out)
}
