# Mutex-Poisoning DoS Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A single triggerable panic — especially a panicking plugin host — must not brick the running daemon by permanently poisoning the engine mutex.

**Architecture:** Two layers of defense. (1) Root cause: wrap plugin invocations (`invoke_plugin_command`, `dispatch_plugin_event`) in `std::panic::catch_unwind` so a panicking host is converted to an error instead of unwinding through the locked engine. (2) Defense-in-depth: the daemon recovers from a poisoned `Mutex` via `lock().unwrap_or_else(|e| e.into_inner())` rather than `.expect(...)`, so any other panic that does poison the lock no longer 500s every subsequent request forever.

**Tech Stack:** Rust, std `Mutex`, `std::panic::catch_unwind` + `AssertUnwindSafe`. No new dependencies (the std recovery path avoids pulling in `parking_lot`).

**Audit finding:** Mutex-poisoning denial of service (MEDIUM, security) — `audit/security.md`. Locations: `crates/cairn-daemon/src/lib.rs` (`.lock().expect("engine mutex poisoned")`), `crates/cairn-app/src/lib.rs` (`invoke_plugin_command` — "a panicking plugin host is not caught").

---

### Task 1: Isolate plugin panics in the engine (`cairn-app`)

**Files:**
- Modify: `crates/cairn-app/src/lib.rs` (`invoke_plugin_command` ~516-534, `dispatch_plugin_event` ~538-545, doc comment ~507-512)
- Test: `crates/cairn-app/src/lib.rs` (inline `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing test**

Add to the `tests` module (near the other `PluginHost` stubs):

```rust
/// A host whose invoke panics — simulates a buggy/malicious plugin host.
struct PanickingHost;
impl PluginHost for PanickingHost {
    fn plugins(&self) -> Vec<PluginInfo> {
        vec![PluginInfo {
            id: "boom".into(),
            name: "boom".into(),
            version: "0".into(),
            commands: Vec::new(),
        }]
    }
    fn invoke(
        &mut self,
        _plugin: &str,
        _command: &str,
        _args: &serde_json::Value,
        _callbacks: &mut dyn cairn_ports::PluginCallbacks,
    ) -> Result<serde_json::Value, PortError> {
        panic!("plugin host panicked mid-invoke");
    }
}

#[test]
fn plugin_panic_is_caught_and_engine_survives() {
    let tmp = tempfile::tempdir().unwrap();
    let mut eng = engine(tmp.path());
    let mut events = Vec::new();
    let a = NotePath::new("a.md").unwrap();
    eng.write_note(&a, "hello body", &mut events).unwrap();

    eng.set_plugin_host(Box::new(PanickingHost));
    let mut sink: Vec<Event> = Vec::new();
    // The panic must be converted to an error, not unwind through the caller.
    let res = eng.invoke_plugin_command("boom", "x", &serde_json::Value::Null, &mut sink);
    assert!(matches!(res, Err(PortError::Adapter(_))));

    // The engine is still usable afterward — its state was not corrupted.
    assert_eq!(eng.read_note(&a).unwrap(), "hello body");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-app plugin_panic_is_caught_and_engine_survives`
Expected: FAIL — the test process unwinds/aborts on the uncaught `panic!` (test reported as failed/panicked).

- [ ] **Step 3: Implement `catch_unwind` in both plugin entry points**

Replace `invoke_plugin_command`'s body (keep signature) so the host call is caught:

```rust
        // Move the real host into a local so `self.plugins` no longer aliases it;
        // the callbacks handler can then borrow the rest of `self` (the store) to
        // service host-callbacks the plugin sends mid-invoke.
        let mut host = std::mem::replace(&mut self.plugins, Box::new(NoopPluginHost));
        // Catch a panicking host so it surfaces as an error instead of unwinding
        // through the daemon's locked engine (which would poison the mutex and
        // brick the daemon — audit: mutex-poisoning DoS). `self`/`host` are only
        // borrowed for the call, so `AssertUnwindSafe` is sound: on panic the
        // engine's RefCell/borrow guards unwind cleanly and `host` is restored.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut cb = EngineCallbacks { engine: self, sink };
            host.invoke(plugin, command, args, &mut cb)
            // cb is dropped here, releasing the &mut self borrow
        }));
        self.plugins = host;
        result.unwrap_or_else(|_| Err(PortError::Adapter("plugin host panicked".into())))
```

Apply the same protection to `dispatch_plugin_event` (best-effort; a panic is swallowed since the method returns `()`):

```rust
        let mut host = std::mem::replace(&mut self.plugins, Box::new(NoopPluginHost));
        // Catch a panicking host (see invoke_plugin_command) so a plugin can't
        // poison the daemon's engine mutex via event dispatch. Best-effort.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut cb = EngineCallbacks { engine: self, sink };
            host.dispatch_event(event, &mut cb);
        }));
        self.plugins = host;
```

Update the `invoke_plugin_command` doc comment: replace the paragraph claiming a panicking host poisons the engine and leaves `NoopPluginHost`, with a note that a panic is caught and surfaced as `PortError::Adapter`, and the host is restored.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p cairn-app plugin_panic_is_caught_and_engine_survives`
Expected: PASS (a "thread panicked" line may print to stderr from the default hook — that is the caught panic, not a failure).

- [ ] **Step 5: Run the full crate test suite**

Run: `cargo test -p cairn-app`
Expected: all pass.

---

### Task 2: Recover from a poisoned engine mutex (`cairn-daemon`)

**Files:**
- Modify: `crates/cairn-daemon/src/lib.rs` (add private `engine()` helper on `AppState`; replace the three `.lock().expect("engine mutex poisoned")` sites at ~105, ~133, ~141)
- Test: `crates/cairn-daemon/src/lib.rs` (inline `#[cfg(test)] mod tests` — needs the private `engine` field)

- [ ] **Step 1: Write the failing test**

Add to the bottom of `crates/cairn-daemon/src/lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use cairn_infra::{GitVcs, LocalFsStore, TantivyIndex};

    fn state(dir: &std::path::Path) -> AppState {
        AppState::new(Engine::new(
            LocalFsStore::open(dir).unwrap(),
            TantivyIndex::in_memory().unwrap(),
            GitVcs::open_or_init(dir).unwrap(),
        ))
    }

    #[test]
    fn poisoned_engine_mutex_still_serves_requests() {
        let tmp = tempfile::tempdir().unwrap();
        let state = state(tmp.path());

        // Poison the mutex: panic while holding the engine lock.
        let st = state.clone();
        let _ = std::thread::spawn(move || {
            let _guard = st.engine.lock().unwrap();
            panic!("simulated engine panic under lock");
        })
        .join();
        assert!(state.engine.is_poisoned(), "precondition: mutex is poisoned");

        // Despite poisoning, a query must still succeed rather than 500 forever.
        let resp = state.run_query_blocking(&Query::ListNotes);
        assert!(resp.is_ok(), "poisoned mutex must be recovered, not propagated");
    }
}
```

(Confirm the `Query` variant name for "list notes" in `cairn-contract` and adjust `Query::ListNotes` if the actual variant differs.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p cairn-daemon poisoned_engine_mutex_still_serves_requests`
Expected: FAIL — `run_query_blocking` hits `.lock().expect("engine mutex poisoned")` and panics.

- [ ] **Step 3: Add the recovery helper and use it at all three sites**

Add a private method on `AppState` (in the `impl AppState` block):

```rust
    /// Lock the engine, recovering from poisoning instead of propagating it.
    ///
    /// A panic in any engine operation poisons the `Mutex`; with `.expect(...)`
    /// every subsequent request would panic and 500 forever (audit: mutex-
    /// poisoning DoS). The data behind the lock is a single engine whose
    /// invariants are re-established on the next operation, so recovering the
    /// guard and continuing is correct.
    fn engine(&self) -> std::sync::MutexGuard<'_, CairnEngine> {
        self.engine.lock().unwrap_or_else(|e| e.into_inner())
    }
```

Replace the three sites:
- `run_command_blocking`: `let mut guard = self.engine.lock().expect("engine mutex poisoned");` → `let mut guard = self.engine();`
- `run_query_blocking`: `let guard = self.engine.lock().expect("engine mutex poisoned");` → `let guard = self.engine();`
- `apply_change_blocking`: `let mut guard = self.engine.lock().expect("engine mutex poisoned");` → `let mut guard = self.engine();`

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p cairn-daemon poisoned_engine_mutex_still_serves_requests`
Expected: PASS.

- [ ] **Step 5: Run the full crate test suite**

Run: `cargo test -p cairn-daemon`
Expected: all pass.

---

### Task 3: Verify, review, ship

- [ ] **Step 1: Workspace build + lint**

Run: `cargo test -p cairn-app -p cairn-daemon` and `cargo clippy -p cairn-app -p cairn-daemon --all-targets`
Expected: green, no new warnings.

- [ ] **Step 2: requesting-code-review** — confirm scope is only the poisoning/panic-isolation fix.

- [ ] **Step 3: Commit, push, open PR** against `tau-rs/cairn` `main`, citing the finding. No merge.
