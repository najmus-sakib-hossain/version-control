use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::*;
use std::path::PathBuf;

mod context;
mod crdt;
mod server;
mod storage;
mod sync;
mod watcher;

#[derive(Parser)]
#[command(name = "forge")]
#[command(about = "Next-generation version control with operation-level tracking, CRDT-based sync, and seamless Git integration", version)]
#[command(after_help = "Forge Features:
- Operation-level version control with CRDT for conflict-free collaboration
- Real-time sync between multiple peers via WebSocket
- Character-level anchors and permalinks for precise code references
- AI-powered code annotations and context exploration
- Git repository synchronization and integration
- Collaborative server for multi-user editing
- Time-travel debugging to view file states at any timestamp
- Comprehensive operation logging and querying
- Seamless Git command support without 'git' prefix

All Git commands are supported without the 'git' prefix. Use 'forge <git-command>' instead of 'git <git-command>'.

Main Porcelain Commands:
   add, am, archive, backfill, bisect, branch, bundle, checkout, cherry-pick, citool, clean, clone, commit, describe, diff, fetch, format-patch, gc, gitk, grep, gui, init, log, maintenance, merge, mv, notes, pull, push, range-diff, rebase, reset, restore, revert, rm, scalar, shortlog, show, sparse-checkout, stash, status, submodule, survey, switch, tag, worktree

Ancillary Commands / Manipulators:
   config, fast-export, fast-import, filter-branch, mergetool, pack-refs, prune, reflog, refs, remote, repack, replace

Ancillary Commands / Interrogators:
   annotate, blame, bugreport, count-objects, diagnose, difftool, fsck, gitweb, help, instaweb, merge-tree, rerere, show-branch, verify-commit, verify-tag, version, whatchanged

Interacting with Others:
   archimport, cvsexportcommit, cvsimport, cvsserver, imap-send, p4, quiltimport, request-pull, send-email, svn

Low-level Commands / Manipulators:
   apply, checkout-index, commit-graph, commit-tree, hash-object, index-pack, merge-file, merge-index, mktag, mktree, multi-pack-index, pack-objects, prune-packed, read-tree, replay, symbolic-ref, unpack-objects, update-index, update-ref, write-tree

Low-level Commands / Interrogators:
   cat-file, cherry, diff-files, diff-index, diff-pairs, diff-tree, for-each-ref, for-each-repo, get-tar-commit-id, ls-files, ls-remote, ls-tree, merge-base, name-rev, pack-redundant, rev-list, rev-parse, show-index, show-ref, unpack-file, var, verify-pack

Low-level Commands / Syncing Repositories:
   daemon, fetch-pack, http-backend, send-pack, update-server-info

Low-level Commands / Internal Helpers:
   check-attr, check-ignore, check-mailmap, check-ref-format, column, credential, credential-cache, credential-store, fmt-merge-msg, hook, interpret-trailers, mailinfo, mailsplit, merge-one-file, patch-id, sh-i18n, sh-setup, stripspace

External commands:
   askpass, askyesno, credential-helper-selector, credential-manager, flow, lfs, update-git-for-windows")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new Forge repository
    Init {
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },

    /// Watch for changes and track operations
    Watch {
        #[arg(short, long, default_value = ".")]
        path: PathBuf,

        /// Enable real-time sync
        #[arg(long)]
        sync: bool,

        /// WebSocket peer(s) to connect, e.g. ws://localhost:3000/ws
        #[arg(long, value_name = "URL")]
        peer: Vec<String>,
    },

    /// Query the operation log
    OpLog {
        #[arg(short, long)]
        file: Option<PathBuf>,

        #[arg(short, long)]
        limit: Option<usize>,
    },

    /// Create a character-level anchor/permalink
    Anchor {
        file: PathBuf,
        line: usize,
        column: usize,

        #[arg(short, long)]
        message: Option<String>,
    },

    /// Annotate code with context
    Annotate {
        file: PathBuf,
        line: usize,

        #[arg(short, long)]
        message: String,

        #[arg(long)]
        ai: bool,
    },

    /// Show annotations and context for a file
    Context {
        file: PathBuf,

        #[arg(short, long)]
        line: Option<usize>,
    },

    /// Sync with Git repository
    GitSync {
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },

    /// Any unrecognized subcommand will be passed to the system `git`.
    #[command(external_subcommand)]
    GitPassthrough(Vec<String>),

    /// Start collaborative server
    Serve {
        #[arg(short, long, default_value = "3000")]
        port: u16,

        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },

    /// Show time-travel view of a file
    TimeTravel {
        file: PathBuf,

        #[arg(short, long)]
        timestamp: Option<String>,
    },
}

#[tokio::main(flavor = "multi_thread", worker_threads = 10)]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let command = match cli.command {
        Some(cmd) => cmd,
        None => Commands::Watch {
            path: ".".into(),
            sync: false,
            peer: vec![],
        },
    };

    match command {
        Commands::Init { path } => {
            println!(
                "{}",
                "🚀 Initializing Forge DeltaDB repository...".cyan().bold()
            );
            storage::init(&path).await?;
            println!("{}", "✓ Repository initialized successfully!".green());
            println!("\n{}", "Next steps:".yellow());
            println!(
                "  1. {} - Start tracking operations",
                "forge watch".bright_white()
            );
            println!("  2. {} - View operation log", "forge oplog".bright_white());
            println!(
                "  3. {} - Add context to code",
                "forge annotate <file> <line> -m \"message\"".bright_white()
            );
        }

        Commands::Watch { path, sync, peer } => {
            println!(
                "{}",
                "👁  Starting operation-level tracking...".cyan().bold()
            );
            watcher::watch(path, sync, peer).await?;
        }

        Commands::OpLog { file, limit } => {
            storage::show_log(file, limit.unwrap_or(50)).await?;
        }

        Commands::Anchor {
            file,
            line,
            column,
            message,
        } => {
            let anchor = context::create_anchor(&file, line, column, message).await?;
            println!(
                "{} Created anchor: {}",
                "✓".green(),
                anchor.id.to_string().bright_yellow()
            );
            println!("  Permalink: {}", anchor.permalink().bright_blue());
        }

        Commands::Annotate {
            file,
            line,
            message,
            ai,
        } => {
            context::annotate(&file, line, &message, ai).await?;
            println!("{} Annotation added", "✓".green());
        }

        Commands::Context { file, line } => {
            context::show_context(&file, line).await?;
        }

        Commands::GitSync { path } => {
            println!("{}", "🔄 Syncing with Git...".cyan().bold());
            storage::git_sync(&path).await?;
            println!("{}", "✓ Sync complete".green());
        }

        Commands::GitPassthrough(args) => {
            use tokio::process::Command;
            let status = if args.is_empty() {
                Command::new("git").status().await?
            } else {
                Command::new("git").args(args).status().await?
            };
            if !status.success() {
                eprintln!("git exited with status: {}", status);
            }
        }

        Commands::Serve { port, path } => {
            println!(
                "{}",
                format!("🌐 Starting server on port {}...", port)
                    .cyan()
                    .bold()
            );
            server::start(port, path).await?;
        }

        Commands::TimeTravel { file, timestamp } => {
            storage::time_travel(&file, timestamp).await?;
        }
    }

    Ok(())
}
