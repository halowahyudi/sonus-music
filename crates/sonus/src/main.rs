mod run;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "sonus")]
#[command(about = "Free, local-first music player powered by yt-dlp")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Launch the player window (default when no command is given)
    Play,

    /// Search YouTube Music and print the results
    Search {
        query: String,
        /// How many results to fetch
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
    },

    /// Print the tracks YouTube recommends after a given one
    Radio {
        /// A YouTube video ID (the 11-character part of the watch URL)
        youtube_id: String,
        /// How many recommendations to fetch
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
    },

    /// List the built-in and custom themes
    Themes,

    /// Show where Sonus keeps its files, and check for yt-dlp and ffmpeg
    Paths,

    /// Manage the yt-dlp and ffmpeg binaries Sonus downloads
    #[command(subcommand)]
    Deps(run::DepsCommand),
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command.unwrap_or(Commands::Play) {
        Commands::Play => run::gui()?,

        Commands::Search { query, limit } => {
            // Searching shells out to yt-dlp, which needs an async runtime.
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(run::search_cli(&query, limit))?;
        }

        Commands::Radio { youtube_id, limit } => {
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(run::radio_cli(&youtube_id, limit))?;
        }

        Commands::Themes => run::list_themes()?,
        Commands::Paths => run::print_paths()?,
        Commands::Deps(cmd) => run::deps(cmd)?,
    }

    Ok(())
}
