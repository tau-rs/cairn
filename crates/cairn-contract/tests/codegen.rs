//! Verifies the `#[ts(export)]` bindings generate without error.
use cairn_contract::{Command, Event, Query};
use ts_rs::TS;

#[test]
fn exports_typescript_bindings() {
    // `decl()` returns the full TypeScript type declaration string.
    assert!(Command::decl().contains("Command"));
    assert!(Query::decl().contains("Query"));
    assert!(Event::decl().contains("Event"));
    // `export_all()` writes the type and all its dependencies to `TS_RS_EXPORT_DIR`
    // (default: `./bindings`).
    Command::export_all().unwrap();
    Query::export_all().unwrap();
    Event::export_all().unwrap();
}
