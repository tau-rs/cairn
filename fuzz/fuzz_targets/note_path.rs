#![no_main]
//! Fuzz `NotePath::new` — the path-validation surface behind every write
//! (security.md S1/S4/S7). The invariant under test: parsing arbitrary
//! input must never panic; it returns `Ok` or a `NotePathError`.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = cairn_domain::NotePath::new(s);
    }
});
