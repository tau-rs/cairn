//! The `cairn` CLI: an in-process consumer of the engine.

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use cairn_app::{Engine, Event, EventSink};
use cairn_contract::{Command as WireCommand, CommandResponse, Query as WireQuery, QueryResponse};
use cairn_infra::{GitVcs, LocalFsStore, NotifyWatcher, TantivyIndex};
use cairn_ports::Watcher;
use cairn_service::{app_event_to_wire, dispatch_command, dispatch_query, run_watch_loop};
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
}

fn build_engine(root: &Path) -> Result<Engine<LocalFsStore, TantivyIndex, GitVcs>, String> {
    let store = LocalFsStore::open(root).map_err(|e| e.to_string())?;
    let vcs = GitVcs::open_or_init(root).map_err(|e| e.to_string())?;
    let index = TantivyIndex::in_memory().map_err(|e| e.to_string())?;
    Ok(Engine::new(store, index, vcs))
}

fn run() -> Result<(), String> {
    let cli = Cli::parse();
    let root = cli.cairn;
    let mut events: Vec<Event> = Vec::new();

    // Only `init` may create a new cairn. Every other command requires an
    // existing one, so we never silently `git init` in the user's directory.
    // `.git` is a dir in a normal repo but a file in worktrees/submodules.
    if !matches!(cli.command, Command::Init) && !root.join(".git").exists() {
        return Err(format!(
            "not a cairn at {0} (run `cairn --cairn {0} init` first)",
            root.display()
        ));
    }

    let mut engine = build_engine(&root)?;
    // Always reindex on startup so queries see current content.
    engine.reindex(&mut events).map_err(|e| e.to_string())?;

    match cli.command {
        Command::Init => {
            println!("initialized cairn at {}", root.display());
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
            if let QueryResponse::Graph { edges, .. } =
                dispatch_query(&engine, &WireQuery::GetGraph).map_err(|e| e.to_string())?
            {
                for edge in edges {
                    println!("{} -> {}", edge.from, edge.to);
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
    }
    Ok(())
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
            ],
        );
        assert_eq!(out, "changed a.md\nremoved b.md\n");
    }

    #[test]
    fn json_lines_include_all_wire_events() {
        let out = render(
            true,
            vec![
                Event::NoteChanged(NotePath::new("a.md").unwrap()),
                Event::Reindexed(2),
            ],
        );
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"type\":\"note_changed\""));
        assert!(lines[0].contains("\"path\":\"a.md\""));
        assert!(lines[1].contains("\"type\":\"reindexed\""));
        assert!(lines[1].contains("\"count\":2"));
    }
}
