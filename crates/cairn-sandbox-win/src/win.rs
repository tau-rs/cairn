//! Windows AppContainer launcher implementation. All unsafe FFI is here.

use std::ffi::OsString;
use std::os::windows::ffi::OsStrExt;
use std::process::ExitCode;

use windows_sys::Win32::Foundation::{
    CloseHandle, LocalFree, ERROR_ALREADY_EXISTS, GENERIC_EXECUTE, GENERIC_READ, HANDLE, S_OK,
    WAIT_OBJECT_0,
};
use windows_sys::Win32::Security::Authorization::{
    GetNamedSecurityInfoW, SetEntriesInAclW, SetNamedSecurityInfoW, EXPLICIT_ACCESS_W,
    GRANT_ACCESS, SE_FILE_OBJECT, TRUSTEE_IS_GROUP, TRUSTEE_IS_SID, TRUSTEE_W,
};
use windows_sys::Win32::Security::Isolation::{
    CreateAppContainerProfile, DeriveAppContainerSidFromAppContainerName,
};
use windows_sys::Win32::Security::{
    ACL, CONTAINER_INHERIT_ACE, DACL_SECURITY_INFORMATION, OBJECT_INHERIT_ACE,
    PSECURITY_DESCRIPTOR, PSID, SECURITY_CAPABILITIES,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
    InitializeProcThreadAttributeList, ResumeThread, TerminateProcess, UpdateProcThreadAttribute,
    WaitForSingleObject, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
    EXTENDED_STARTUPINFO_PRESENT, INFINITE, PROCESS_INFORMATION,
    PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, STARTUPINFOEXW,
};

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
        // The SID is intentionally not freed: the launcher process is
        // short-lived and exits right after the child does.
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

/// Grant the package SID `GENERIC_READ | GENERIC_EXECUTE` on `plugin_dir`,
/// merged into the existing DACL with object+container inherit so files inside
/// are readable. The vault is never passed here, so it stays deny-by-default.
fn grant_appcontainer_read(plugin_dir: &std::ffi::OsStr, sid: PSID) -> Result<(), String> {
    let mut path = wide_path(plugin_dir);
    let mut old_dacl: *mut ACL = std::ptr::null_mut();
    let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    // SAFETY: out-params populated on success; `path` is live.
    let rc = unsafe {
        GetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut old_dacl,
            std::ptr::null_mut(),
            &mut sd,
        )
    };
    if rc != 0 {
        return Err(format!("GetNamedSecurityInfoW failed: {rc}"));
    }

    let ea = EXPLICIT_ACCESS_W {
        grfAccessPermissions: GENERIC_READ | GENERIC_EXECUTE,
        grfAccessMode: GRANT_ACCESS,
        grfInheritance: OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE,
        Trustee: TRUSTEE_W {
            pMultipleTrustee: std::ptr::null_mut(),
            MultipleTrusteeOperation: 0,
            TrusteeForm: TRUSTEE_IS_SID,
            TrusteeType: TRUSTEE_IS_GROUP,
            ptstrName: sid as *mut u16, // for TRUSTEE_IS_SID, this field holds the PSID
        },
    };

    let mut new_dacl: *mut ACL = std::ptr::null_mut();
    // SAFETY: `ea` and `old_dacl` are live; `new_dacl` is an out-param freed below.
    let rc = unsafe { SetEntriesInAclW(1, &ea, old_dacl, &mut new_dacl) };
    if rc != 0 {
        // SAFETY: `sd` came from GetNamedSecurityInfoW and must be LocalFree'd.
        unsafe { LocalFree(sd as _) };
        return Err(format!("SetEntriesInAclW failed: {rc}"));
    }

    // SAFETY: `new_dacl` is the merged DACL; `path` is live.
    let rc = unsafe {
        SetNamedSecurityInfoW(
            path.as_mut_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            new_dacl,
            std::ptr::null_mut(),
        )
    };
    // SAFETY: free the buffers allocated by the two calls above.
    unsafe {
        LocalFree(new_dacl as _);
        LocalFree(sd as _);
    }
    if rc != 0 {
        return Err(format!("SetNamedSecurityInfoW failed: {rc}"));
    }
    Ok(())
}

/// Quote one argv element per the CRT/CommandLineToArgvW rules and append to
/// `out`. (`CreateProcessW` takes a single command line, not an argv vector.)
fn append_quoted(out: &mut String, arg: &str) {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        out.push_str(arg);
        return;
    }
    out.push('"');
    let mut backslashes = 0usize;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                for _ in 0..(backslashes * 2 + 1) {
                    out.push('\\');
                }
                backslashes = 0;
                out.push('"');
            }
            _ => {
                for _ in 0..backslashes {
                    out.push('\\');
                }
                backslashes = 0;
                out.push(c);
            }
        }
    }
    for _ in 0..(backslashes * 2) {
        out.push('\\');
    }
    out.push('"');
}

fn confine_and_run(args: &[OsString]) -> Result<u8, String> {
    // Parse: --plugin-dir <dir> -- <cmd> <args...>
    if args.first().map(|a| a != "--plugin-dir").unwrap_or(true) {
        return Err("usage: --plugin-dir <dir> -- <cmd> [args...]".into());
    }
    let plugin_dir = args
        .get(1)
        .ok_or("missing <dir> after --plugin-dir")?
        .clone();
    if args.get(2).map(|a| a != "--").unwrap_or(true) {
        return Err("expected `--` after --plugin-dir <dir>".into());
    }
    let cmd = args.get(3).ok_or("missing <cmd> after --")?.clone();
    let inner: Vec<&OsString> = args.iter().skip(4).collect();

    let sid = ensure_app_container_sid()?;
    grant_appcontainer_read(&plugin_dir, sid)?;

    // Build the command line: argv0 = cmd, then the inner args.
    let mut cmdline = String::new();
    append_quoted(&mut cmdline, &cmd.to_string_lossy());
    for a in &inner {
        cmdline.push(' ');
        append_quoted(&mut cmdline, &a.to_string_lossy());
    }
    let mut cmdline_w = wide(&cmdline);

    // SECURITY_CAPABILITIES with the package SID and NO capabilities → the
    // built-in WFP filter blocks all sockets (no network).
    let mut caps = SECURITY_CAPABILITIES {
        AppContainerSid: sid,
        Capabilities: std::ptr::null_mut(),
        CapabilityCount: 0,
        Reserved: 0,
    };

    // STARTUPINFOEXW with a one-entry attribute list carrying the capabilities.
    // SAFETY: zeroed STARTUPINFOEXW is a valid initial state.
    let mut si: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
    si.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;

    let mut attr_size: usize = 0;
    // SAFETY: first call with null list returns required size in `attr_size`.
    unsafe { InitializeProcThreadAttributeList(std::ptr::null_mut(), 1, 0, &mut attr_size) };
    let mut attr_buf = vec![0u8; attr_size];
    si.lpAttributeList = attr_buf.as_mut_ptr() as _;
    // SAFETY: `attr_buf` is sized per the probe call above.
    if unsafe { InitializeProcThreadAttributeList(si.lpAttributeList, 1, 0, &mut attr_size) } == 0 {
        return Err("InitializeProcThreadAttributeList failed".into());
    }
    // SAFETY: `caps` outlives the CreateProcessW call below.
    let ok = unsafe {
        UpdateProcThreadAttribute(
            si.lpAttributeList,
            0,
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
            &mut caps as *mut _ as _,
            std::mem::size_of::<SECURITY_CAPABILITIES>(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err("UpdateProcThreadAttribute failed".into());
    }

    // Kill-on-close job so the whole tree dies with the launcher.
    // SAFETY: job handle is closed before return.
    let job: HANDLE = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job.is_null() {
        return Err("CreateJobObjectW failed".into());
    }
    // SAFETY: zeroed limit struct is a valid initial state.
    let mut jli: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    jli.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: `jli` is live and correctly sized.
    let set_ok = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &mut jli as *mut _ as _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if set_ok == 0 {
        // SAFETY: `job` is a valid handle.
        unsafe { CloseHandle(job) };
        return Err("SetInformationJobObject failed".into());
    }

    // SAFETY: zeroed PROCESS_INFORMATION is a valid initial state.
    let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: `cmdline_w` is a mutable, NUL-terminated wide buffer; `si` carries
    // a valid attribute list; handles inherited so stdio passes through.
    let created = unsafe {
        CreateProcessW(
            std::ptr::null(),
            cmdline_w.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1, // bInheritHandles = TRUE → inherit stdio
            EXTENDED_STARTUPINFO_PRESENT | CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT,
            std::ptr::null(),
            std::ptr::null(),
            &si.StartupInfo,
            &mut pi,
        )
    };
    // SAFETY: attribute list no longer needed once the process is created.
    unsafe { DeleteProcThreadAttributeList(si.lpAttributeList) };
    if created == 0 {
        // SAFETY: `job` is a valid handle.
        unsafe { CloseHandle(job) };
        return Err(
            "CreateProcessW failed (is the command inside an AppContainer-readable path?)".into(),
        );
    }

    // Assign to the job BEFORE resuming so the child cannot escape confinement.
    // SAFETY: both handles are valid from CreateProcessW.
    let assigned = unsafe { AssignProcessToJobObject(job, pi.hProcess) };
    if assigned == 0 {
        // SAFETY: the child is still suspended; terminate it rather than let it
        // run outside the job, then release all handles.
        unsafe {
            TerminateProcess(pi.hProcess, 1);
            CloseHandle(pi.hThread);
            CloseHandle(pi.hProcess);
            CloseHandle(job);
        }
        return Err("AssignProcessToJobObject failed".into());
    }
    // SAFETY: assign succeeded; release the child to run inside the job.
    unsafe { ResumeThread(pi.hThread) };

    // Wait and collect the exit code.
    // SAFETY: `pi.hProcess` valid until we CloseHandle it.
    let mut code: u32 = 1;
    unsafe {
        if WaitForSingleObject(pi.hProcess, INFINITE) == WAIT_OBJECT_0 {
            GetExitCodeProcess(pi.hProcess, &mut code);
        }
        CloseHandle(pi.hThread);
        CloseHandle(pi.hProcess);
        CloseHandle(job);
    }
    Ok((code & 0xFF) as u8)
}
