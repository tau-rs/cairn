//! The `cairn` CLI: an in-process consumer of the engine.

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use cairn_app::{Event, EventSink};
use cairn_contract::{Command as WireCommand, CommandResponse, Query as WireQuery, QueryResponse};
use cairn_infra::{NotifyWatcher, MIN_QUERY_CHARS};
use cairn_ports::Watcher;
use cairn_service::{app_event_to_wire, dispatch_command, dispatch_query, run_watch_loop};
use cairn_startup::{build_engine, ensure_cairn};
use clap::{Parser, Subcommand};

/// Renders engine events for `cairn watch`. Generic over the writer so it is
/// unit-testable without spawning the blocking command.
struct WatchSink<W: Write> {
    json: bool,
    out: W,
}

impl<W: Write> EventSink for WatchSink<W> {
    fn emit(&mut self, event: Event) {
        if self.json {
            let wire = app_event_to_wire(event);
            let _ = writeln!(
                self.out,
                "{}",
                serde_json::to_string(&wire).expect("wire event serializes")
            );
        } else {
            match event {
                Event::NoteChanged(p) => {
                    let _ = writeln!(self.out, "changed {}", p.as_str());
                }
                Event::NoteDeleted(p) => {
                    let _ = writeln!(self.out, "removed {}", p.as_str());
                }
                // Reindexed / Committed are noise for a human watch view.
                _ => {}
            }
        }
    }
}

/// Renders agent events for `cairn ask`: answer text to stdout (flushed per
/// chunk so it streams), tool/error chatter to stderr.
struct AgentStdoutSink;

impl cairn_ports::AgentSink for AgentStdoutSink {
    fn emit(&mut self, event: cairn_ports::AgentEvent) {
        use cairn_ports::AgentEvent::{
            Completed, Failed, TextDelta, ToolCompleted, ToolStarted, TurnCompleted,
        };
        match event {
            TextDelta(text) => {
                print!("{text}");
                let _ = std::io::stdout().flush();
            }
            ToolStarted { tool } => eprintln!("  [tool {tool}…]"),
            ToolCompleted { tool, ok } => {
                eprintln!("  [tool {tool} {}]", if ok { "ok" } else { "error" });
            }
            TurnCompleted => {}
            Completed => println!(),
            Failed { message } => eprintln!("\nagent error: {message}"),
            // tau's event vocabulary is #[non_exhaustive]; ignore unknown kinds.
            _ => {}
        }
    }
}

#[derive(Parser)]
#[command(name = "cairn", about = "Cairn note engine")]
struct Cli {
    /// Path to the cairn (defaults to the current directory).
    #[arg(long, default_value = ".")]
    cairn: std::path::PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Initialize a new cairn (git repo + directory).
    Init,
    /// Create or overwrite a note from a string.
    Write {
        /// Relative note path, e.g. `notes/a.md`.
        path: String,
        /// Markdown contents.
        contents: String,
    },
    /// Print a note's contents.
    Read {
        /// Relative note path.
        path: String,
    },
    /// Watch the cairn for changes and stream them until interrupted.
    Watch {
        /// Emit one JSON event per line instead of human-readable lines.
        #[arg(long)]
        json: bool,
    },
    /// Rename or move a note (link-aware).
    Rename {
        /// Current relative path.
        from: String,
        /// New relative path (may be in a different directory).
        to: String,
    },
    /// Search notes.
    Search {
        /// Query string.
        query: String,
    },
    /// Ask a question; answers grounded in your notes, streamed.
    Ask {
        /// The question.
        query: String,
    },
    /// List notes that link to a note.
    Backlinks {
        /// Relative note path.
        path: String,
    },
    /// Commit all changes.
    Commit {
        /// Commit message.
        message: String,
    },
    /// List all notes with their titles.
    List,
    /// Print the link graph as `from -> to` edges.
    Graph,
    /// List all tags with note counts.
    Tags,
    /// List notes carrying a tag.
    Tagged {
        /// The tag to filter by.
        tag: String,
    },
    /// Show a note's commit history (newest first).
    History {
        /// Relative note path.
        path: String,
    },
    /// Print a note's contents at a past revision.
    Show {
        /// Relative note path.
        path: String,
        /// A git revspec (short/full hash, `HEAD~1`…).
        revision: String,
    },
    /// Print the link graph as of a past revision.
    GraphAt {
        /// A git revspec (short/full hash, `HEAD~1`…).
        revision: String,
    },
    /// Print what changed in the link graph between two revisions.
    GraphDiff {
        /// Older revspec.
        from: String,
        /// Newer revspec.
        to: String,
    },
    /// Restore a note to a past revision (writes that version as current).
    Restore {
        /// Relative note path.
        path: String,
        /// A git revspec to restore from.
        revision: String,
    },
    /// Manage plugins installed from a git URL.
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
    },
}

#[derive(Subcommand)]
enum PluginAction {
    /// Install a plugin from a git URL. It lands UNTRUSTED - approve it in
    /// cairn.toml before it will run.
    Add {
        /// Git URL (https or ssh).
        url: String,
        /// Branch, tag, or commit to check out (default: the remote's HEAD).
        #[arg(long)]
        r#ref: Option<String>,
    },
    /// List installed plugins with trust and update status.
    List {
        /// Skip the network check for available updates.
        #[arg(long)]
        offline: bool,
    },
    /// Remove an installed plugin's files (does not revoke trust).
    Remove {
        /// Plugin id (its directory name under .cairn/plugins).
        id: String,
    },
}

/// Whether a command needs the O(vault) startup reindex. `search` consults the
/// full-text index it builds; `watch` seeds its dedup memo/stamps from it so a
/// spurious first event on a pre-existing note is suppressed. Every other
/// command reads the store or the lazy notes-cache directly, so we skip the
/// full disk read for them — it is wasted on a one-shot `read`, `commit`, or
/// `backlinks` (audit D2).
fn needs_startup_reindex(command: &Command) -> bool {
    matches!(
        command,
        Command::Search { .. } | Command::Watch { .. } | Command::Ask { .. }
    )
}

/// The message `init` prints. Distinguishes a freshly created cairn from a
/// re-run on an existing one, so `init` is no longer silently a no-op that
/// always reports success (audit D9).
fn init_message(already: bool, root: &Path) -> String {
    if already {
        format!("already a cairn at {}", root.display())
    } else {
        format!("initialized cairn at {}", root.display())
    }
}

/// A hint to print when a search query is too short for the n-gram index to
/// match anything. `None` when the query is long enough. Mirrors the index's
/// own `trim().chars().count()` rejection so the hint fires exactly when the
/// index returns empty for being too short (audit D11).
fn short_query_hint(query: &str) -> Option<String> {
    if query.trim().chars().count() < MIN_QUERY_CHARS {
        Some(format!(
            "hint: query is shorter than the {MIN_QUERY_CHARS}-character minimum; no results"
        ))
    } else {
        None
    }
}

fn run() -> Result<(), String> {
    let cli = Cli::parse();
    let root = cli.cairn;
    let mut events: Vec<Event> = Vec::new();

    // Only `init` may create a new cairn. Every other command requires an
    // existing one, so we never silently `git init` in the user's directory.
    // Capture cairn-ness before `build_engine`'s `open_or_init` would create
    // `.git`, so `init` can tell a created cairn from a no-op (D9).
    let is_cairn = cairn_startup::is_cairn(&root);
    if !matches!(cli.command, Command::Init) {
        ensure_cairn(&root).map_err(|e| e.to_string())?;
    }

    // Plugin management is pure fs/git — it needs the cairn root but neither the
    // engine nor the startup reindex.
    if let Command::Plugin { action } = &cli.command {
        return run_plugin(&root, action);
    }

    let mut engine = build_engine(&root).map_err(|e| e.to_string())?;
    // Build the search index only for commands that need it (D2): a full
    // reindex is O(vault) work and a full disk read, wasted on a one-shot
    // `read`, `commit`, or `backlinks`.
    if needs_startup_reindex(&cli.command) {
        engine.reindex(&mut events).map_err(|e| e.to_string())?;
    }

    match cli.command {
        Command::Init => {
            println!("{}", init_message(is_cairn, &root));
        }
        Command::Write { path, contents } => {
            let resp = dispatch_command(
                &mut engine,
                &WireCommand::WriteNote {
                    path: path.clone(),
                    contents,
                },
                &mut events,
            )
            .map_err(|e| e.to_string())?;
            debug_assert!(matches!(resp, CommandResponse::Done));
            println!("wrote {path}");
        }
        Command::Read { path } => {
            match dispatch_query(&engine, &WireQuery::GetNote { path })
                .map_err(|e| e.to_string())?
            {
                QueryResponse::Note { contents } => print!("{contents}"),
                _ => unreachable!("GetNote returns Note"),
            }
        }
        Command::Rename { from, to } => {
            let resp = dispatch_command(
                &mut engine,
                &WireCommand::RenameNote {
                    from: from.clone(),
                    to: to.clone(),
                },
                &mut events,
            )
            .map_err(|e| e.to_string())?;
            debug_assert!(matches!(resp, CommandResponse::Done));
            println!("renamed {from} -> {to}");
        }
        Command::Search { query } => {
            if let Some(hint) = short_query_hint(&query) {
                eprintln!("{hint}");
            }
            if let QueryResponse::SearchResults { results } =
                dispatch_query(&engine, &WireQuery::Search { query }).map_err(|e| e.to_string())?
            {
                for r in results {
                    println!("{}", r.path);
                    if !r.snippet.is_empty() {
                        println!("    {}", r.snippet);
                    }
                }
            }
        }
        Command::Ask { query } => {
            let cfg = cairn_infra::TauConfig::from_env().ok_or_else(|| {
                "tau not configured: set TAU_BIN (and optionally TAU_AGENT, TAU_PROJECT)"
                    .to_string()
            })?;
            let runtime = cairn_infra::TauServeRuntime::new(cfg);
            let mut sink = AgentStdoutSink;
            let cited = cairn_service::augmented_answer(&engine, &query, &runtime, &mut sink, 5)
                .map_err(|e| e.to_string())?;
            if !cited.is_empty() {
                eprintln!("sources:");
                for path in cited {
                    eprintln!("  - {path}");
                }
            }
        }
        Command::Backlinks { path } => {
            if let QueryResponse::Paths { paths } =
                dispatch_query(&engine, &WireQuery::GetBacklinks { path })
                    .map_err(|e| e.to_string())?
            {
                for p in paths {
                    println!("{p}");
                }
            }
        }
        Command::Commit { message } => {
            let resp = dispatch_command(&mut engine, &WireCommand::Commit { message }, &mut events)
                .map_err(|e| e.to_string())?;
            if let CommandResponse::Committed { commit } = resp {
                println!("committed {commit}");
            }
        }
        Command::List => {
            if let QueryResponse::Notes { notes } =
                dispatch_query(&engine, &WireQuery::ListNotes).map_err(|e| e.to_string())?
            {
                for n in notes {
                    println!("{}\t{}", n.path, n.title);
                }
            }
        }
        Command::Graph => {
            if let QueryResponse::Graph { nodes, edges } = dispatch_query(
                &engine,
                &WireQuery::GetGraph {
                    scope: cairn_contract::GraphScope::Full,
                },
            )
            .map_err(|e| e.to_string())?
            {
                for n in &nodes {
                    println!("{}\t{}", n.path, n.title);
                }
                for edge in edges {
                    println!("{} -> {}", edge.from, edge.to);
                }
            }
        }
        Command::GraphAt { revision } => {
            if let QueryResponse::Graph { nodes, edges } = dispatch_query(
                &engine,
                &WireQuery::GraphAt {
                    revision,
                    scope: cairn_contract::GraphScope::Full,
                },
            )
            .map_err(|e| e.to_string())?
            {
                for n in &nodes {
                    println!("{}\t{}", n.path, n.title);
                }
                for edge in edges {
                    println!("{} -> {}", edge.from, edge.to);
                }
            }
        }
        Command::GraphDiff { from, to } => {
            if let QueryResponse::GraphDiff {
                nodes_added,
                nodes_removed,
                // `nodes_changed` is wire-only for now (Stream 7 UI); the CLI
                // diff view intentionally renders only added/removed.
                nodes_changed: _,
                edges_added,
                edges_removed,
            } = dispatch_query(
                &engine,
                &WireQuery::GraphDiff {
                    from,
                    to,
                    scope: cairn_contract::GraphScope::Full,
                },
            )
            .map_err(|e| e.to_string())?
            {
                for n in &nodes_added {
                    println!("+ {}", n.path);
                }
                for n in &nodes_removed {
                    println!("- {}", n.path);
                }
                for e in &edges_added {
                    println!("+ {} -> {}", e.from, e.to);
                }
                for e in &edges_removed {
                    println!("- {} -> {}", e.from, e.to);
                }
            }
        }
        Command::Tags => {
            if let QueryResponse::Tags { tags } =
                dispatch_query(&engine, &WireQuery::ListTags).map_err(|e| e.to_string())?
            {
                for t in tags {
                    println!("{}\t{}", t.tag, t.count);
                }
            }
        }
        Command::Tagged { tag } => {
            if let QueryResponse::Paths { paths } =
                dispatch_query(&engine, &WireQuery::NotesByTag { tag })
                    .map_err(|e| e.to_string())?
            {
                for p in paths {
                    println!("{p}");
                }
            }
        }
        Command::History { path } => {
            if let QueryResponse::History { revisions } =
                dispatch_query(&engine, &WireQuery::NoteHistory { path })
                    .map_err(|e| e.to_string())?
            {
                for r in revisions {
                    println!("{}  {}", r.id, r.message);
                }
            }
        }
        Command::Show { path, revision } => {
            match dispatch_query(&engine, &WireQuery::NoteAt { path, revision })
                .map_err(|e| e.to_string())?
            {
                QueryResponse::Note { contents } => print!("{contents}"),
                _ => unreachable!("NoteAt returns Note"),
            }
        }
        Command::Restore { path, revision } => {
            let resp = dispatch_command(
                &mut engine,
                &WireCommand::RestoreNote {
                    path: path.clone(),
                    revision: revision.clone(),
                },
                &mut events,
            )
            .map_err(|e| e.to_string())?;
            debug_assert!(matches!(resp, CommandResponse::Done));
            println!("restored {path} from {revision}");
        }
        Command::Watch { json } => {
            let handle = NotifyWatcher
                .watch(&root)
                .map_err(|e| format!("file watcher: {e}"))?;
            eprintln!("watching {} for changes", root.display());
            let mut sink = WatchSink {
                json,
                out: std::io::stdout(),
            };
            run_watch_loop(&handle, |change| {
                if let Err(e) = engine.apply_change(change, &mut sink) {
                    eprintln!("watch: {e}");
                }
            });
        }
        Command::Plugin { .. } => unreachable!("Plugin short-circuits before build_engine"),
    }
    Ok(())
}

fn run_plugin(root: &Path, action: &PluginAction) -> Result<(), String> {
    use cairn_infra::plugin_install;
    match action {
        PluginAction::Add { url, r#ref } => {
            let installed =
                plugin_install::install(root, url, r#ref.as_deref()).map_err(|e| e.to_string())?;
            let verb = if installed.updated {
                "updated"
            } else {
                "cloned"
            };
            let short = installed.commit.get(..7).unwrap_or(&installed.commit);
            println!(
                "{verb} {} @ {} ({short}) -> .cairn/plugins/{}/",
                installed.id, installed.git_ref, installed.id
            );
            println!("UNTRUSTED - will not run until you approve. Add to cairn.toml:\n");
            println!("  [[plugins.trusted]]");
            println!("  dir  = \"{}\"", installed.id);
            println!("  hash = \"{}\"", installed.hash);
        }
        PluginAction::List { offline } => {
            let infos = plugin_install::list(root, !offline).map_err(|e| e.to_string())?;
            print_plugin_list(&infos);
        }
        PluginAction::Remove { id } => {
            plugin_install::remove(root, id).map_err(|e| e.to_string())?;
            println!("removed .cairn/plugins/{id}/ and {id}.source.toml");
            println!(
                "note: if \"{id}\" is still in cairn.toml [plugins].trusted, \
                 remove that line to revoke trust."
            );
        }
    }
    Ok(())
}

fn print_plugin_list(infos: &[cairn_infra::plugin_install::InstalledInfo]) {
    use cairn_infra::plugin_install::UpdateStatus;
    if infos.is_empty() {
        println!("no plugins installed");
        return;
    }
    println!("ID\tSOURCE\tPINNED\tTRUSTED\tUPDATE");
    for i in infos {
        let update = match &i.update {
            UpdateStatus::Skipped => "-".to_string(),
            UpdateStatus::UpToDate => "up to date".to_string(),
            UpdateStatus::Available(c) => format!("{} available", c.get(..7).unwrap_or(c)),
            UpdateStatus::Unreachable => "unreachable".to_string(),
        };
        let trusted = if i.trusted { "yes" } else { "no" };
        println!(
            "{}\t{}\t{}\t{}\t{}",
            i.id, i.source, i.pinned_ref, trusted, update
        );
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cairn_domain::NotePath;

    fn render(json: bool, events: Vec<Event>) -> String {
        let mut sink = WatchSink {
            json,
            out: Vec::<u8>::new(),
        };
        for e in events {
            sink.emit(e);
        }
        String::from_utf8(sink.out).unwrap()
    }

    #[test]
    fn human_lines_skip_reindexed() {
        let out = render(
            false,
            vec![
                Event::NoteChanged(NotePath::new("a.md").unwrap()),
                Event::NoteDeleted(NotePath::new("b.md").unwrap()),
                Event::Reindexed(3),
                // Committed is also noise for a human watch view.
                Event::Committed("abc1234".into()),
            ],
        );
        assert_eq!(out, "changed a.md\nremoved b.md\n");
    }

    #[test]
    fn only_search_and_watch_need_the_startup_reindex() {
        // D2: `search` consults the full-text index and `watch` seeds the
        // dedup memo/stamps from it; every other command reads the store /
        // notes-cache directly, so we skip the O(vault) build for them.
        assert!(needs_startup_reindex(&Command::Search {
            query: "x".into()
        }));
        assert!(needs_startup_reindex(&Command::Watch { json: false }));
        assert!(needs_startup_reindex(&Command::Ask { query: "x".into() }));
        assert!(!needs_startup_reindex(&Command::Read {
            path: "a.md".into()
        }));
        assert!(!needs_startup_reindex(&Command::Commit {
            message: "m".into()
        }));
        assert!(!needs_startup_reindex(&Command::Backlinks {
            path: "a.md".into()
        }));
        assert!(!needs_startup_reindex(&Command::Init));
    }

    #[test]
    fn init_message_distinguishes_fresh_from_existing() {
        // D9: a fresh init and a re-run on an existing cairn read differently.
        let p = Path::new("/tmp/v");
        assert_eq!(init_message(false, p), "initialized cairn at /tmp/v");
        assert_eq!(init_message(true, p), "already a cairn at /tmp/v");
    }

    #[test]
    fn short_query_yields_hint() {
        // D11: a sub-2-char or whitespace query gets a hint, not a silent empty.
        assert!(short_query_hint("a").is_some());
        assert!(short_query_hint("  ").is_some());
        assert!(short_query_hint("").is_some());
        assert!(short_query_hint("ab").is_none());
        assert!(short_query_hint("hello").is_none());
    }

    #[test]
    fn json_lines_include_all_wire_events() {
        let out = render(
            true,
            vec![
                Event::NoteChanged(NotePath::new("a.md").unwrap()),
                Event::NoteDeleted(NotePath::new("b.md").unwrap()),
                Event::Reindexed(2),
            ],
        );
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("\"type\":\"note_changed\""));
        assert!(lines[0].contains("\"path\":\"a.md\""));
        assert!(lines[1].contains("\"type\":\"note_deleted\""));
        assert!(lines[1].contains("\"path\":\"b.md\""));
        assert!(lines[2].contains("\"type\":\"reindexed\""));
        assert!(lines[2].contains("\"count\":2"));
    }
}
