//! Windows-only behavioral tests for the AppContainer launcher. Each spawns the
//! built launcher binary (`CARGO_BIN_EXE_cairn-sandbox-win`) and asserts the
//! confinement guarantee. Skipped unless AppContainer is available on the host.

#![cfg(windows)]

use std::path::PathBuf;
use std::process::Command;

fn launcher() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_cairn-sandbox-win"))
}

/// True if `--probe` succeeds (AppContainer usable on this host/runner).
fn appcontainer_usable() -> bool {
    Command::new(launcher())
        .arg("--probe")
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[test]
fn probe_reports_appcontainer_available() {
    // GitHub windows-latest supports AppContainer; assert the probe succeeds.
    assert!(
        appcontainer_usable(),
        "--probe must succeed on a runner with AppContainer"
    );
}

#[test]
fn exec_and_pipe_stdout() {
    if !appcontainer_usable() {
        eprintln!("skipping: AppContainer unavailable");
        return;
    }
    // A unique, isolated plugin dir — never the shared %TEMP% root, so parallel
    // tests don't contend on the same directory's ACL.
    let base = std::env::temp_dir().join(format!("cairn_sbx_x_{}", std::process::id()));
    let plugin_dir = base.join("plugin");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    let cmd = PathBuf::from(r"C:\Windows\System32\cmd.exe");
    let out = Command::new(launcher())
        .arg("--plugin-dir")
        .arg(&plugin_dir)
        .arg("--")
        .arg(&cmd)
        .arg("/c")
        .arg("echo hi")
        .output()
        .expect("spawn launcher");
    assert!(
        out.status.success(),
        "the plugin command must be allowed to exec: {out:?}"
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("hi"),
        "stdout must pipe through the jail: {out:?}"
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn write_to_host_is_denied() {
    if !appcontainer_usable() {
        eprintln!("skipping: AppContainer unavailable");
        return;
    }
    // Isolated plugin dir (granted read) and a sibling host file that is NEVER
    // granted, so the write-deny is independent of the plugin-dir read grant.
    let base = std::env::temp_dir().join(format!("cairn_sbx_w_{}", std::process::id()));
    let plugin_dir = base.join("plugin");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    let target = base.join("escaped.txt");
    let _ = std::fs::remove_file(&target);
    let cmd = PathBuf::from(r"C:\Windows\System32\cmd.exe");
    // Quote the path so a space in %TEMP% does not split the redirect target.
    let redirect = format!("echo x> \"{}\"", target.display());
    let status = Command::new(launcher())
        .arg("--plugin-dir")
        .arg(&plugin_dir)
        .arg("--")
        .arg(&cmd)
        .arg("/c")
        .arg(&redirect)
        .status()
        .expect("spawn launcher");
    assert!(!status.success(), "writing to the host must fail");
    assert!(!target.exists(), "the host file must not be created");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn denies_vault_read_but_allows_plugin_dir() {
    if !appcontainer_usable() {
        eprintln!("skipping: AppContainer unavailable");
        return;
    }
    // Two sibling temp dirs: one acts as the plugin dir (granted), one as the
    // vault (never granted -> unreadable). Distinct dirs avoid granting read on
    // the shared temp root.
    let base = std::env::temp_dir().join(format!("cairn_sbx_{}", std::process::id()));
    let plugin_dir = base.join("plugin");
    let vault = base.join("vault");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::create_dir_all(&vault).unwrap();
    std::fs::write(plugin_dir.join("own.txt"), b"OWN").unwrap();
    std::fs::write(vault.join("secret.md"), b"SECRET").unwrap();
    let cmd = PathBuf::from(r"C:\Windows\System32\cmd.exe");

    let own = plugin_dir.join("own.txt");
    let allowed = Command::new(launcher())
        .arg("--plugin-dir")
        .arg(&plugin_dir)
        .arg("--")
        .arg(&cmd)
        .arg("/c")
        .arg("type")
        .arg(&own)
        .output()
        .expect("spawn");
    assert!(
        allowed.status.success(),
        "reading the plugin's own dir must be allowed: {allowed:?}"
    );
    assert!(
        String::from_utf8_lossy(&allowed.stdout).contains("OWN"),
        "plugin-dir read must return the file contents: {allowed:?}"
    );

    let secret = vault.join("secret.md");
    let denied = Command::new(launcher())
        .arg("--plugin-dir")
        .arg(&plugin_dir)
        .arg("--")
        .arg(&cmd)
        .arg("/c")
        .arg("type")
        .arg(&secret)
        .output()
        .expect("spawn");
    assert!(!denied.status.success(), "reading the vault must be denied");

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn network_is_denied() {
    if !appcontainer_usable() {
        eprintln!("skipping: AppContainer unavailable");
        return;
    }
    // Listen on an ephemeral loopback port from the (unconfined) test process.
    let _listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = _listener.local_addr().unwrap().port();
    // A unique, isolated plugin dir (not the shared %TEMP% root).
    let base = std::env::temp_dir().join(format!("cairn_sbx_n_{}", std::process::id()));
    let plugin_dir = base.join("plugin");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    let cmd = PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe");
    // PowerShell connect attempt: exit 0 on success, 1 on failure.
    let script = format!(
        "try {{ (New-Object Net.Sockets.TcpClient).Connect('127.0.0.1',{port}); exit 0 }} catch {{ exit 1 }}"
    );
    let status = Command::new(launcher())
        .arg("--plugin-dir")
        .arg(&plugin_dir)
        .arg("--")
        .arg(&cmd)
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-Command")
        .arg(&script)
        .status()
        .expect("spawn launcher");
    assert!(
        !status.success(),
        "an AppContainer with no network capability must not connect"
    );
    let _ = std::fs::remove_dir_all(&base);
}
