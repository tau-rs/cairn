//! Example cairn plugin built on `cairn-plugin-sdk`: declares commands + typed
//! handlers; the SDK owns the JSON-RPC/NDJSON loop and the host-callbacks.
//! `echo` returns its args; `noteLen`/`writeNote`/`noteCount`/`find`/`deleteNote`
//! call back to the host.

use cairn_plugin_sdk::{CairnEvent, Host, Plugin};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
struct PathArgs {
    path: String,
}

#[derive(Deserialize)]
struct WriteArgs {
    path: String,
    contents: String,
}

#[derive(Deserialize)]
struct QueryArgs {
    query: String,
}

fn main() {
    let mut plugin = Plugin::new("example", env!("CARGO_PKG_VERSION"));

    plugin.command("echo", "Echo", |args: Value, _host: &mut Host| Ok(args));

    plugin.command("noteLen", "Note length", |a: PathArgs, host: &mut Host| {
        let contents = host.read_note(&a.path)?;
        Ok(json!({ "len": contents.len() }))
    });

    plugin.command(
        "writeNote",
        "Write note",
        |a: WriteArgs, host: &mut Host| {
            host.write_note(&a.path, &a.contents)?;
            Ok(json!({ "written": true }))
        },
    );

    plugin.command("noteCount", "Note count", |_a: Value, host: &mut Host| {
        let notes = host.list_notes()?;
        Ok(json!({ "count": notes.len() }))
    });

    plugin.command("find", "Find", |a: QueryArgs, host: &mut Host| {
        let hits = host.search(&a.query)?;
        Ok(json!({ "hits": hits.len() }))
    });

    plugin.command(
        "deleteNote",
        "Delete note",
        |a: PathArgs, host: &mut Host| {
            host.delete_note(&a.path)?;
            Ok(json!({ "deleted": true }))
        },
    );

    // On any cairn change, record the changed path into a marker note.
    plugin.on_event(|ev: CairnEvent, host: &mut Host| {
        host.write_note("seen.md", &ev.path)?;
        Ok(())
    });

    // Test fixture: never responds, so the host's read timeout can fire.
    plugin.command("hang", "Hang", |_args: Value, _host: &mut Host| {
        std::thread::sleep(std::time::Duration::from_secs(86_400));
        Ok(json!(null))
    });

    plugin.run();
}
