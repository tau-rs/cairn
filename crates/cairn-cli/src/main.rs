//! The `cairn` CLI: an in-process consumer of the engine.

use std::path::Path;
use std::process::ExitCode;

use cairn_app::{Engine, Event};
use cairn_contract::{Command as WireCommand, CommandResponse, Query as WireQuery, QueryResponse};
use cairn_infra::{GitVcs, InMemoryIndex, LocalFsStore};
use cairn_service::{dispatch_command, dispatch_query};
use clap::{Parser, Subcommand};

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

fn build_engine(root: &Path) -> Result<Engine<LocalFsStore, InMemoryIndex, GitVcs>, String> {
    let store = LocalFsStore::open(root).map_err(|e| e.to_string())?;
    let vcs = GitVcs::open_or_init(root).map_err(|e| e.to_string())?;
    Ok(Engine::new(store, InMemoryIndex::default(), vcs))
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
        Command::Search { query } => {
            if let QueryResponse::Paths { paths } =
                dispatch_query(&engine, &WireQuery::Search { query }).map_err(|e| e.to_string())?
            {
                for p in paths {
                    println!("{p}");
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
