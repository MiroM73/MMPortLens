//! The console companion of `mmportlens.exe` (#44), shipped as `mmportlens.com`.
//!
//! `mmportlens.exe` is a GUI program, so `cmd` and PowerShell neither wait for it nor pass it
//! their redirections (`> file`, `$x = …`), and the exit code is lost. A shell that runs
//! `mmportlens` prefers `mmportlens.com` (PATHEXT lists `.COM` before `.EXE`): this small console
//! program starts `mmportlens.exe` from its own folder with the same arguments and its own
//! standard handles, and waits –
//! - until the program ends, then it ends with the same exit code (`--export`, `--record`,
//!   invalid command lines, `--version`), or
//! - until the program signals that it opened its window (the event named in
//!   `MMPORTLENS_CONSOLE_EVENT`), then it ends with 0 and the window stays (a normal start, and
//!   `--help` after its text was printed).
//!
//! Ctrl+C reaches both programs: the recording of `mmportlens.exe` stops and saves, and this
//! program ignores it, so it still reports the exit code. The Start menu, Explorer and file
//! associations keep using `mmportlens.exe`, without a console window.

use std::process::{Command, ExitCode};

/// Exit code when `mmportlens.exe` cannot be started (same as an error of the program).
const START_FAILED: u8 = 1;

fn main() -> ExitCode {
    let exe = match std::env::current_exe() {
        Ok(p) => p.with_file_name(if cfg!(windows) {
            "mmportlens.exe"
        } else {
            "mmportlens"
        }),
        Err(e) => {
            eprintln!("MM PortLens: {e}");
            return ExitCode::from(START_FAILED);
        }
    };
    let mut cmd = Command::new(&exe);
    cmd.args(std::env::args_os().skip(1));
    match run(cmd) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("MM PortLens: {}: {e}", exe.display());
            ExitCode::from(START_FAILED)
        }
    }
}

#[cfg(windows)]
fn run(mut cmd: Command) -> std::io::Result<u8> {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateEventW(
            attrs: *const c_void,
            manual: i32,
            initial: i32,
            name: *const u16,
        ) -> *mut c_void;
        fn WaitForMultipleObjects(
            count: u32,
            handles: *const *mut c_void,
            all: i32,
            ms: u32,
        ) -> u32;
        fn CloseHandle(handle: *mut c_void) -> i32;
        fn SetConsoleCtrlHandler(
            handler: Option<unsafe extern "system" fn(u32) -> i32>,
            add: i32,
        ) -> i32;
    }
    const INFINITE: u32 = u32::MAX;
    const WAIT_OBJECT_0: u32 = 0;

    /// Ctrl+C / Ctrl+Break are for `mmportlens.exe` (it stops a recording); this program keeps
    /// waiting for its exit code. A handler routine is not inherited by the child.
    unsafe extern "system" fn ignore(kind: u32) -> i32 {
        const CTRL_CLOSE_EVENT: u32 = 2;
        i32::from(kind < CTRL_CLOSE_EVENT)
    }
    // SAFETY: registers a static function; nothing else is passed.
    unsafe {
        SetConsoleCtrlHandler(Some(ignore), 1);
    }

    let name = format!("Local\\MMPortLens-console-{}", std::process::id());
    let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
    // SAFETY: `wide` is NUL-terminated and lives until the call returns; no security attributes.
    let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, wide.as_ptr()) };
    if !event.is_null() {
        cmd.env("MMPORTLENS_CONSOLE_EVENT", &name);
    }
    // The standard handles are inherited: the console, a file of `>` or a pipe of PowerShell.
    let mut child = cmd.spawn()?;
    let window_opened = if event.is_null() {
        false
    } else {
        let handles = [child.as_raw_handle(), event];
        // SAFETY: both handles are valid for the duration of the call (the child is not waited
        // on or dropped before it returns).
        let which = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) };
        // SAFETY: the event handle came from CreateEventW and is not used afterwards.
        unsafe { CloseHandle(event) };
        which == WAIT_OBJECT_0 + 1
    };
    if window_opened {
        return Ok(0);
    }
    let status = child.wait()?;
    Ok(status.code().map_or(START_FAILED, |c| c as u8))
}

#[cfg(not(windows))]
fn run(mut cmd: Command) -> std::io::Result<u8> {
    let status = cmd.status()?;
    Ok(status.code().map_or(START_FAILED, |c| c as u8))
}
