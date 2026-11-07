//! Administrator mode (#15): is this process elevated, and starting it again elevated through
//! UAC (`ShellExecuteW` with the "runas" verb). One executable for both modes – no second exe
//! with a `requireAdministrator` manifest.

// The pure helpers are only called by the Windows code (and tested on every platform).
#![cfg_attr(not(windows), allow(dead_code))]

/// Marks the elevated child: it never tries to elevate again (no loop when elevation does not
/// give a full administrator token, e.g. with UAC switched off).
pub const ELEVATED_ARG: &str = "--elevated";

/// Why the elevated start did not happen.
#[derive(Clone, Debug, PartialEq)]
pub enum ElevateError {
    /// The user said no in the UAC dialog.
    Cancelled,
    Failed(String),
}

/// One argument quoted for a Windows command line (the rules of `CommandLineToArgvW`):
/// arguments with spaces, tabs or quotes are put in quotes; backslashes are doubled only before
/// a quote and at the end of a quoted argument.
fn quote_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '\n', '"']) {
        return arg.to_owned();
    }
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            c => {
                out.push_str(&"\\".repeat(backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.push_str(&"\\".repeat(backslashes * 2));
    out.push('"');
    out
}

/// The parameters of a command line (without the program).
pub fn join_args(args: &[String]) -> String {
    args.iter()
        .map(|a| quote_arg(a))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(windows)]
mod imp {
    use super::ElevateError;
    use std::ffi::c_void;

    type Handle = *mut c_void;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> Handle;
        fn CloseHandle(h: Handle) -> i32;
        fn GetLastError() -> u32;
    }

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn OpenProcessToken(process: Handle, access: u32, token: *mut Handle) -> i32;
        fn GetTokenInformation(
            token: Handle,
            class: u32,
            info: *mut c_void,
            len: u32,
            returned: *mut u32,
        ) -> i32;
    }

    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ShellExecuteW(
            hwnd: Handle,
            operation: *const u16,
            file: *const u16,
            parameters: *const u16,
            directory: *const u16,
            show: i32,
        ) -> Handle;
    }

    const TOKEN_QUERY: u32 = 0x0008;
    const TOKEN_ELEVATION: u32 = 20;
    const SW_SHOWNORMAL: i32 = 1;
    const ERROR_CANCELLED: u32 = 1223;

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    pub fn is_elevated() -> bool {
        let mut token: Handle = std::ptr::null_mut();
        // SAFETY: the pseudo handle of the own process needs no closing; the token handle is
        // closed below; `elevation` is a TOKEN_ELEVATION (one DWORD) of the size passed.
        unsafe {
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return false;
            }
            let mut elevation = 0u32;
            let mut len = 0u32;
            let ok = GetTokenInformation(
                token,
                TOKEN_ELEVATION,
                (&raw mut elevation).cast(),
                4,
                &mut len,
            ) != 0;
            CloseHandle(token);
            ok && elevation != 0
        }
    }

    pub fn relaunch(params: &str) -> Result<(), ElevateError> {
        let exe = std::env::current_exe().map_err(|e| ElevateError::Failed(e.to_string()))?;
        let (verb, file, params) = (
            wide("runas".as_ref()),
            wide(exe.as_os_str()),
            wide(params.as_ref()),
        );
        // SAFETY: all strings are NUL-terminated and outlive the call; no window handle and no
        // directory (the current one is used).
        let (result, error) = unsafe {
            let r = ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                file.as_ptr(),
                params.as_ptr(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            );
            (r as usize, GetLastError())
        };
        match (result, error) {
            // Values above 32 mean success.
            (r, _) if r > 32 => Ok(()),
            (_, ERROR_CANCELLED) => Err(ElevateError::Cancelled),
            (r, e) => Err(ElevateError::Failed(format!(
                "ShellExecute {r}: {}",
                std::io::Error::from_raw_os_error(e as i32)
            ))),
        }
    }
}

/// This process runs with an administrator token (always `false` outside Windows).
pub fn is_elevated() -> bool {
    #[cfg(windows)]
    return imp::is_elevated();
    #[cfg(not(windows))]
    false
}

/// Starts this program again elevated (UAC prompt) with the given arguments; the caller then
/// closes this one.
pub fn relaunch_elevated(args: &[String]) -> Result<(), ElevateError> {
    #[cfg(windows)]
    return imp::relaunch(&join_args(args));
    #[cfg(not(windows))]
    {
        let _ = args;
        Err(ElevateError::Failed(
            "not supported on this platform".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_line_quoting() {
        assert_eq!(quote_arg("--elevated"), "--elevated");
        assert_eq!(quote_arg(""), r#""""#);
        assert_eq!(
            quote_arg(r"C:\Users\Jo Doe\x.json"),
            r#""C:\Users\Jo Doe\x.json""#
        );
        // Backslashes before a quote and at the end of a quoted argument are doubled.
        assert_eq!(quote_arg(r#"a"b"#), r#""a\"b""#);
        assert_eq!(quote_arg(r#"a\"b"#), r#""a\\\"b""#);
        assert_eq!(quote_arg(r"C:\My Dir\"), r#""C:\My Dir\\""#);
        // Backslashes without quotes stay as they are.
        assert_eq!(quote_arg(r"C:\dir\file"), r"C:\dir\file");
        assert_eq!(
            join_args(&[
                "--restore".into(),
                r"C:\Temp Dir\r.json".into(),
                ELEVATED_ARG.into()
            ]),
            r#"--restore "C:\Temp Dir\r.json" --elevated"#
        );
        assert_eq!(join_args(&[]), "");
    }

    #[test]
    fn elevation_query_does_not_fail() {
        // Either answer is fine; the call must work (and be stable).
        assert_eq!(is_elevated(), is_elevated());
    }
}
