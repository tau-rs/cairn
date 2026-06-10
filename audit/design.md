# Cairn — Design Findings

Subsections: Code design, DX (developer experience), UX (user experience).

---

## Code design

### D1. `content_hash` is documented "do not persist" but is persisted and reloaded

**Severity: Medium**

**Locations:**
- `crates/cairn-domain/src/note.rs:127-138` (doc: "not stable across Rust versions
  or process restarts — do not persist it or compare hashes across processes")
- `crates/cairn-app/src/lib.rs:194-213` (`save_state` writes the memo hash to `state.json`)
- `crates/cairn-app/src/lib.rs:155-192, 557-575` (`reconcile_warm` / `parse_state` load it back into `memo`)

**Description.** `content_hash` uses `DefaultHasher` and is explicitly documented as
non-portable and not-for-persistence. Yet `save_state` serializes the memo
(`hash`) into `.cairn/state.json`, and `reconcile_warm` loads it back into the
in-memory memo, where `apply_change` later compares fresh hashes against it for
change deduplication.

**Impact.** The persisted hash becomes meaningless after a Rust/std upgrade
(SipHash keying/algorithm changes), silently weakening dedup; the code directly
contradicts its own contract. The stamp guard masks most of the damage, which
makes the latent bug hard to notice.

**Recommendation.** Either stop persisting the hash and recompute on load (rely on
the stamp for the fast path), or switch to a stable, explicitly-versioned hash
(e.g. a fixed FNV/xxhash with a schema version in `state.json`) and update the doc.

### D2. CLI re-indexes the entire vault on every invocation

**Severity: Low**

**Location:** `crates/cairn-cli/src/main.rs:134-136`

**Description.** Every CLI command builds an in-memory Tantivy index and calls
`engine.reindex` (full `list` + `read` + `parse` + index of all notes) before
dispatching — even for a single-note `read`, a `backlinks`, or `commit`.

**Impact.** O(vault size) work and full disk read per command; CLI latency grows
linearly with the cairn even for trivial operations.

**Recommendation.** Reuse the persisted on-disk index path the daemon already has
(`reconcile`), or lazily index only what a command needs (e.g. skip indexing for
`read`/`commit`).

### D3. Stringly-typed error plumbing at boundaries

**Severity: Low**

**Locations:**
- `crates/cairn-ports/src/lib.rs:11-13` (`PortError::Adapter(String)`)
- `crates/cairn-infra/src/git.rs` (every op `.map_err(|e| PortError::Adapter(e.to_string()))`)
- `crates/cairn-cli/src/main.rs` / `crates/cairn-daemon/src/main.rs` (`.map_err(|e| e.to_string())`)
- `crates/cairn-service/src/lib.rs:34-36` (`ServiceError::Internal(String)`)

**Description.** Errors are flattened to strings early and often. The original
`git2::Error` codes, `std::io::Error` kinds, and Tantivy errors are discarded at
the adapter boundary, leaving only a message.

**Impact.** Callers cannot match on cause (e.g. distinguish "lock held" from
"corrupt repo"); harder to test and to surface actionable messages.

**Recommendation.** Carry a `#[source]` typed error (or at least an error-kind
enum) in `PortError::Adapter` so structure survives to the edges.

### D4. `parse_state` discards the parse error

**Severity: Low**

**Location:** `crates/cairn-app/src/lib.rs:557` (`fn parse_state(...) -> Result<RestoredState, ()>`)

**Description.** A failed `state.json` parse maps to `Err(())`, throwing away why
it failed; `reconcile` then silently falls back to a full cold rebuild
(`crates/cairn-app/src/lib.rs:138-146`).

**Impact.** A persistently corrupt/incompatible state file causes a silent full
rebuild on every startup with no signal — slow and undiagnosable. (Also a
diagnostics finding; see diagnostics.md G3.)

**Recommendation.** Return a real error and log it at warn level before falling
back.

### D5. Mixed static/dynamic dispatch and concrete-type pinning

**Severity: Low**

**Locations:**
- `crates/cairn-app/src/lib.rs:53-61` (`Engine<S, I, V>` + `Box<dyn PluginHost>`)
- `crates/cairn-daemon/src/lib.rs:29` (`type CairnEngine = Engine<LocalFsStore, TantivyIndex, GitVcs>`)

**Description.** The engine is generic over three port params threaded through
every signature in `cairn-service`, but the plugin host is already a trait object,
and the daemon immediately pins all three generics to one concrete tuple. The
generics buy little beyond test substitution while adding signature noise across
the dispatcher.

**Impact.** API surface friction (every dispatch fn carries `<S, I, V>`); minor.

**Recommendation.** Consider boxing the three ports behind trait objects (as the
plugin host already is) to collapse the generics, or keep generics only where a
test genuinely substitutes an adapter.

---

## DX (developer experience)

### D6. Misleading "capabilities recorded only (not enforced)" doc

**Severity: Low**

**Locations:**
- `crates/cairn-plugin-protocol/src/lib.rs:202` ("Declared capabilities — recorded only (not enforced in slice 1).")
- `crates/cairn-infra/src/plugin_host.rs:183-199` (capabilities *are* enforced for callbacks)

**Description.** The protocol doc says capabilities are not enforced, but the host
does gate every callback on them. The comment is stale and contradicts behavior.

**Impact.** A plugin author reading the protocol crate will misunderstand the
security model.

**Recommendation.** Update the doc to state callbacks are capability-gated host-side
(and reference the limits noted in security.md S3).

### D7. Commit author hardcoded, ignores git identity

**Severity: Low**

**Location:** `crates/cairn-infra/src/git.rs:50` (`Signature::now("Cairn", "cairn@localhost")`)

**Description.** Every commit is authored/committed as `Cairn <cairn@localhost>`,
ignoring the user's configured git identity.

**Impact.** Cairn-made commits are indistinguishable per-author and look foreign in
`git log`/blame; surprising for a "git-backed" tool.

**Recommendation.** Use the repo/global git signature when available
(`repo.signature()`), falling back to the cairn default only if unset.

### D8. Duplicated startup logic, no shared crate

**Severity: Low**

**Locations:**
- `crates/cairn-cli/src/main.rs:112-132`
- `crates/cairn-daemon/src/main.rs:40-56`

**Description.** Engine construction and the `.git`-existence "is this a cairn?"
check are copy-pasted between CLI and daemon (the code comments acknowledge it).

**Impact.** Drift risk; two places to fix bugs like the symlink/`.git` issues.

**Recommendation.** Extract a small `cairn-startup` (or reuse `cairn-app`) helper
for "open an existing cairn" and "build engine."

---

## UX (user experience)

### D9. `init` is silently non-idempotent and always reports success

**Severity: Low**

**Location:** `crates/cairn-cli/src/main.rs:138-141`

**Description.** `init` always prints `initialized cairn at <path>` even when the
directory was already a cairn (it runs `open_or_init` + reindex unconditionally).
There is no "already initialized" feedback.

**Impact.** Users cannot tell whether `init` created or no-oped; encourages
re-running blindly.

**Recommendation.** Detect an existing `.git`/cairn and print a distinct
"already a cairn" message; consider erroring if the user expects a fresh init.

### D10. Read/permission errors surface to users as "not found"

**Severity: Low** (root cause is diagnostics.md G2)

**Location:** `crates/cairn-infra/src/localfs.rs:78-80`

**Description.** `read` maps every IO error to `NotFound`, so a permission-denied
or is-a-directory error is reported to the user as "note not found: <path>".

**Impact.** Misleading CLI/daemon error messages that send users debugging the
wrong problem.

**Recommendation.** Map only `ErrorKind::NotFound` to `NotFound`; pass others as
`Adapter` with the real cause.

### D11. Sub-2-character search silently returns nothing

**Severity: Low**

**Location:** `crates/cairn-infra/src/tantivy_index.rs:144-148`

**Description.** A query shorter than the minimum n-gram (2 chars), or whitespace,
returns an empty result set with no explanation.

**Impact.** Users searching `a` or a single CJK character get "no results" and may
assume the index is broken rather than that the query is too short.

**Recommendation.** Surface a hint (CLI message / structured warning) when a query
is rejected for being below the n-gram minimum.
