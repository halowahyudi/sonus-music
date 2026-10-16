//! Application state and the playback orchestration that ties it together.
//!
//! The central idea: the GUI owns the [`Queue`] and decides *what* plays next;
//! the audio thread only knows how to play one track at a time. That split
//! keeps the non-`Send` engine isolated and makes the interesting logic —
//! auto-advance, radio extension, repeat — plain testable Rust.

use std::sync::Arc;

use sonus_core::{Playlist, Queue, QueueSource, RepeatMode, Track};
use sonus_library::{Database, Favorites, History, Playlists, Settings, Tracks};
use sonus_ytdlp::{
    check_dependencies, enrich_tracks, extend_queue, fetch_metadata, search, SearchResult,
};

use crate::audio::{AudioController, Transport};
use crate::pixel::Spectrum;
use crate::task::Pending;
use crate::theme::ThemeManager;

/// Which screen the user is looking at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    /// Search box and results.
    #[default]
    Search,
    /// The playback queue ("up next").
    Queue,
    /// Saved playlists.
    Playlists,
    /// Liked tracks and history.
    Library,
}

impl View {
    pub const ALL: [View; 4] = [View::Search, View::Queue, View::Playlists, View::Library];

    pub fn label(self) -> &'static str {
        match self {
            View::Search => "SEARCH",
            View::Queue => "QUEUE",
            View::Playlists => "LISTS",
            View::Library => "LIBRARY",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            View::Search => "?",
            View::Queue => ">",
            View::Playlists => "#",
            View::Library => "*",
        }
    }
}

/// A transient message shown in the status bar.
#[derive(Debug, Clone)]
pub struct Notice {
    pub text: String,
    pub is_error: bool,
    /// Seconds remaining before it fades.
    pub ttl: f32,
}

impl Notice {
    pub fn info(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: false,
            ttl: 4.0,
        }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            is_error: true,
            // Errors linger — they're usually worth reading.
            ttl: 8.0,
        }
    }
}

/// The playback queue plus the radio behaviour around it.
pub struct Player {
    pub queue: Queue,
    /// When true, running off the end of the queue pulls in more tracks from
    /// YouTube's recommendation radio.
    pub radio_enabled: bool,
    /// Set while a radio fetch is in flight.
    pub radio_pending: Pending<Result<Vec<Track>, String>>,
    /// The track the last radio fetch was seeded from.
    pub radio_seed: Option<String>,
    /// Set when playback ran out of queue and is waiting on a radio fetch to
    /// carry on. This is what distinguishes "the queue ended" from "the user
    /// pressed stop" — without it, a fetch landing after a manual stop would
    /// surprise the user by starting playback again.
    pub awaiting_radio: bool,
    /// Volume, mirrored here so the UI can render it before the audio thread
    /// acknowledges the change.
    pub volume: f32,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            queue: Queue::new(),
            radio_enabled: true,
            radio_pending: Pending::default(),
            radio_seed: None,
            awaiting_radio: false,
            volume: 0.7,
        }
    }
}

impl Player {
    pub fn current(&self) -> Option<&Track> {
        self.queue.current()
    }

    pub fn is_playing(&self, youtube_id: &str) -> bool {
        self.queue
            .current()
            .is_some_and(|t| t.youtube_id == youtube_id)
    }

    /// The track to seed recommendations from.
    ///
    /// The *newest* track in the queue, not the one currently playing. Radio
    /// for a mid-queue track mostly returns songs already queued, so seeding
    /// from the frontier is what actually grows the queue — seeding from the
    /// current track is why playback used to stall at the end of a list.
    pub fn radio_seed_track(&self) -> Option<&Track> {
        self.queue.items().last().map(|item| &item.track)
    }

    /// Should we pull more tracks when the queue runs out?
    pub fn should_extend(&self) -> bool {
        self.radio_enabled
            && self.queue.source().extends_itself()
            && self.queue.remaining() <= 2
            && !self.radio_pending.is_running()
    }
}

/// One dependency (yt-dlp or ffmpeg) being installed by the setup overlay.
#[derive(Debug, Clone)]
pub struct SetupEntry {
    pub dep: sonus_deps::Dependency,
    /// 0.0..=1.0 download progress, or -1 while unknown.
    pub progress: f32,
    /// Message while this entry is installing (e.g. "downloading").
    pub label: String,
    /// Set when this tool finished installing.
    pub done: bool,
    /// Set when installation failed; the overlay shows the reason.
    pub error: Option<String>,
}

/// First-run setup: downloads yt-dlp and ffmpeg into Sonus' data dir.
///
/// The GUI can't run if these are missing, so this blocks the whole window
/// until either everything is installed or the user quits. The actual download
/// runs on a background thread (it's blocking I/O, not async-friendly work),
/// and `poll` reads its progress via a shared mutex once per frame.
pub struct SetupState {
    entries: Vec<SetupEntry>,
    /// Shared with the installer thread so progress can be read without a
    /// channel round-trip.
    shared: Arc<std::sync::Mutex<Vec<SetupEntry>>>,
    /// The background thread, kept alive for the duration of the install.
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Default for SetupState {
    fn default() -> Self {
        Self::new()
    }
}

impl SetupState {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            shared: Arc::new(std::sync::Mutex::new(Vec::new())),
            worker: None,
        }
    }

    /// True when setup still has work to do.
    pub fn is_running(&self) -> bool {
        !self.entries.is_empty() && self.entries.iter().any(|e| !e.done && e.error.is_none())
    }

    pub fn has_failed(&self) -> bool {
        self.entries.iter().any(|e| e.error.is_some())
    }

    /// Refresh `entries` from the shared state the installer thread writes.
    pub fn poll(&mut self) {
        if let Ok(shared) = self.shared.lock() {
            self.entries = shared.clone();
        }
    }

    /// Snapshot of the current per-tool install state, for the overlay to draw.
    pub fn entries(&self) -> Vec<SetupEntry> {
        self.entries.clone()
    }

    /// A human-friendly summary of what still needs installing.
    pub fn pending_label(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|e| !e.done && e.error.is_none())
            .map(|e| e.label.clone())
            .collect()
    }
}

/// Everything the UI needs.
pub struct SonusApp {
    pub audio: AudioController,
    pub themes: ThemeManager,
    pub db: Option<Database>,
    pub tokio: tokio::runtime::Handle,

    // ---- navigation -----------------------------------------------------
    pub view: View,
    pub show_themes: bool,
    pub show_eq: bool,
    pub show_help: bool,

    // ---- search ---------------------------------------------------------
    pub search_input: String,
    pub search_results: Vec<SearchResult>,
    pub search_pending: Pending<Result<Vec<SearchResult>, String>>,
    /// Background metadata pass that fills in artists the flat listing omits.
    pub enrich_pending: Pending<Vec<Track>>,
    /// Which result the keyboard cursor is on.
    pub search_cursor: Option<usize>,
    pub focus_search: bool,

    // ---- playback -------------------------------------------------------
    pub player: Player,
    pub spectrum: Spectrum,
    /// Position within the track, smoothed between audio-thread updates.
    pub position_secs: f32,
    /// Set when the user is dragging the seek bar.
    pub scrubbing: Option<f32>,

    // ---- library --------------------------------------------------------
    pub playlists: Vec<(String, String, usize)>,
    pub open_playlist: Option<Playlist>,
    pub liked_ids: Vec<String>,
    pub recent: Vec<Track>,
    pub new_playlist_name: String,
    pub renaming_playlist: Option<(String, String)>,

    // ---- async ----------------------------------------------------------
    pub metadata_pending: Pending<Result<Track, String>>,
    /// Background staleness check for the managed tools.
    pub stale_pending: Pending<Vec<String>>,

    // ---- feedback -------------------------------------------------------
    pub notices: Vec<Notice>,
    /// Problems detected at startup (missing yt-dlp/ffmpeg).
    pub dependency_problems: Vec<String>,
    /// First-run dependency installer state (yt-dlp/ffmpeg download).
    pub setup: SetupState,
    /// Errors from the audio thread already shown, so we don't repeat them.
    pub last_audio_error: Option<String>,

    // ---- animation ------------------------------------------------------
    /// Seconds since the app started; drives marquee and blink effects.
    pub elapsed: f32,
}

impl SonusApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        tokio: tokio::runtime::Handle,
        db: Option<Database>,
    ) -> Self {
        crate::pixel::font::install(&cc.egui_ctx);
        crate::pixel::font::install_crisp_tessellation(&cc.egui_ctx);

        let themes = ThemeManager::load();
        themes.palette().apply(&cc.egui_ctx);

        let audio = AudioController::spawn();
        let volume = db
            .as_ref()
            .and_then(|db| Settings::get_f32(db, sonus_library::setting_keys::VOLUME, 0.7).ok())
            .unwrap_or(0.7);
        audio.set_volume(volume);

        let mut app = Self {
            audio,
            themes,
            db,
            tokio,

            view: View::default(),
            show_themes: false,
            show_eq: false,
            show_help: false,

            search_input: String::new(),
            search_results: Vec::new(),
            search_pending: Pending::default(),
            enrich_pending: Pending::default(),
            search_cursor: None,
            focus_search: true,

            player: Player {
                volume,
                ..Default::default()
            },
            spectrum: Spectrum::new(sonus_audio::NUM_BARS, crate::pixel::spectrum::Style::Bars),
            position_secs: 0.0,
            scrubbing: None,

            playlists: Vec::new(),
            open_playlist: None,
            liked_ids: Vec::new(),
            recent: Vec::new(),
            new_playlist_name: String::new(),
            renaming_playlist: None,

            metadata_pending: Pending::default(),
            stale_pending: Pending::default(),

            notices: Vec::new(),
            dependency_problems: check_dependencies(),
            setup: SetupState::new(),
            last_audio_error: None,

            elapsed: 0.0,
        };

        // Kick off the update check on the async runtime; it must never block
        // startup, so it runs in the background and surfaces as a notice.
        app.spawn_stale_check();

        app.restore_settings();
        app.refresh_library();
        app
    }

    // -----------------------------------------------------------------------
    // First-run dependency setup
    // -----------------------------------------------------------------------

    /// True when yt-dlp or ffmpeg is missing and setup hasn't cleared it yet.
    pub fn needs_dependency_setup(&self) -> bool {
        !self.dependency_problems.is_empty()
    }

    /// Start installing the missing tools in the background.
    ///
    /// The download is blocking I/O, so it runs on a plain thread and reports
    /// progress through a shared mutex that `poll_setup` reads each frame.
    pub fn start_dependency_setup(&mut self) {
        let deps: Vec<sonus_deps::Dependency> = [
            sonus_deps::Dependency::YtDlp,
            sonus_deps::Dependency::Ffmpeg,
        ]
        .into_iter()
        .filter(|dep| sonus_deps::needs_install(*dep))
        .collect();
        if deps.is_empty() {
            // Nothing missing — clear the problems and carry on.
            self.dependency_problems.clear();
            return;
        }

        let entries: Vec<SetupEntry> = deps
            .iter()
            .map(|dep| SetupEntry {
                dep: *dep,
                progress: 0.0,
                label: format!("{}: waiting", dep.name()),
                done: false,
                error: None,
            })
            .collect();
        self.setup.entries = entries.clone();
        *self.setup.shared.lock().unwrap() = entries.clone();

        let shared = self.setup.shared.clone();
        self.setup.worker = Some(std::thread::spawn(move || {
            // Install sequentially: yt-dlp first (smaller, fails fast), then
            // ffmpeg (the big one).
            for entry in &mut *shared.lock().unwrap() {
                let mut on_progress = |p: f32| {
                    if let Ok(mut guard) = shared.lock() {
                        if let Some(e) = guard.iter_mut().find(|e| e.dep == entry.dep) {
                            e.progress = p;
                            e.label = format!("{}: {:.0}%", entry.dep.name(), p * 100.0);
                        }
                    }
                };
                let result = sonus_deps::install(entry.dep, &mut on_progress);
                let mut guard = shared.lock().unwrap();
                if let Some(e) = guard.iter_mut().find(|e| e.dep == entry.dep) {
                    match result {
                        Ok(outcome) => {
                            e.done = true;
                            e.progress = 1.0;
                            e.label =
                                format!("{} installed (v{})", entry.dep.name(), outcome.version);
                        }
                        Err(err) => {
                            e.error = Some(err.to_string());
                            e.label = format!("{} failed", entry.dep.name());
                        }
                    }
                }
            }
        }));
    }

    /// Refresh setup progress from the installer thread.
    pub fn poll_setup(&mut self) {
        self.setup.poll();

        // Re-resolve once the thread has finished its work; when everything's
        // in place the overlay disappears on its own.
        if self.setup.worker.as_ref().is_some_and(|w| w.is_finished()) {
            self.setup.worker = None;
            if self.setup.has_failed() {
                // Leave the problems in place so the overlay keeps showing the
                // error; the user can retry.
            } else if self.setup.entries.iter().all(|e| e.done) {
                self.dependency_problems.clear();
            }
        }
    }

    // -----------------------------------------------------------------------
    // Update check
    // -----------------------------------------------------------------------

    /// Kick off the managed-tool update check on the async runtime.
    ///
    /// Only bundled tools are checked — a system yt-dlp is the distro's
    /// business. The check is throttled inside `sonus-deps` to once a day, so
    /// this usually resolves instantly without touching the network.
    fn spawn_stale_check(&mut self) {
        if self.dependency_problems.is_empty() {
            let handle = self.tokio.clone();
            self.stale_pending = Pending::start(&handle, async move {
                let mut messages = Vec::new();
                for dep in [
                    sonus_deps::Dependency::YtDlp,
                    sonus_deps::Dependency::Ffmpeg,
                ] {
                    if let sonus_deps::StaleStatus::Stale { current, latest } =
                        sonus_deps::check_stale(dep)
                    {
                        messages.push(format!(
                            "{} v{current} — update v{latest} available. Run `sonus deps update`.",
                            dep.name()
                        ));
                    }
                }
                messages
            });
        }
    }

    /// Surface any finished update check as a notice.
    pub fn poll_stale(&mut self) {
        if let Some(messages) = self.stale_pending.poll_take() {
            for message in messages {
                self.notify(Notice::info(message));
            }
        }
    }

    /// Restore persisted preferences.
    fn restore_settings(&mut self) {
        let Some(db) = self.db.as_ref() else {
            return;
        };

        if let Ok(Some(name)) = Settings::get(db, sonus_library::setting_keys::THEME) {
            self.themes.select_by_name(&name);
        }
        if let Ok(mode) = Settings::get(db, sonus_library::setting_keys::REPEAT_MODE) {
            let repeat = match mode.as_deref() {
                Some("all") => RepeatMode::All,
                Some("one") => RepeatMode::One,
                _ => RepeatMode::Off,
            };
            self.player.queue.set_repeat(repeat);
        }
        if let Ok(enabled) =
            Settings::get_bool(db, sonus_library::setting_keys::RADIO_ENABLED, true)
        {
            self.player.radio_enabled = enabled;
        }
    }

    /// Persist a preference, ignoring failures — a full disk shouldn't stop
    /// playback.
    pub fn save_setting(&self, key: &str, value: &str) {
        if let Some(db) = self.db.as_ref() {
            let _ = Settings::set(db, key, value);
        }
    }

    fn save_setting_f32(&self, key: &str, value: f32) {
        if let Some(db) = self.db.as_ref() {
            let _ = Settings::set_f32(db, key, value);
        }
    }

    /// Reload playlists, likes and history from the database.
    pub fn refresh_library(&mut self) {
        let Some(db) = self.db.as_ref() else {
            return;
        };
        self.playlists = Playlists::all(db).unwrap_or_default();
        self.liked_ids = Favorites::ids(db).unwrap_or_default();
        self.recent = History::recent(db, 50).unwrap_or_default();
    }

    // -----------------------------------------------------------------------
    // Search
    // -----------------------------------------------------------------------

    /// Kick off a search for the current query.
    pub fn start_search(&mut self) {
        let query = self.search_input.trim().to_string();
        if query.is_empty() {
            return;
        }

        let handle = self.tokio.clone();
        self.search_pending = Pending::start(&handle, async move {
            search(&query, 30).await.map_err(|e| e.to_string())
        });
        self.search_cursor = None;
    }

    /// Collect a finished search, if there is one.
    ///
    /// The flat listing comes back in a couple of seconds with titles only.
    /// Rather than make the user wait ~2s per track for artist names, we show
    /// the results immediately and enrich them in one batched call.
    fn poll_search(&mut self) {
        if let Some(result) = self.search_pending.poll_take() {
            match result {
                Ok(results) => {
                    let count = results.len();
                    self.search_results = results;
                    self.search_cursor = (!self.search_results.is_empty()).then_some(0);
                    if count == 0 {
                        self.notify(Notice::info("No results found"));
                    } else {
                        self.start_enrichment();
                    }
                }
                Err(e) => {
                    self.search_results.clear();
                    self.notify(Notice::error(format!("Search failed: {e}")));
                }
            }
        }

        self.poll_enrichment();
    }

    /// Fetch artists and durations for the current results, in one batch.
    fn start_enrichment(&mut self) {
        // Only the results that are missing an artist are worth fetching.
        let ids: Vec<String> = self
            .search_results
            .iter()
            .filter(|r| r.artist.is_none() || r.duration_secs.is_none())
            .map(|r| r.youtube_id.clone())
            .collect();

        if ids.is_empty() {
            return;
        }

        let handle = self.tokio.clone();
        self.enrich_pending = Pending::start(&handle, async move {
            enrich_tracks(&ids).await.unwrap_or_default()
        });
    }

    /// Merge enriched metadata into the visible results.
    fn poll_enrichment(&mut self) {
        let Some(enriched) = self.enrich_pending.poll_take() else {
            return;
        };

        for track in &enriched {
            if let Some(result) = self
                .search_results
                .iter_mut()
                .find(|r| r.youtube_id == track.youtube_id)
            {
                if result.artist.is_none() {
                    result.artist = track.artist.clone();
                }
                if result.duration_secs.is_none() {
                    result.duration_secs = track.duration_secs;
                }
            }
        }

        if let Some(db) = self.db.as_ref() {
            let _ = Tracks::upsert_many(db, &enriched);
        }
    }

    // -----------------------------------------------------------------------
    // Playback
    // -----------------------------------------------------------------------

    /// Play a single track immediately, replacing the queue with it plus a
    /// radio tail so playback keeps going.
    pub fn play_now(&mut self, track: Track) {
        self.player.queue.load(vec![track], 0);
        self.player.queue.set_source(QueueSource::Radio);
        self.player.radio_seed = None;
        self.start_current();
        self.extend_from_radio();
    }

    /// Play a list of tracks starting at `index`.
    pub fn play_list(&mut self, tracks: Vec<Track>, index: usize, source: QueueSource) {
        if tracks.is_empty() {
            return;
        }
        self.player.queue.load(tracks, index);
        self.player.queue.set_source(source);
        self.player.radio_seed = None;
        self.start_current();

        if self.player.queue.source().extends_itself() {
            self.extend_from_radio();
        }
    }

    /// Load the queue but don't start playing — used by "add to queue".
    pub fn enqueue(&mut self, track: Track) {
        if self.player.queue.push_back(track.clone()) {
            self.notify(Notice::info(format!("Queued: {}", track.title)));
        } else {
            self.notify(Notice::info("Already in the queue"));
        }
    }

    pub fn play_next(&mut self, track: Track) {
        if self.player.queue.push_next(track.clone()) {
            self.notify(Notice::info(format!("Playing next: {}", track.title)));
        } else {
            self.notify(Notice::info("Already in the queue"));
        }
    }

    /// Start whatever the queue cursor points at.
    fn start_current(&mut self) {
        let Some(track) = self.player.queue.current().cloned() else {
            return;
        };

        self.audio.play(&track.youtube_id);
        self.position_secs = 0.0;
        self.spectrum.reset();
        self.record_play(&track);
        self.fetch_metadata(&track);
    }

    /// Record the play and persist the track so history/favourites can find it.
    fn record_play(&mut self, track: &Track) {
        if let Some(db) = self.db.as_ref() {
            let _ = Tracks::upsert(db, track);
            let _ = History::record(db, track);
        }
        self.recent = {
            let mut recent = self.recent.clone();
            recent.retain(|t| t.youtube_id != track.youtube_id);
            recent.insert(0, track.clone());
            recent.truncate(50);
            recent
        };
    }

    /// Fetch full metadata for a track and merge it into the queue.
    ///
    /// Search results only carry title/artist/duration. The Music extractor
    /// knows the album and a proper thumbnail, and getting those makes the
    /// library entries worthwhile.
    fn fetch_metadata(&mut self, track: &Track) {
        let id = track.youtube_id.clone();
        let handle = self.tokio.clone();
        self.metadata_pending = Pending::start(&handle, async move {
            fetch_metadata(&id)
                .await
                .map(Track::from)
                .map_err(|e| e.to_string())
        });
    }

    fn poll_metadata(&mut self) {
        let Some(result) = self.metadata_pending.poll_take() else {
            return;
        };
        let Ok(enriched) = result else {
            // Metadata is a nice-to-have; a failure isn't worth a notice.
            return;
        };

        // Merge into the queue entry, keeping the title we already showed so
        // the display doesn't jump mid-playback. Only fill in fields the
        // search result couldn't provide. The autoplay flag survives the merge.
        if let Some(index) = self
            .player
            .queue
            .items()
            .iter()
            .position(|item| item.track.youtube_id == enriched.youtube_id)
        {
            let existing = self.player.queue.track_at(index);
            let merged = {
                let mut track = enriched.clone();
                if let Some(existing) = existing {
                    track.title = existing.title.clone();
                    if track.album.is_none() {
                        track.album = existing.album.clone();
                    }
                    if track.duration_secs.is_none() {
                        track.duration_secs = existing.duration_secs;
                    }
                    if track.thumbnail_url.is_none() {
                        track.thumbnail_url = existing.thumbnail_url.clone();
                    }
                }
                track
            };
            self.player.queue.update_track_at(index, merged);
        }

        if let Some(db) = self.db.as_ref() {
            let _ = Tracks::upsert(db, &enriched);
        }
    }

    /// Skip to the next track, extending the queue from the radio if needed.
    pub fn next_track(&mut self) {
        if self.player.queue.advance().is_some() {
            self.start_current();
        } else if self.player.radio_enabled {
            // Nothing left locally — ask the radio for more and play the first
            // thing it returns.
            self.request_radio_continuation();
        } else {
            self.audio.stop();
            self.notify(Notice::info("End of queue"));
        }
    }

    pub fn previous_track(&mut self) {
        // Restart the track if we're more than a few seconds in — that's what
        // the button does on every other player.
        if self.position_secs > 3.0 {
            self.seek_to(0.0);
            return;
        }
        if self.player.queue.previous().is_some() {
            self.start_current();
        }
    }

    /// Seek within the current track.
    pub fn seek_to(&mut self, seconds: f32) {
        let Some(track) = self.player.queue.current().cloned() else {
            return;
        };
        let seconds = seconds.max(0.0);
        self.position_secs = seconds;
        self.audio.seek(&track.youtube_id, seconds as u64);
    }

    pub fn toggle_play_pause(&mut self) {
        match self.audio.status().transport {
            Transport::Playing | Transport::Buffering => self.audio.pause(),
            Transport::Paused => self.audio.resume(),
            Transport::Stopped => {
                if self.player.queue.current().is_some() {
                    self.start_current();
                } else if let Some(first) = self.search_results.first() {
                    let track: Track = first.into();
                    self.play_now(track);
                }
            }
        }
    }

    /// Stop playback at the user's request.
    ///
    /// Cancels any pending "waiting for the radio" state: without this, a fetch
    /// that lands after the user pressed stop would start playing again.
    pub fn stop_playback(&mut self) {
        self.player.awaiting_radio = false;
        self.audio.stop();
    }

    pub fn set_volume(&mut self, volume: f32) {
        let volume = volume.clamp(0.0, 1.0);
        self.player.volume = volume;
        self.audio.set_volume(volume);
        self.save_setting_f32(sonus_library::setting_keys::VOLUME, volume);
    }

    pub fn cycle_repeat(&mut self) {
        let mode = self.player.queue.cycle_repeat();
        let name = match mode {
            RepeatMode::Off => "off",
            RepeatMode::All => "all",
            RepeatMode::One => "one",
        };
        self.save_setting(sonus_library::setting_keys::REPEAT_MODE, name);
        self.notify(Notice::info(format!("Repeat: {}", mode.label())));
    }

    pub fn toggle_shuffle(&mut self) {
        let shuffled = self.player.queue.toggle_shuffle();
        self.notify(Notice::info(if shuffled {
            "Shuffle on"
        } else {
            "Shuffle off"
        }));
    }

    // -----------------------------------------------------------------------
    // Radio
    // -----------------------------------------------------------------------

    /// Top the queue up from the recommendation radio.
    ///
    /// Seeded from the *newest* track in the queue rather than the one playing.
    /// See [`Player::radio_seed_track`] for why that matters.
    pub fn extend_from_radio(&mut self) {
        let Some(seed) = self.player.radio_seed_track().cloned() else {
            return;
        };
        if self.player.radio_pending.is_running() {
            return;
        }
        self.player.radio_seed = Some(seed.youtube_id.clone());

        let handle = self.tokio.clone();
        let queued: Vec<String> = self
            .player
            .queue
            .items()
            .iter()
            .map(|item| item.track.youtube_id.clone())
            .collect();

        self.player.radio_pending = Pending::start(&handle, async move {
            extend_queue(&seed, &queued, sonus_ytdlp::DEFAULT_RADIO_BATCH)
                .await
                .map_err(|e| e.to_string())
        });
    }

    /// Playback ran out of queue — fetch more and carry on when it lands.
    fn request_radio_continuation(&mut self) {
        if !self.player.radio_enabled {
            self.notify(Notice::info("End of queue"));
            return;
        }
        self.player.awaiting_radio = true;

        if self.player.radio_pending.is_running() {
            // A fetch is already in flight; `poll_radio` will pick it up.
            return;
        }
        self.extend_from_radio();
        self.notify(Notice::info("Finding more music..."));
    }

    fn poll_radio(&mut self) {
        let Some(result) = self.player.radio_pending.poll_take() else {
            return;
        };

        let added = match result {
            Ok(tracks) => self.player.queue.extend_autoplay(tracks),
            Err(e) => {
                // The network refused us — the library can still keep the
                // music going with what the user has actually listened to.
                if self.extend_local_recommendations() {
                    self.player.awaiting_radio = false;
                    if self.player.queue.advance().is_some() {
                        self.start_current();
                    }
                    return;
                }
                // Radio is an enhancement — failing it shouldn't interrupt
                // playback. Drop the wait so we don't sit here forever.
                self.player.awaiting_radio = false;
                self.notify(Notice::error(format!("Recommendations unavailable: {e}")));
                return;
            }
        };

        // Playback is waiting on this fetch: start whatever it brought back.
        //
        // This runs even when `added == 0`. Returning early there was the bug:
        // a fetch that added nothing (because the radio only suggested songs
        // already queued) left playback stopped with no explanation, and the
        // "Finding more music..." notice on screen forever.
        if self.player.awaiting_radio {
            self.player.awaiting_radio = false;

            if added == 0 {
                // The network radio had nothing new. Try the local library
                // before giving up — it's free and always available.
                if self.extend_local_recommendations() {
                    if self.player.queue.advance().is_some() {
                        self.start_current();
                    }
                    return;
                }
                // Nothing new this round. Don't keep hammering the same seed —
                // the user can ask again, or add something to the queue.
                self.notify(Notice::info("No more recommendations for this queue"));
                return;
            }

            if self.player.queue.advance().is_some() {
                self.start_current();
            }
        }
    }

    /// Top the queue up from the user's own play history.
    ///
    /// The local radio is the offline fallback: it replays the same artist's
    /// tracks, then the most-played songs, then old favourites nobody has heard
    /// in a while. Returns true when something was actually added.
    fn extend_local_recommendations(&mut self) -> bool {
        let Some(db) = self.db.as_ref() else {
            return false;
        };
        let Some(seed) = self.player.queue.current().cloned() else {
            return false;
        };

        let queued: Vec<String> = self
            .player
            .queue
            .items()
            .iter()
            .map(|item| item.track.youtube_id.clone())
            .collect();

        match sonus_library::Recommendations::similar(db, &seed, &queued, 5) {
            Ok(tracks) if !tracks.is_empty() => {
                let added = self.player.queue.extend_autoplay(tracks);
                if added > 0 {
                    self.notify(Notice::info("Playing from your library"));
                }
                added > 0
            }
            _ => false,
        }
    }

    // -----------------------------------------------------------------------
    // Playlists & favourites
    // -----------------------------------------------------------------------

    pub fn create_playlist(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let Some(db) = self.db.as_ref() else {
            return;
        };

        match Playlists::create(db, name) {
            Ok(_) => {
                self.refresh_library();
                self.notify(Notice::info(format!("Created playlist: {name}")));
            }
            Err(e) => self.notify(Notice::error(format!("Could not create playlist: {e}"))),
        }
    }

    /// Save the current queue as a playlist.
    pub fn save_queue_as_playlist(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() || self.player.queue.is_empty() {
            return;
        }
        let Some(db) = self.db.as_ref() else {
            return;
        };

        let tracks = self.player.queue.tracks();
        match Playlists::create_with(db, name, &tracks) {
            Ok(_) => {
                self.refresh_library();
                self.notify(Notice::info(format!(
                    "Saved {} tracks to {name}",
                    tracks.len()
                )));
            }
            Err(e) => self.notify(Notice::error(format!("Could not save playlist: {e}"))),
        }
    }

    pub fn add_to_playlist(&mut self, playlist_id: &str, track: &Track) {
        let Some(db) = self.db.as_ref() else {
            return;
        };
        match Playlists::add_track(db, playlist_id, track) {
            Ok(true) => {
                self.refresh_library();
                self.notify(Notice::info(format!("Added to playlist: {}", track.title)));
            }
            Ok(false) => self.notify(Notice::info("Already in that playlist")),
            Err(e) => self.notify(Notice::error(format!("Could not add track: {e}"))),
        }
    }

    pub fn delete_playlist(&mut self, id: &str) {
        let Some(db) = self.db.as_ref() else {
            return;
        };
        if let Err(e) = Playlists::delete(db, id) {
            self.notify(Notice::error(format!("Could not delete playlist: {e}")));
            return;
        }
        if self.open_playlist.as_ref().is_some_and(|p| p.id == id) {
            self.open_playlist = None;
        }
        self.refresh_library();
        self.notify(Notice::info("Playlist deleted"));
    }

    pub fn rename_playlist(&mut self, id: &str, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let Some(db) = self.db.as_ref() else {
            return;
        };
        match Playlists::rename(db, id, name) {
            Ok(()) => {
                self.refresh_library();
                if let Some(open) = &mut self.open_playlist {
                    if open.id == id {
                        open.name = name.to_string();
                    }
                }
            }
            Err(e) => self.notify(Notice::error(format!("Could not rename playlist: {e}"))),
        }
    }

    /// Load a playlist into the queue and start playing.
    pub fn play_playlist(&mut self, id: &str) {
        let Some(db) = self.db.as_ref() else {
            return;
        };
        match Playlists::get(db, id) {
            Ok(Some(playlist)) if !playlist.is_empty() => {
                let name = playlist.name.clone();
                self.play_list(playlist.tracks, 0, QueueSource::Playlist(name));
            }
            Ok(_) => self.notify(Notice::info("That playlist is empty")),
            Err(e) => self.notify(Notice::error(format!("Could not load playlist: {e}"))),
        }
    }

    pub fn open_playlist(&mut self, id: &str) {
        let Some(db) = self.db.as_ref() else {
            return;
        };
        match Playlists::get(db, id) {
            Ok(playlist) => self.open_playlist = playlist,
            Err(e) => self.notify(Notice::error(format!("Could not open playlist: {e}"))),
        }
    }

    pub fn is_liked(&self, youtube_id: &str) -> bool {
        self.liked_ids.iter().any(|id| id == youtube_id)
    }

    pub fn toggle_like(&mut self, track: &Track) {
        let Some(db) = self.db.as_ref() else {
            return;
        };
        match Favorites::toggle(db, track) {
            Ok(liked) => {
                if liked {
                    self.liked_ids.push(track.youtube_id.clone());
                    self.notify(Notice::info(format!("Liked: {}", track.title)));
                } else {
                    self.liked_ids.retain(|id| id != &track.youtube_id);
                    self.notify(Notice::info("Removed from liked"));
                }
            }
            Err(e) => self.notify(Notice::error(format!("Could not update favourites: {e}"))),
        }
    }

    // -----------------------------------------------------------------------
    // Themes
    // -----------------------------------------------------------------------

    pub fn select_theme(&mut self, index: usize) {
        if self.themes.select(index) {
            let name = self.themes.current_name().to_string();
            self.save_setting(sonus_library::setting_keys::THEME, &name);
        }
    }

    pub fn cycle_theme(&mut self) {
        self.themes.cycle();
        let name = self.themes.current_name().to_string();
        self.save_setting(sonus_library::setting_keys::THEME, &name);
    }

    // -----------------------------------------------------------------------
    // Feedback
    // -----------------------------------------------------------------------

    pub fn notify(&mut self, notice: Notice) {
        // Replace a previous notice with the same text rather than stacking
        // duplicates — holding a key down shouldn't queue ten messages.
        self.notices.retain(|n| n.text != notice.text);
        self.notices.push(notice);
        if self.notices.len() > 3 {
            self.notices.remove(0);
        }
    }

    fn tick_notices(&mut self, dt: f32) {
        for notice in &mut self.notices {
            notice.ttl -= dt;
        }
        self.notices.retain(|n| n.ttl > 0.0);
    }

    /// The notice currently worth showing, if any.
    pub fn active_notice(&self) -> Option<&Notice> {
        self.notices.last()
    }

    // -----------------------------------------------------------------------
    // Per-frame update
    // -----------------------------------------------------------------------

    /// Advance all async state and react to playback events.
    pub fn tick(&mut self, ctx: &egui::Context, dt: f32) {
        self.elapsed += dt;

        self.poll_setup();
        self.poll_stale();
        self.poll_search();
        self.poll_metadata();
        self.poll_radio();
        self.poll_audio(ctx);

        self.tick_notices(dt);

        // Keep the position moving between audio-thread updates.
        if self.scrubbing.is_none() {
            let status = self.audio.status();
            if status.transport.is_active() {
                // The audio thread reports every 50ms; interpolating keeps the
                // clock and progress bar smooth at 60fps.
                let target = status.position_ms as f32 / 1000.0;
                self.position_secs += (target - self.position_secs) * 0.25;
            }
        }
    }

    fn poll_audio(&mut self, ctx: &egui::Context) {
        let status = self.audio.status();

        // Update the spectrum from the shared analyzer.
        if let Ok(analyzer) = self.audio.spectrum().lock() {
            self.spectrum.update(analyzer.bars(), analyzer.peaks());
        }

        if let Some(error) = self.audio.take_error() {
            if self.last_audio_error.as_deref() != Some(&error) {
                self.last_audio_error = Some(error.clone());
                self.notify(Notice::error(error));
            }
        }

        // A finished track: advance the queue, or extend it from the radio.
        if self.audio.take_finished() {
            if self.player.queue.repeat_mode() == RepeatMode::One {
                // Replay the same track from the top.
                self.start_current();
            } else if self.player.queue.advance().is_some() {
                self.start_current();
            } else if self.player.radio_enabled {
                self.request_radio_continuation();
            } else {
                self.notify(Notice::info("End of queue"));
            }
        }

        // Top up the queue before it actually runs dry, so there's no gap.
        if self.player.should_extend() && status.transport.is_active() {
            self.extend_from_radio();
        }

        // Repaint while anything is animating.
        if status.transport.is_active() || !self.spectrum.is_idle() {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }
        if self.search_pending.is_running()
            || self.enrich_pending.is_running()
            || self.player.radio_pending.is_running()
            || self.metadata_pending.is_running()
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    /// The track currently loaded, if any.
    pub fn current_track(&self) -> Option<&Track> {
        self.player.queue.current()
    }

    /// Progress through the current track in `0.0..=1.0`.
    pub fn progress(&self) -> f32 {
        if let Some(scrub) = self.scrubbing {
            return scrub;
        }
        let duration = self
            .current_track()
            .and_then(|t| t.duration_secs)
            .unwrap_or(0);
        if duration == 0 {
            return 0.0;
        }
        (self.position_secs / duration as f32).clamp(0.0, 1.0)
    }
}

/// Build the audio controller's shared equalizer for the EQ view.
pub fn eq_handle(app: &SonusApp) -> Arc<std::sync::Mutex<sonus_audio::Equalizer>> {
    app.audio.eq()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_labels_are_unique() {
        let mut labels: Vec<&str> = View::ALL.iter().map(|v| v.label()).collect();
        labels.sort();
        labels.dedup();
        assert_eq!(labels.len(), View::ALL.len());
    }

    #[test]
    fn default_view_is_search() {
        assert_eq!(View::default(), View::Search);
    }

    #[test]
    fn notice_ttl_differs_by_severity() {
        assert!(Notice::error("x").ttl > Notice::info("x").ttl);
        assert!(Notice::error("x").is_error);
        assert!(!Notice::info("x").is_error);
    }

    #[test]
    fn player_defaults_to_radio_enabled() {
        let player = Player::default();
        assert!(player.radio_enabled);
        assert!(player.current().is_none());
    }

    #[test]
    fn should_extend_requires_room_left_and_a_radio_source() {
        let mut player = Player::default();
        player.queue.load(
            (0..10)
                .map(|i| Track::new(format!("id{i:09}"), "Song"))
                .collect(),
            0,
        );

        // Plenty queued — no need to fetch.
        player.queue.set_source(QueueSource::Radio);
        assert!(!player.should_extend());

        // A manual queue never extends itself.
        player.queue.load(vec![Track::new("aaaaaaaaaaa", "One")], 0);
        player.queue.set_source(QueueSource::Manual);
        assert!(!player.should_extend());

        // Radio source, nearly empty — time to fetch.
        player.queue.set_source(QueueSource::Radio);
        assert!(player.should_extend());
    }

    #[test]
    fn should_extend_respects_the_radio_toggle() {
        let mut player = Player::default();
        player.queue.load(vec![Track::new("aaaaaaaaaaa", "One")], 0);
        player.queue.set_source(QueueSource::Radio);
        assert!(player.should_extend());

        player.radio_enabled = false;
        assert!(!player.should_extend());
    }

    /// Radio must be seeded from the newest track, not the one playing.
    ///
    /// Seeding from the current track is what caused playback to stall at the
    /// end of a queue: the radio for a mid-list song mostly suggests tracks
    /// already queued, so the fetch added nothing and playback stopped dead.
    #[test]
    fn radio_seeds_from_the_newest_track() {
        let mut player = Player::default();
        player.queue.load(
            vec![
                Track::new("aaaaaaaaaaa", "First"),
                Track::new("bbbbbbbbbbb", "Second"),
                Track::new("ccccccccccc", "Newest"),
            ],
            0,
        );

        // Cursor on the first track, but the seed must be the last one.
        assert_eq!(player.queue.current().unwrap().youtube_id, "aaaaaaaaaaa");
        assert_eq!(
            player.radio_seed_track().map(|t| t.youtube_id.as_str()),
            Some("ccccccccccc")
        );
    }

    #[test]
    fn radio_seed_is_none_for_an_empty_queue() {
        let player = Player::default();
        assert!(player.radio_seed_track().is_none());
    }

    #[test]
    fn radio_seed_follows_the_cursor_after_a_jump() {
        let mut player = Player::default();
        player.queue.load(
            vec![
                Track::new("aaaaaaaaaaa", "One"),
                Track::new("bbbbbbbbbbb", "Two"),
            ],
            0,
        );
        // The frontier doesn't move just because playback did — the newest
        // track is still the one that grows the queue best.
        player.queue.jump_to(1);
        assert_eq!(
            player.radio_seed_track().map(|t| t.youtube_id.as_str()),
            Some("bbbbbbbbbbb")
        );
    }

    #[test]
    fn awaiting_radio_starts_false() {
        // Guards the "stop then a stale fetch starts playing" bug.
        assert!(!Player::default().awaiting_radio);
    }

    #[test]
    fn should_extend_stops_once_the_fetch_is_in_flight() {
        // The poll loop runs every 50ms; without this guard it would fire a
        // fresh fetch on every tick while the first one is still running.
        let mut player = Player::default();
        player.queue.load(vec![Track::new("aaaaaaaaaaa", "One")], 0);
        player.queue.set_source(QueueSource::Radio);
        assert!(player.should_extend(), "an empty radio queue should top up");

        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        player.radio_pending = Pending::start(rt.handle(), async {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            Ok(Vec::new())
        });

        assert!(
            !player.should_extend(),
            "must not queue a second fetch while one is in flight"
        );
    }

    #[test]
    fn is_playing_matches_the_cursor() {
        let mut player = Player::default();
        player.queue.load(vec![Track::new("aaaaaaaaaaa", "One")], 0);
        assert!(player.is_playing("aaaaaaaaaaa"));
        assert!(!player.is_playing("bbbbbbbbbbb"));
    }

    // -----------------------------------------------------------------------
    // SetupState
    // -----------------------------------------------------------------------

    #[test]
    fn setup_starts_idle() {
        let setup = SetupState::new();
        assert!(!setup.is_running());
        assert!(!setup.has_failed());
        assert!(setup.entries().is_empty());
    }

    #[test]
    fn setup_default_is_the_same_as_new() {
        let a = SetupState::default();
        let b = SetupState::new();
        assert_eq!(a.entries().len(), b.entries().len());
    }

    #[test]
    fn setup_poll_absorbs_shared_state() {
        let mut setup = SetupState::new();

        // Simulate the installer thread writing progress.
        {
            let mut guard = setup.shared.lock().unwrap();
            guard.push(SetupEntry {
                dep: sonus_deps::Dependency::YtDlp,
                progress: 0.5,
                label: "yt-dlp: 50%".into(),
                done: false,
                error: None,
            });
        }

        assert!(
            setup.entries().is_empty(),
            "shared state not visible until poll"
        );
        setup.poll();
        let entries = setup.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].dep, sonus_deps::Dependency::YtDlp);
        assert!((entries[0].progress - 0.5).abs() < 1e-6);
    }

    #[test]
    fn setup_running_until_all_done() {
        let mut setup = SetupState::new();
        let make = |done, error| SetupEntry {
            dep: sonus_deps::Dependency::YtDlp,
            progress: 1.0,
            label: "yt-dlp".into(),
            done,
            error,
        };

        // One entry still going → running.
        setup.entries = vec![make(false, None)];
        assert!(setup.is_running());

        // All done → not running.
        setup.entries = vec![make(true, None), make(true, None)];
        assert!(!setup.is_running());
        assert!(!setup.has_failed());

        // A failure is not "running" (no point waiting), but is "failed".
        setup.entries = vec![make(false, Some("network down".into()))];
        assert!(!setup.is_running());
        assert!(setup.has_failed());
    }
}
