use anyhow::{Context, Result};
use clap::Subcommand;
use sonus_gui::{SonusApp, ThemeManager};

/// Subcommands for managing Sonus' downloaded tools.
#[derive(Subcommand)]
pub enum DepsCommand {
    /// Show where each tool is, and whether it's the bundled or system one
    Status,

    /// Download any missing tools into Sonus' data dir
    Install,

    /// Force re-download of the latest tools
    Update,
}

/// Launch the GUI — a frameless, always-on-top window in the style of the
/// classic desktop players.
pub fn gui() -> Result<()> {
    // The tokio runtime lives on its own thread so it can never block the
    // render loop. yt-dlp calls are spawned onto it and polled from the GUI.
    let handle = spawn_runtime()?;

    // A failure to open the library shouldn't stop the player from working —
    // search and playback are still useful without saved playlists.
    let db = match open_database() {
        Ok(db) => Some(db),
        Err(e) => {
            eprintln!("[sonus] library unavailable, continuing without it: {e:#}");
            None
        }
    };

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([440.0, 620.0])
        .with_min_inner_size([380.0, 480.0])
        .with_title("Sonus")
        .with_app_id("sonus")
        // Always on top and undecorated: the app draws its own title bar.
        .with_always_on_top()
        .with_decorations(false)
        .with_resizable(true);

    if let Some(icon) = sonus_gui::window_icon() {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "Sonus",
        options,
        Box::new(move |cc| Ok(Box::new(SonusApp::new(cc, handle, db)))),
    )
    .map_err(|e| anyhow::anyhow!("could not start the GUI: {e}"))
}

/// Build a multi-threaded tokio runtime on a dedicated thread and hand back a
/// handle to it.
fn spawn_runtime() -> Result<tokio::runtime::Handle> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("sonus-async".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(e) => {
                    let _ = tx.send(Err(e.to_string()));
                    return;
                }
            };
            let _ = tx.send(Ok(runtime.handle().clone()));
            // Park forever; dropping the runtime would kill in-flight tasks.
            runtime.block_on(std::future::pending::<()>());
        })
        .context("could not spawn the async runtime thread")?;

    rx.recv()
        .context("async runtime thread died during startup")?
        .map_err(|e| anyhow::anyhow!("could not build the async runtime: {e}"))
}

fn open_database() -> Result<sonus_library::Database> {
    let path = sonus_library::Database::default_path();
    sonus_library::Database::open(&path)
        .with_context(|| format!("could not open {}", path.display()))
}

/// Search from the command line and print the results.
///
/// The flat listing carries titles only — no artist, no duration. The GUI hides
/// that with a background enrichment pass, but the CLI has nothing to hide
/// behind, so it enriches up front. It costs a couple of seconds and is the
/// difference between a useful listing and a column of "Unknown Artist".
pub async fn search_cli(query: &str, limit: usize) -> Result<()> {
    let results = sonus_ytdlp::search(query, limit)
        .await
        .context("search failed")?;

    if results.is_empty() {
        println!("No results for '{query}'.");
        return Ok(());
    }

    // Enrichment is best-effort: if yt-dlp can't be reached a second time, the
    // flat titles are still worth printing.
    let ids: Vec<String> = results.iter().map(|r| r.youtube_id.clone()).collect();
    let enriched = sonus_ytdlp::enrich_tracks(&ids).await.unwrap_or_default();

    println!("{} results for '{query}':\n", results.len());
    for (index, result) in results.iter().enumerate() {
        let details = enriched.iter().find(|t| t.youtube_id == result.youtube_id);

        let duration = result
            .duration_secs
            .or_else(|| details.and_then(|t| t.duration_secs))
            .map(|s| format!("{}:{:02}", s / 60, s % 60))
            .unwrap_or_else(|| "--:--".to_string());

        let artist = result
            .artist
            .clone()
            .or_else(|| details.and_then(|t| t.artist.clone()))
            .unwrap_or_else(|| "Unknown Artist".to_string());

        println!(
            "  {:>2}. {} - {} [{}]",
            index + 1,
            result.title,
            artist,
            duration
        );
    }
    println!("\nPlay it with:  sonus   (then search for '{query}')");
    Ok(())
}

/// Print the tracks YouTube recommends after a given track.
pub async fn radio_cli(youtube_id: &str, limit: usize) -> Result<()> {
    let tracks = sonus_ytdlp::radio_for(youtube_id, limit)
        .await
        .context("could not fetch recommendations")?;

    if tracks.is_empty() {
        println!("No recommendations for '{youtube_id}'.");
        return Ok(());
    }

    println!("{} recommendations:\n", tracks.len());
    for (index, track) in tracks.iter().enumerate() {
        println!("  {:>2}. {}", index + 1, track.label());
    }
    Ok(())
}

/// List every available theme.
pub fn list_themes() -> Result<()> {
    let manager = ThemeManager::load();

    println!("Available themes:\n");
    for (index, theme) in manager.themes().iter().enumerate() {
        let marker = if index == manager.current_index() {
            "*"
        } else {
            " "
        };
        println!("  {marker} {}", theme.name);
    }

    println!("\n* = currently active");

    if !manager.warnings().is_empty() {
        println!("\nProblems loading custom themes:");
        for warning in manager.warnings() {
            println!("  - {warning}");
        }
    }

    println!("\nCustom themes: {}", sonus_gui::themes_dir().display());
    println!("Drop a .toml file there to add one.");
    Ok(())
}

/// Print the platform-specific paths Sonus uses, and check the external tools.
pub fn print_paths() -> Result<()> {
    println!(
        "Library:     {}",
        sonus_library::Database::default_path().display()
    );
    println!(
        "Audio cache: {}",
        sonus_cache::AudioCache::default_dir().display()
    );
    println!("Themes:      {}", sonus_gui::themes_dir().display());

    println!("Binaries:    {}", sonus_deps::bin_dir().display());

    let problems = sonus_ytdlp::check_dependencies();
    if problems.is_empty() {
        println!("\nyt-dlp and ffmpeg: found");
    } else {
        println!("\nMissing dependencies:");
        for problem in problems {
            println!("  - {problem}");
        }
    }

    Ok(())
}

/// Print where each tool comes from and its version.
fn describe(dep: sonus_deps::Dependency) -> String {
    use sonus_deps::ResolvedBinary;

    match match dep {
        sonus_deps::Dependency::YtDlp => sonus_deps::resolve_ytdlp(),
        sonus_deps::Dependency::Ffmpeg => sonus_deps::resolve_ffmpeg(),
    } {
        ResolvedBinary::Found(path) => format!("{}    {} (system)", dep.name(), path.display()),
        ResolvedBinary::Managed(path, version) => {
            format!(
                "{}    {} (bundled v{})",
                dep.name(),
                path.display(),
                version
            )
        }
        ResolvedBinary::Missing => format!("{}    MISSING", dep.name()),
    }
}

/// `sonus deps` subcommands.
pub fn deps(cmd: DepsCommand) -> Result<()> {
    use sonus_deps::Dependency;

    match cmd {
        DepsCommand::Status => {
            println!("Tool    Path (source)\n");
            for dep in [Dependency::YtDlp, Dependency::Ffmpeg] {
                println!("{}", describe(dep));
            }
            println!("\nRun `sonus deps install` to fetch any missing tools.");

            // Staleness is a network call, so only report it when something
            // is actually installed; a totally fresh machine shouldn't block.
            let stale: Vec<String> = [Dependency::YtDlp, Dependency::Ffmpeg]
                .into_iter()
                .filter_map(|dep| match sonus_deps::check_stale(dep) {
                    sonus_deps::StaleStatus::Stale { current, latest } => Some(format!(
                        "  {} v{current} — v{latest} available. Run `sonus deps update`.",
                        dep.name()
                    )),
                    _ => None,
                })
                .collect();
            if !stale.is_empty() {
                println!("\nUpdates available:");
                for line in stale {
                    println!("{line}");
                }
            }
        }

        DepsCommand::Install => {
            for dep in [Dependency::YtDlp, Dependency::Ffmpeg] {
                if !sonus_deps::needs_install(dep) {
                    println!("{}: already available", dep.name());
                    continue;
                }
                print!("Installing {}... ", dep.name());
                // Progress isn't useful on one line; just wait for completion.
                let mut on_progress = |_: f32| {};
                match sonus_deps::install(dep, &mut on_progress) {
                    Ok(outcome) => {
                        println!("done (v{})", outcome.version);
                    }
                    Err(e) => {
                        eprintln!("failed: {e}");
                        std::process::exit(1);
                    }
                }
            }
        }

        DepsCommand::Update => {
            for dep in [Dependency::YtDlp, Dependency::Ffmpeg] {
                print!("Updating {}... ", dep.name());
                let mut on_progress = |_: f32| {};
                match sonus_deps::update(dep, &mut on_progress) {
                    Ok(outcome) => {
                        println!("done (v{})", outcome.version);
                    }
                    Err(e) => {
                        eprintln!("failed: {e}");
                        std::process::exit(1);
                    }
                }
            }
        }
    }

    Ok(())
}
