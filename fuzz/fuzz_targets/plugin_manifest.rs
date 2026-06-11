#![no_main]
//! Fuzz plugin manifest TOML parsing (security.md S1/S3) — the daemon
//! deserializes untrusted `.cairn/plugins/*/manifest.toml` into `Manifest`
//! (plugin_host.rs:422). Parsing arbitrary bytes must never panic.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = toml::from_str::<cairn_plugin_protocol::Manifest>(s);
    }
});
