use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::*;
use std::path::PathBuf;

mod crdt;
mod storage;
mod context;
mod watcher;
mod sync;
mod server;

#[derive(Parser)]
#[command(name = "forge")]
#[command(about = "Operation-level version control with CRDT", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
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
    },

    /// Query the operation log
    Log {
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

    match cli.command {
        Commands::Init { path } => {
            println!("{}", "🚀 Initializing Forge DeltaDB repository...".cyan().bold());
            storage::init(&path).await?;
            println!("{}", "✓ Repository initialized successfully!".green());
            println!("\n{}", "Next steps:".yellow());
            println!("  1. {} - Start tracking operations", "forge watch".bright_white());
            println!("  2. {} - View operation log", "forge log".bright_white());
            println!("  3. {} - Add context to code", "forge annotate <file> <line> -m \"message\"".bright_white());
        }

        Commands::Watch { path, sync } => {
            println!("{}", "👁  Starting operation-level tracking...".cyan().bold());
            watcher::watch(path, sync).await?;
        }

        Commands::Log { file, limit } => {
            storage::show_log(file, limit.unwrap_or(50)).await?;
        }

        Commands::Anchor { file, line, column, message } => {
            let anchor_id = context::create_anchor(&file, line, column, message).await?;
            println!("{} Created anchor: {}", "✓".green(), anchor_id.to_string().bright_yellow());
            println!("  Permalink: {}", format!("forge://{}#L{}:C{}", file.display(), line, column).bright_blue());
        }

        Commands::Annotate { file, line, message, ai } => {
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

        Commands::Serve { port, path } => {
            println!("{}", format!("🌐 Starting server on port {}...", port).cyan().bold());
            server::start(port, path).await?;
        }

        Commands::TimeTravel { file, timestamp } => {
            storage::time_travel(&file, timestamp).await?;
        }
    }

    Ok(())
}