mod commands;

use std::process;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "tuck", version, about = "Archive files to an external drive and restore them later")]
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
        /// Subfolder on the drive to use as root for tuck data
        #[arg(long)]
        prefix: Option<String>,
        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
        /// Skip confirmation prompt
        #[arg(long)]
        no_confirm: bool,
        /// Keep the local copy after archiving
        #[arg(long)]
        keep_local: bool,
        /// Replace existing archive if already archived
        #[arg(long)]
        force: bool,
    },
    /// Restore an archived file or folder to its original location
    Restore {
        /// Original path of the archived file or folder
        path: String,
        /// Name of the external drive (auto-detected if only one)
        #[arg(long)]
        drive: Option<String>,
        /// Subfolder on the drive to use as root for tuck data
        #[arg(long)]
        prefix: Option<String>,
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
        /// Subfolder on the drive to use as root for tuck data
        #[arg(long)]
        prefix: Option<String>,
    },
    /// Check if a path is archived
    Status {
        /// Path to check
        path: String,
        /// Name of the external drive (auto-detected if only one)
        #[arg(long)]
        drive: Option<String>,
        /// Subfolder on the drive to use as root for tuck data
        #[arg(long)]
        prefix: Option<String>,
    },
    /// Verify checksums of all archived files on a drive
    Verify {
        /// Name of the external drive (auto-detected if only one)
        #[arg(long)]
        drive: Option<String>,
        /// Subfolder on the drive to use as root for tuck data
        #[arg(long)]
        prefix: Option<String>,
    },
    /// Update tuck to the latest version
    Update {
        /// Only check for updates, don't install
        #[arg(long)]
        check: bool,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// View or set default configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Show current configuration
    Show,
    /// Set default prefix
    SetPrefix {
        /// Prefix value (use "" to clear)
        value: String,
    },
    /// Set default drive
    SetDrive {
        /// Drive name (use "" to clear)
        value: String,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Add {
            path,
            drive,
            prefix,
            dry_run,
            no_confirm,
            keep_local,
            force,
        } => commands::add::run(&path, drive.as_deref(), prefix.as_deref(), dry_run, no_confirm, keep_local, force),
        Commands::Restore {
            path,
            drive,
            prefix,
            dry_run,
            force,
            keep_archive,
        } => commands::restore::run(&path, drive.as_deref(), prefix.as_deref(), dry_run, force, keep_archive),
        Commands::List { drive, prefix } => commands::list::run(drive.as_deref(), prefix.as_deref()),
        Commands::Status { path, drive, prefix } => commands::status::run(&path, drive.as_deref(), prefix.as_deref()),
        Commands::Verify { drive, prefix } => commands::verify::run(drive.as_deref(), prefix.as_deref()),
        Commands::Update { check, force } => commands::update::run(check, force),
        Commands::Config { action } => commands::config::run(action),
    };

    if let Err(e) = result {
        eprintln!("{}", e);
        process::exit(e.exit_code());
    }
}
