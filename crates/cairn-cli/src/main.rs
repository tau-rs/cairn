//! The `cairn` CLI: an in-process consumer of the engine.

use std::path::Path;
use std::process::ExitCode;

use cairn_app::{Engine, Event};
use cairn_domain::NotePath;
use cairn_infra::{GitVcs, InMemoryIndex, LocalFsStore};
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
    if !matches!(cli.command, Command::Init) && !root.join(".git").is_dir() {
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
            let p = NotePath::new(&path).map_err(|e| e.to_string())?;
            engine
                .write_note(&p, &contents, &mut events)
                .map_err(|e| e.to_string())?;
            println!("wrote {path}");
        }
        Command::Read { path } => {
            let p = NotePath::new(&path).map_err(|e| e.to_string())?;
            print!("{}", engine.read_note(&p).map_err(|e| e.to_string())?);
        }
        Command::Search { query } => {
            for hit in engine.search(&query).map_err(|e| e.to_string())? {
                println!("{}", hit.path.as_str());
            }
        }
        Command::Backlinks { path } => {
            let p = NotePath::new(&path).map_err(|e| e.to_string())?;
            for b in engine.backlinks(&p).map_err(|e| e.to_string())? {
                println!("{}", b.as_str());
            }
        }
        Command::Commit { message } => {
            let id = engine
                .commit(&message, &mut events)
                .map_err(|e| e.to_string())?;
            println!("committed {id}");
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
