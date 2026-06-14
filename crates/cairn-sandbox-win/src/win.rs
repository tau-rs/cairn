//! Windows AppContainer launcher implementation. All unsafe FFI is here.

use std::ffi::OsString;
use std::os::windows::ffi::OsStrExt;
use std::process::ExitCode;

use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, S_OK};
use windows_sys::Win32::Security::Isolation::{
    CreateAppContainerProfile, DeriveAppContainerSidFromAppContainerName,
};
use windows_sys::Win32::Security::PSID;

/// Fixed AppContainer profile name → stable package SID. Stable so the read
/// grant on a plugin dir is idempotent across runs.
const PROFILE_NAME: &str = "Cairn.PluginSandbox";

/// UTF-16, NUL-terminated copy of `s`, for the `PCWSTR` Win32 string params.
fn wide(s: &str) -> Vec<u16> {
    OsString::from(s)
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Wide, NUL-terminated copy of an `OsStr` path.
// TEMPORARY allow — `wide_path` is first used by `confine_and_run` in Task 5,
// which removes this attribute.
#[allow(dead_code)]
fn wide_path(p: &std::ffi::OsStr) -> Vec<u16> {
    p.encode_wide().chain(std::iter::once(0)).collect()
}

/// Create-or-confirm the AppContainer profile and return its package SID.
/// `CreateAppContainerProfile` returns `HRESULT_FROM_WIN32(ERROR_ALREADY_EXISTS)`
/// if the profile exists; in that case we derive the SID instead.
fn ensure_app_container_sid() -> Result<PSID, String> {
    let name = wide(PROFILE_NAME);
    let display = wide("Cairn Plugin Sandbox");
    let desc = wide("Confines trusted cairn plugins");
    let mut sid: PSID = std::ptr::null_mut();
    // SAFETY: all pointers reference live local buffers for the duration of the
    // call; `sid` is an out-param populated on success.
    let hr = unsafe {
        CreateAppContainerProfile(
            name.as_ptr(),
            display.as_ptr(),
            desc.as_ptr(),
            std::ptr::null(),
            0,
            &mut sid,
        )
    };
    if hr == S_OK {
        return Ok(sid);
    }
    // HRESULT_FROM_WIN32(ERROR_ALREADY_EXISTS) == 0x800700B7
    let already = (0x8007_0000u32 | (ERROR_ALREADY_EXISTS & 0xFFFF)) as i32;
    if hr == already {
        // SAFETY: out-param `sid` populated on success; `name` is live.
        let dhr = unsafe { DeriveAppContainerSidFromAppContainerName(name.as_ptr(), &mut sid) };
        if dhr == S_OK {
            return Ok(sid);
        }
        return Err(format!(
            "DeriveAppContainerSidFromAppContainerName failed: 0x{dhr:08X}"
        ));
    }
    Err(format!("CreateAppContainerProfile failed: 0x{hr:08X}"))
}

/// Entry point dispatched from `main`.
pub fn run() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if args.first().map(|a| a == "--probe").unwrap_or(false) {
        return match ensure_app_container_sid() {
            Ok(_) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("cairn-sandbox-win: {e}");
                ExitCode::FAILURE
            }
        };
    }
    match confine_and_run(&args) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("cairn-sandbox-win: {e}");
            ExitCode::FAILURE
        }
    }
}

// TEMPORARY stub — replaced by the real implementation in Task 5.
#[allow(dead_code)]
fn confine_and_run(_args: &[OsString]) -> Result<u8, String> {
    Err("confine_and_run not yet implemented".into())
}
