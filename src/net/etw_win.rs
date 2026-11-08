//! The real-time ETW session of [`super::etw`] (#4): `StartTraceW` creates a session named
//! `MMPortLens ETW <pid>`, `EnableTraceEx2` adds the kernel network provider, `OpenTraceW` +
//! `ProcessTrace` deliver its events to [`callback`] in a thread of its own, which decodes them
//! and sends them to the poll thread.
//!
//! A trace session outlives its process: when MM PortLens is killed, the session stays in the
//! system (there are at most 64). [`Session::start`] therefore first stops the sessions of
//! MM PortLens processes that no longer run, and [`Session::stop`] (also on `Drop`) stops this
//! one.
//!
//! The functions are declared directly (no binding crate); the structure layouts are checked by
//! the size assertions below.

use super::etw::{EtwError, EtwEvent, KEYWORDS, PROVIDER, decode, filetime_ms};
use std::ffi::c_void;
use std::sync::mpsc::Sender;
use std::thread::JoinHandle;

/// Start of the session names.
const NAME_PREFIX: &str = "MMPortLens ETW ";

const ERROR_SUCCESS: u32 = 0;
const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_ALREADY_EXISTS: u32 = 183;
const WNODE_FLAG_TRACED_GUID: u32 = 0x0002_0000;
const EVENT_TRACE_REAL_TIME_MODE: u32 = 0x0000_0100;
const EVENT_TRACE_CONTROL_STOP: u32 = 1;
const EVENT_CONTROL_CODE_ENABLE_PROVIDER: u32 = 1;
const TRACE_LEVEL_VERBOSE: u8 = 5;
const PROCESS_TRACE_MODE_REAL_TIME: u32 = 0x0000_0100;
const PROCESS_TRACE_MODE_EVENT_RECORD: u32 = 0x1000_0000;
const INVALID_PROCESSTRACE_HANDLE: u64 = u64::MAX;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
/// Room for the logger name (and an empty log file name) behind the properties, in UTF-16 units.
const NAME_CHARS: usize = 1024;
/// `QueryAllTracesW` returns at most 64 sessions.
const MAX_SESSIONS: usize = 64;

#[repr(C)]
#[allow(dead_code)] // the fields give the layout
#[derive(Clone, Copy)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

/// `WNODE_HEADER`.
#[repr(C)]
#[allow(dead_code)] // the fields give the layout
struct WnodeHeader {
    buffer_size: u32,
    provider_id: u32,
    historical_context: u64,
    time_stamp: i64,
    guid: Guid,
    client_context: u32,
    flags: u32,
}

/// `EVENT_TRACE_PROPERTIES`; the logger name follows it in the same buffer.
#[repr(C)]
#[allow(dead_code)] // the fields give the layout
struct TraceProperties {
    wnode: WnodeHeader,
    buffer_size: u32,
    minimum_buffers: u32,
    maximum_buffers: u32,
    maximum_file_size: u32,
    log_file_mode: u32,
    flush_timer: u32,
    enable_flags: u32,
    age_limit: i32,
    number_of_buffers: u32,
    free_buffers: u32,
    events_lost: u32,
    buffers_written: u32,
    log_buffers_lost: u32,
    real_time_buffers_lost: u32,
    logger_thread_id: *mut c_void,
    log_file_name_offset: u32,
    logger_name_offset: u32,
}

/// `EVENT_DESCRIPTOR`.
#[repr(C)]
#[allow(dead_code)] // the fields give the layout
struct EventDescriptor {
    id: u16,
    version: u8,
    channel: u8,
    level: u8,
    opcode: u8,
    task: u16,
    keyword: u64,
}

/// `EVENT_HEADER`.
#[repr(C)]
#[allow(dead_code)] // the fields give the layout
struct EventHeader {
    size: u16,
    header_type: u16,
    flags: u16,
    event_property: u16,
    thread_id: u32,
    process_id: u32,
    time_stamp: i64,
    provider_id: Guid,
    descriptor: EventDescriptor,
    processor_time: u64,
    activity_id: Guid,
}

/// `EVENT_RECORD`.
#[repr(C)]
#[allow(dead_code)] // the fields give the layout
struct EventRecord {
    header: EventHeader,
    buffer_context: u32,
    extended_data_count: u16,
    user_data_length: u16,
    extended_data: *mut c_void,
    user_data: *const u8,
    user_context: *mut c_void,
}

type RecordCallback = unsafe extern "system" fn(*mut EventRecord);

/// `EVENT_TRACE_LOGFILEW`; `EVENT_TRACE` and `TRACE_LOGFILE_HEADER` (filled by the system, not
/// read) are opaque.
#[repr(C)]
#[allow(dead_code)] // the fields give the layout
struct TraceLogfile {
    log_file_name: *mut u16,
    logger_name: *mut u16,
    current_time: i64,
    buffers_read: u32,
    process_trace_mode: u32,
    current_event: [u64; 11],
    logfile_header: [u64; 35],
    buffer_callback: *mut c_void,
    buffer_size: u32,
    filled: u32,
    events_lost: u32,
    event_record_callback: Option<RecordCallback>,
    is_kernel_trace: u32,
    context: *mut c_void,
}

#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(size_of::<WnodeHeader>() == 48);
    assert!(size_of::<TraceProperties>() == 120);
    assert!(size_of::<EventHeader>() == 80);
    assert!(size_of::<EventRecord>() == 112);
    assert!(size_of::<TraceLogfile>() == 448);
};

#[link(name = "advapi32")]
unsafe extern "system" {
    fn StartTraceW(handle: *mut u64, name: *const u16, props: *mut TraceProperties) -> u32;
    fn ControlTraceW(handle: u64, name: *const u16, props: *mut TraceProperties, code: u32) -> u32;
    fn EnableTraceEx2(
        handle: u64,
        provider: *const Guid,
        code: u32,
        level: u8,
        any: u64,
        all: u64,
        timeout: u32,
        params: *const c_void,
    ) -> u32;
    fn OpenTraceW(logfile: *mut TraceLogfile) -> u64;
    fn ProcessTrace(
        handles: *const u64,
        count: u32,
        start: *const c_void,
        end: *const c_void,
    ) -> u32;
    fn CloseTrace(handle: u64) -> u32;
    fn QueryAllTracesW(props: *mut *mut TraceProperties, count: u32, loggers: *mut u32) -> u32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
    fn CloseHandle(h: *mut c_void) -> i32;
}

fn provider_guid() -> Guid {
    let (data1, data2, data3, data4) = PROVIDER;
    Guid {
        data1,
        data2,
        data3,
        data4,
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// A zeroed properties buffer with room for the names (`Vec<u64>` keeps it aligned).
fn properties() -> Vec<u64> {
    let bytes = size_of::<TraceProperties>() + 2 * NAME_CHARS * 2;
    let mut buf = vec![0u64; bytes.div_ceil(8)];
    let p = buf.as_mut_ptr() as *mut TraceProperties;
    // SAFETY: the buffer is zeroed, aligned and larger than `TraceProperties`.
    unsafe {
        (*p).wnode.buffer_size = (buf.len() * 8) as u32;
        (*p).logger_name_offset = size_of::<TraceProperties>() as u32;
        (*p).log_file_name_offset = (size_of::<TraceProperties>() + NAME_CHARS * 2) as u32;
    }
    buf
}

/// Stops the session `name` (an orphan of a killed MM PortLens, or this process's own).
fn stop_by_name(name: &str) -> u32 {
    let mut props = properties();
    let name = wide(name);
    // SAFETY: the properties buffer has the size it declares; the name is NUL-terminated.
    unsafe {
        ControlTraceW(
            0,
            name.as_ptr(),
            props.as_mut_ptr() as *mut TraceProperties,
            EVENT_TRACE_CONTROL_STOP,
        )
    }
}

fn process_alive(pid: u32) -> bool {
    // SAFETY: plain query access; the handle is closed at once.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return false;
        }
        CloseHandle(h);
    }
    true
}

/// The PID in a session name of MM PortLens.
fn session_pid(name: &str) -> Option<u32> {
    name.strip_prefix(NAME_PREFIX)?.parse().ok()
}

/// Stops the sessions of MM PortLens processes that no longer run.
fn stop_orphans() {
    let mut bufs: Vec<Vec<u64>> = (0..MAX_SESSIONS).map(|_| properties()).collect();
    let mut ptrs: Vec<*mut TraceProperties> = bufs
        .iter_mut()
        .map(|b| b.as_mut_ptr() as *mut TraceProperties)
        .collect();
    let mut count = 0u32;
    // SAFETY: `MAX_SESSIONS` buffers, each with the size its header declares.
    let rc = unsafe { QueryAllTracesW(ptrs.as_mut_ptr(), MAX_SESSIONS as u32, &mut count) };
    if rc != ERROR_SUCCESS {
        return;
    }
    for buf in bufs.iter().take(count as usize) {
        let p = buf.as_ptr() as *const TraceProperties;
        // SAFETY: the system wrote a NUL-terminated name at `logger_name_offset`, inside the
        // buffer.
        let name = unsafe {
            let off = (*p).logger_name_offset as usize;
            if off == 0 || off >= buf.len() * 8 {
                continue;
            }
            let start = (buf.as_ptr() as *const u8).add(off) as *const u16;
            let max = (buf.len() * 8 - off) / 2;
            let len = (0..max).take_while(|&i| *start.add(i) != 0).count();
            String::from_utf16_lossy(std::slice::from_raw_parts(start, len))
        };
        if let Some(pid) = session_pid(&name)
            && pid != std::process::id()
            && !process_alive(pid)
        {
            stop_by_name(&name);
        }
    }
}

/// Called by `ProcessTrace` for every event, in the session thread.
unsafe extern "system" fn callback(rec: *mut EventRecord) {
    // SAFETY: ETW passes a valid record for the duration of the call; `user_context` is the
    // `Sender` boxed by `Session::start`, alive until `ProcessTrace` returned.
    unsafe {
        let r = &*rec;
        let tx = &*(r.user_context as *const Sender<EtwEvent>);
        if r.user_data.is_null() || r.header.provider_id.data1 != PROVIDER.0 {
            return;
        }
        let data = std::slice::from_raw_parts(r.user_data, r.user_data_length as usize);
        // Without PROCESS_TRACE_MODE_RAW_TIMESTAMP the time stamp is a FILETIME.
        let time = filetime_ms(r.header.time_stamp);
        if let Some(e) = decode(r.header.descriptor.id, time, data) {
            let _ = tx.send(e);
        }
    }
}

/// A running session; stopped by [`Session::stop`] or when dropped.
pub struct Session {
    name: String,
    handle: u64,
    trace: u64,
    props: Vec<u64>,
    thread: Option<JoinHandle<()>>,
    /// The `Sender` the callback uses (freed after the session thread ended).
    context: *mut Sender<EtwEvent>,
}

// SAFETY: the handles are plain numbers usable from any thread; `context` is only freed after
// the thread that uses it ended.
unsafe impl Send for Session {}

/// The raw handle moved into the session thread.
struct TraceHandle(u64);

impl Session {
    pub fn start(tx: Sender<EtwEvent>) -> Result<Self, EtwError> {
        stop_orphans();
        let name = format!("{NAME_PREFIX}{}", std::process::id());
        let wname = wide(&name);
        let mut props = properties();
        let mut handle = 0u64;
        let start = |props: &mut Vec<u64>, handle: &mut u64| {
            let p = props.as_mut_ptr() as *mut TraceProperties;
            // SAFETY: the buffer was made by `properties()`; StartTraceW copies the name.
            unsafe {
                (*p).wnode.flags = WNODE_FLAG_TRACED_GUID;
                // 1 = QueryPerformanceCounter; converted to FILETIME for the consumer.
                (*p).wnode.client_context = 1;
                (*p).log_file_mode = EVENT_TRACE_REAL_TIME_MODE;
                (*p).buffer_size = 64; // KB
                (*p).minimum_buffers = 4;
                (*p).maximum_buffers = 64;
                (*p).flush_timer = 1; // s
                StartTraceW(handle, wname.as_ptr(), p)
            }
        };
        let mut rc = start(&mut props, &mut handle);
        if rc == ERROR_ALREADY_EXISTS {
            // Left from an earlier run with the same PID.
            stop_by_name(&name);
            props = properties();
            rc = start(&mut props, &mut handle);
        }
        match rc {
            ERROR_SUCCESS => {}
            ERROR_ACCESS_DENIED => return Err(EtwError::Denied),
            e => return Err(EtwError::Failed(format!("StartTrace: error {e}"))),
        }
        let guid = provider_guid();
        // SAFETY: valid session handle and GUID; no enable parameters.
        let rc = unsafe {
            EnableTraceEx2(
                handle,
                &guid,
                EVENT_CONTROL_CODE_ENABLE_PROVIDER,
                TRACE_LEVEL_VERBOSE,
                KEYWORDS,
                0,
                0,
                std::ptr::null(),
            )
        };
        let context = Box::into_raw(Box::new(tx));
        let mut session = Session {
            name,
            handle,
            trace: INVALID_PROCESSTRACE_HANDLE,
            props,
            thread: None,
            context,
        };
        match rc {
            ERROR_SUCCESS => {}
            // A standard user may create a session but not enable a kernel provider.
            ERROR_ACCESS_DENIED => return Err(EtwError::Denied),
            e => return Err(EtwError::Failed(format!("EnableTraceEx2: error {e}"))),
        }
        let mut wname = wide(&session.name);
        // SAFETY: an all-zero EVENT_TRACE_LOGFILEW is valid (null pointers, no callbacks).
        let mut logfile: TraceLogfile = unsafe { std::mem::zeroed() };
        logfile.logger_name = wname.as_mut_ptr();
        logfile.process_trace_mode = PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD;
        logfile.event_record_callback = Some(callback);
        logfile.context = context as *mut c_void;
        // SAFETY: the logfile structure is complete; OpenTraceW copies what it keeps.
        let trace = unsafe { OpenTraceW(&mut logfile) };
        if trace == INVALID_PROCESSTRACE_HANDLE {
            let e = std::io::Error::last_os_error();
            return Err(EtwError::Failed(format!("OpenTrace: {e}")));
        }
        session.trace = trace;
        let h = TraceHandle(trace);
        let thread = std::thread::Builder::new()
            .name("etw".into())
            .spawn(move || {
                let h = h;
                // SAFETY: a handle from OpenTraceW; blocks until the session is stopped.
                unsafe {
                    ProcessTrace(&h.0, 1, std::ptr::null(), std::ptr::null());
                }
            })
            .map_err(|e| EtwError::Failed(e.to_string()))?;
        session.thread = Some(thread);
        Ok(session)
    }

    /// Stops the session and waits for its thread.
    pub fn stop(self) {
        drop(self);
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: the handles belong to this session; stopping makes ProcessTrace return,
        // CloseTrace ends it should it still wait. The context is freed only after the thread
        // (the only user of it) ended.
        unsafe {
            ControlTraceW(
                self.handle,
                std::ptr::null(),
                self.props.as_mut_ptr() as *mut TraceProperties,
                EVENT_TRACE_CONTROL_STOP,
            );
            if self.trace != INVALID_PROCESSTRACE_HANDLE {
                CloseTrace(self.trace);
            }
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
            drop(Box::from_raw(self.context));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_names() {
        assert_eq!(session_pid("MMPortLens ETW 1234"), Some(1234));
        assert_eq!(session_pid("MMPortLens ETW x"), None);
        assert_eq!(session_pid("NT Kernel Logger"), None);
    }

    #[test]
    fn not_elevated_is_denied_or_runs() {
        // Without administrator rights StartTrace is refused; elevated it runs and stops.
        let (tx, _rx) = std::sync::mpsc::channel();
        match Session::start(tx) {
            Ok(s) => s.stop(),
            Err(e) => assert_eq!(e, EtwError::Denied, "{e}"),
        }
    }
}
