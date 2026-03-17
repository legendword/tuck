mod commands;

use std::process;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "tuck", about = "Archive files to an external drive and restore them later")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Archive a file or folder to an external drive
    Add {
        /// Path to the file or folder to archive
        path: String,
        /// Name of the external drive (auto-detected if only one)
        #[arg(long)]
        drive: Option<String>,
        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
        /// Skip confirmation prompt
        #[arg(long)]
        no_confirm: bool,
        /// Keep the local copy after archiving
        #[arg(long)]
        keep_local: bool,
    },
    /// Restore an archived file or folder to its original location
    Restore {
        /// Original path of the archived file or folder
        path: String,
        /// Name of the external drive (auto-detected if only one)
        #[arg(long)]
        drive: Option<String>,
        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
        /// Overwrite existing local files
        #[arg(long)]
        force: bool,
        /// Keep the archive copy on the drive after restoring
        #[arg(long)]
        keep_archive: bool,
    },
    /// List all archived entries on a drive
    List {
        /// Name of the external drive (auto-detected if only one)
        #[arg(long)]
        drive: Option<String>,
    },
    /// Check if a path is archived
    Status {
        /// Path to check
        path: String,
    },
    /// Verify checksums of all archived files on a drive
    Verify {
        /// Name of the external drive (auto-detected if only one)
        #[arg(long)]
        drive: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Add {
            path,
            drive,
            dry_run,
            no_confirm,
            keep_local,
        } => commands::add::run(&path, drive.as_deref(), dry_run, no_confirm, keep_local),
        Commands::Restore {
            path,
            drive,
            dry_run,
            force,
            keep_archive,
        } => commands::restore::run(&path, drive.as_deref(), dry_run, force, keep_archive),
        Commands::List { drive } => commands::list::run(drive.as_deref()),
        Commands::Status { path } => commands::status::run(&path),
        Commands::Verify { drive } => commands::verify::run(drive.as_deref()),
    };

    if let Err(e) = result {
        eprintln!("{}", e);
        process::exit(e.exit_code());
    }
}
