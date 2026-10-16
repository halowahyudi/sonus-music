//! The audio thread.
//!
//! `AudioEngine` isn't `Send` — rodio's `OutputStream` holds platform-specific
//! handles that can't cross threads. So the engine lives on a dedicated thread
//! and the GUI talks to it over a channel.
//!
//! The GUI never blocks on the audio thread. It sends commands and reads a
//! snapshot of shared state, which is what keeps the UI at 60fps while a
//! track buffers.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sonus_audio::{AudioEngine, Equalizer, SpectrumAnalyzer, Volume, NUM_BARS};
use sonus_ytdlp::{stream_audio_from, OUTPUT_CHANNELS, OUTPUT_SAMPLE_RATE};

/// Playback transport state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Transport {
    /// Nothing loaded.
    #[default]
    Stopped,
    /// Decoding and playing.
    Playing,
    Paused,
    /// A track is loaded but the pipeline is still starting up.
    Buffering,
}

impl Transport {
    pub fn label(self) -> &'static str {
        match self {
            Transport::Stopped => "STOPPED",
            Transport::Playing => "PLAYING",
            Transport::Paused => "PAUSED",
            Transport::Buffering => "BUFFERING",
        }
    }

    pub fn is_active(self) -> bool {
        matches!(self, Transport::Playing | Transport::Buffering)
    }
}

/// Messages the GUI sends to the audio thread.
#[derive(Debug, Clone)]
pub enum AudioCommand {
    /// Start a track from the beginning.
    Play {
        youtube_id: String,
    },
    /// Start a track from `position_secs` in.
    Seek {
        youtube_id: String,
        position_secs: u64,
    },
    Pause,
    Resume,
    Stop,
    SetVolume(f32),
    /// Shut the thread down.
    Quit,
}

/// State the audio thread publishes for the GUI to read.
#[derive(Debug, Default)]
pub struct AudioStatus {
    pub transport: Transport,
    /// YouTube ID currently loaded, if any.
    pub current_id: Option<String>,
    /// Playback position in milliseconds.
    pub position_ms: u64,
    /// Total duration in milliseconds, when known.
    pub duration_ms: u64,
    /// How many samples are decoded and waiting. Non-zero means we're ahead.
    pub buffered_samples: usize,
    /// Set when a track finishes naturally, so the GUI can advance the queue.
    pub track_finished: bool,
    /// False until the engine has opened an output device. The UI uses this to
    /// disable the transport instead of pretending to play.
    pub device_ready: bool,
    /// Last error, cleared once the GUI has shown it.
    pub error: Option<String>,
}

impl AudioStatus {
    pub fn position_secs(&self) -> u64 {
        self.position_ms / 1000
    }

    pub fn duration_secs(&self) -> u64 {
        self.duration_ms / 1000
    }

    /// Fraction played, or `None` when the duration isn't known.
    pub fn progress(&self) -> Option<f32> {
        if self.duration_ms == 0 {
            return None;
        }
        Some((self.position_ms as f32 / self.duration_ms as f32).clamp(0.0, 1.0))
    }
}

/// Handle the GUI holds onto.
pub struct AudioController {
    tx: Sender<AudioCommand>,
    status: Arc<Mutex<AudioStatus>>,
    /// Set by the audio thread when a track ends; the GUI consumes it.
    finished: Arc<AtomicBool>,
    eq: Arc<Mutex<Equalizer>>,
    spectrum: Arc<Mutex<SpectrumAnalyzer>>,
    volume: Arc<Mutex<f32>>,
    /// Incremented on every `play`, so a stale "finished" flag from the
    /// previous track can be told apart from the current one.
    generation: Arc<AtomicU64>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl AudioController {
    /// Spawn the audio thread.
    ///
    /// Fails only if the thread can't be created; a missing audio device is
    /// reported through `status.error` instead, so the GUI can still run and
    /// show the problem.
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel();
        let status = Arc::new(Mutex::new(AudioStatus::default()));
        let finished = Arc::new(AtomicBool::new(false));
        let eq = Arc::new(Mutex::new(Equalizer::new(OUTPUT_SAMPLE_RATE)));
        let spectrum = Arc::new(Mutex::new(SpectrumAnalyzer::new(NUM_BARS)));
        let volume = Arc::new(Mutex::new(0.7_f32));
        let generation = Arc::new(AtomicU64::new(0));

        let thread = {
            let status = status.clone();
            let finished = finished.clone();
            let eq = eq.clone();
            let spectrum = spectrum.clone();
            let volume = volume.clone();
            let generation = generation.clone();

            std::thread::Builder::new()
                .name("sonus-audio".into())
                .spawn(move || {
                    run(rx, status, finished, eq, spectrum, volume, generation);
                })
                .ok()
        };

        Self {
            tx,
            status,
            finished,
            eq,
            spectrum,
            volume,
            generation,
            thread,
        }
    }

    /// Start a track from the beginning.
    pub fn play(&self, youtube_id: impl Into<String>) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.finished.store(false, Ordering::SeqCst);
        let _ = self.tx.send(AudioCommand::Play {
            youtube_id: youtube_id.into(),
        });
    }

    /// Restart the current track from `position_secs`.
    pub fn seek(&self, youtube_id: impl Into<String>, position_secs: u64) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.finished.store(false, Ordering::SeqCst);
        let _ = self.tx.send(AudioCommand::Seek {
            youtube_id: youtube_id.into(),
            position_secs,
        });
    }

    pub fn pause(&self) {
        let _ = self.tx.send(AudioCommand::Pause);
    }

    pub fn resume(&self) {
        let _ = self.tx.send(AudioCommand::Resume);
    }

    pub fn stop(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.finished.store(false, Ordering::SeqCst);
        let _ = self.tx.send(AudioCommand::Stop);
    }

    pub fn toggle_pause(&self) {
        if self.status().transport == Transport::Paused {
            self.resume();
        } else {
            self.pause();
        }
    }

    pub fn set_volume(&self, volume: f32) {
        if let Ok(mut v) = self.volume.lock() {
            *v = volume.clamp(0.0, 1.0);
        }
        let _ = self.tx.send(AudioCommand::SetVolume(volume));
    }

    pub fn volume(&self) -> f32 {
        self.volume.lock().map(|v| *v).unwrap_or(0.7)
    }

    pub fn status(&self) -> AudioStatus {
        self.status
            .lock()
            .map(|s| AudioStatus {
                transport: s.transport,
                current_id: s.current_id.clone(),
                position_ms: s.position_ms,
                duration_ms: s.duration_ms,
                buffered_samples: s.buffered_samples,
                track_finished: s.track_finished,
                device_ready: s.device_ready,
                error: s.error.clone(),
            })
            .unwrap_or_default()
    }

    /// Consume the "track ended" flag.
    ///
    /// Returns true exactly once per track — the GUI calls this each frame and
    /// advances the queue when it fires.
    pub fn take_finished(&self) -> bool {
        if self.finished.swap(false, Ordering::SeqCst) {
            if let Ok(mut status) = self.status.lock() {
                status.track_finished = false;
            }
            return true;
        }
        false
    }

    pub fn take_error(&self) -> Option<String> {
        self.status
            .lock()
            .ok()
            .and_then(|mut status| status.error.take())
    }

    pub fn eq(&self) -> Arc<Mutex<Equalizer>> {
        self.eq.clone()
    }

    pub fn spectrum(&self) -> Arc<Mutex<SpectrumAnalyzer>> {
        self.spectrum.clone()
    }

    /// Current generation counter — used to discard stale async results.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }
}

impl Drop for AudioController {
    fn drop(&mut self) {
        let _ = self.tx.send(AudioCommand::Quit);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// How often the audio thread refreshes the shared status.
///
/// Must match `sonus_audio::POLL_STEP`: the engine measures starvation by
/// counting poll ticks, so a mismatch would make its timeout wrong.
const POLL_INTERVAL: Duration = sonus_audio::POLL_STEP;

/// How long a stream may stay completely dry before we give up on it.
///
/// The sink emptying is the real "finished" signal, but a pipeline that dies
/// without sending EOF never produces one — the track would sit in "buffering"
/// for the rest of the session. This is the backstop for that.
///
/// It's deliberately generous. `gated_for` only accumulates while *zero*
/// samples are buffered, so a slow-but-alive stream resets it constantly and is
/// never cut off however long it takes. The only thing this catches is a stream
/// that has delivered nothing at all, and the cost of a false positive —
/// killing playback the user is waiting on — is much worse than a false
/// negative, where they see BUFFERING and skip on themselves.
const STARVATION_TIMEOUT: Duration = Duration::from_secs(30);

fn run(
    rx: Receiver<AudioCommand>,
    status: Arc<Mutex<AudioStatus>>,
    finished: Arc<AtomicBool>,
    eq: Arc<Mutex<Equalizer>>,
    spectrum: Arc<Mutex<SpectrumAnalyzer>>,
    volume: Arc<Mutex<f32>>,
    generation: Arc<AtomicU64>,
) {
    let mut engine = match AudioEngine::new_with(eq, spectrum) {
        Ok(engine) => engine,
        Err(e) => {
            // No audio device: keep serving commands so the UI stays alive and
            // the user sees a clear message instead of a dead window.
            if let Ok(mut status) = status.lock() {
                status.error = Some(format!("Audio output unavailable: {e}"));
                status.transport = Transport::Stopped;
            }
            while let Ok(command) = rx.recv() {
                if matches!(command, AudioCommand::Quit) {
                    break;
                }
            }
            return;
        }
    };

    if let Ok(v) = volume.lock() {
        engine.set_volume(Volume::new(*v));
    }

    publish(&status, |s| s.device_ready = true);

    // Duration of the loaded track, used for the progress bar.
    let mut duration_ms: u64 = 0;
    // Generation this thread is currently playing, so a command that arrives
    // while a previous track is loading can supersede it.
    let mut current_generation = 0_u64;

    // The live yt-dlp → ffmpeg pipeline for the current track.
    //
    // Held purely for its lifetime: `AudioPipeline` kills its child processes
    // on drop, so this keeps the stream open. Replacing it (on the next
    // play/seek/stop) drops the previous one and reaps its children. The value
    // is never read, which is why it's underscore-prefixed.
    let mut _pipeline: Option<sonus_ytdlp::AudioPipeline> = None;

    loop {
        // Drain everything queued before doing work, so a rapid burst of
        // clicks collapses into the final state instead of spawning pipelines
        // that are immediately torn down.
        let mut latest: Option<AudioCommand> = None;
        loop {
            match rx.recv_timeout(POLL_INTERVAL) {
                Ok(AudioCommand::Quit) => {
                    engine.stop();
                    _pipeline = None;
                    return;
                }
                Ok(command) => latest = Some(command),
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    engine.stop();
                    _pipeline = None;
                    return;
                }
            }
        }

        if let Some(command) = latest {
            match command {
                AudioCommand::Play { youtube_id } => {
                    current_generation = generation.load(Ordering::SeqCst);
                    duration_ms = 0;

                    publish(&status, |s| {
                        s.transport = Transport::Buffering;
                        s.current_id = Some(youtube_id.clone());
                        s.position_ms = 0;
                        s.duration_ms = 0;
                        s.track_finished = false;
                    });

                    match stream_audio_from(&youtube_id, 0) {
                        Ok(mut new_pipeline) => {
                            // Take the reader first, then keep the pipeline so
                            // its child processes stay alive for the track.
                            if let Some(stdout) = new_pipeline.take_stdout() {
                                engine.play_pcm(
                                    stdout,
                                    OUTPUT_SAMPLE_RATE,
                                    OUTPUT_CHANNELS,
                                    Duration::ZERO,
                                );
                            }
                            _pipeline = Some(new_pipeline);
                            // Stay in Buffering — the poll loop promotes us to
                            // Playing once the pre-roll gate actually opens.
                        }
                        Err(e) => {
                            _pipeline = None;
                            publish(&status, |s| {
                                s.transport = Transport::Stopped;
                                s.error = Some(format!("{e}"));
                            });
                        }
                    }
                }

                AudioCommand::Seek {
                    youtube_id,
                    position_secs,
                } => {
                    current_generation = generation.load(Ordering::SeqCst);

                    publish(&status, |s| {
                        s.transport = Transport::Buffering;
                        s.position_ms = position_secs * 1000;
                    });

                    match stream_audio_from(&youtube_id, position_secs) {
                        Ok(mut new_pipeline) => {
                            if let Some(stdout) = new_pipeline.take_stdout() {
                                engine.play_pcm(
                                    stdout,
                                    OUTPUT_SAMPLE_RATE,
                                    OUTPUT_CHANNELS,
                                    Duration::from_secs(position_secs),
                                );
                            }
                            _pipeline = Some(new_pipeline);
                            // Buffering until the gate opens — the timeline
                            // must not creep forward while ffmpeg is still
                            // seeking and rendering from `position_secs`.
                        }
                        Err(e) => {
                            _pipeline = None;
                            publish(&status, |s| {
                                s.transport = Transport::Stopped;
                                s.error = Some(format!("{e}"));
                            });
                        }
                    }
                }

                AudioCommand::Pause => {
                    engine.pause();
                    publish(&status, |s| s.transport = Transport::Paused);
                }

                AudioCommand::Resume => {
                    if !engine.is_finished() {
                        engine.resume();
                        // Resuming a track that hasn't buffered yet leaves the
                        // gate in charge — the poll loop will promote it to
                        // Playing once audio actually starts.
                        publish(&status, |s| {
                            s.transport = if engine.is_gated() {
                                Transport::Buffering
                            } else {
                                Transport::Playing
                            };
                        });
                    }
                }

                AudioCommand::Stop => {
                    engine.stop();
                    _pipeline = None;
                    publish(&status, |s| {
                        s.transport = Transport::Stopped;
                        s.current_id = None;
                        s.position_ms = 0;
                        s.duration_ms = 0;
                        s.buffered_samples = 0;
                    });
                }

                AudioCommand::SetVolume(v) => {
                    engine.set_volume(Volume::new(v));
                }

                AudioCommand::Quit => {
                    engine.stop();
                    _pipeline = None;
                    return;
                }
            }
        }

        // ---- periodic status refresh -------------------------------------
        let transport = status
            .lock()
            .map(|s| s.transport)
            .unwrap_or(Transport::Stopped);

        if matches!(
            transport,
            Transport::Playing | Transport::Buffering | Transport::Paused
        ) {
            // Drive the buffering gate. While it's shut the engine holds the
            // source back from the sink entirely, so the position is frozen at
            // the seek target and no silence is played. This is what stops the
            // clock running ahead of yt-dlp while it renders the stream.
            //
            // A paused track skips this: the user's pause already stops the
            // clock, and counting that time as starvation would abandon a
            // perfectly healthy stream.
            if transport != Transport::Paused {
                engine.poll_gate();
            }

            let gated = engine.is_gated();
            let position = engine.position();
            let buffered = engine.buffered_samples();

            // A track is done when the sink has drained. rodio reports this
            // directly, which is more reliable than watching the clock.
            let ended = transport != Transport::Paused && engine.is_finished();

            // Give up on a stream that never arrives. A pipeline that dies
            // without sending EOF leaves the gate shut forever, and the track
            // would sit in "buffering" for the rest of the session. Note this
            // does *not* require the gate to have opened first — a stream that
            // delivers nothing at all is the case this exists to catch.
            //
            // `gated_for` only accumulates while the buffer is completely dry,
            // so a slow-but-alive stream that trickles audio in is never cut
            // off, however long it takes.
            let abandoned =
                transport != Transport::Paused && gated && engine.gated_for() >= STARVATION_TIMEOUT;

            publish(&status, |s| {
                s.position_ms = position.as_millis() as u64;
                s.duration_ms = duration_ms;
                s.buffered_samples = buffered;
                // Buffering lasts exactly as long as the gate is shut.
                if s.transport == Transport::Buffering && !gated {
                    s.transport = Transport::Playing;
                } else if s.transport == Transport::Playing && gated {
                    s.transport = Transport::Buffering;
                }
            });

            if (ended || abandoned) && current_generation == generation.load(Ordering::SeqCst) {
                engine.stop();
                publish(&status, |s| {
                    s.transport = Transport::Stopped;
                    s.track_finished = true;
                    s.buffered_samples = 0;
                    if abandoned {
                        s.error = Some("Playback stalled — the stream stopped responding".into());
                    }
                });
                finished.store(true, Ordering::SeqCst);
            }
        }
    }
}

fn publish(status: &Arc<Mutex<AudioStatus>>, update: impl FnOnce(&mut AudioStatus)) {
    match status.lock() {
        Ok(mut status) => update(&mut status),
        Err(poisoned) => update(&mut poisoned.into_inner()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_labels() {
        assert_eq!(Transport::Playing.label(), "PLAYING");
        assert_eq!(Transport::Stopped.label(), "STOPPED");
        assert!(Transport::Playing.is_active());
        assert!(Transport::Buffering.is_active());
        assert!(!Transport::Paused.is_active());
    }

    #[test]
    fn status_progress_is_none_without_a_duration() {
        let status = AudioStatus::default();
        assert_eq!(status.progress(), None);
    }

    #[test]
    fn status_progress_is_a_fraction() {
        let status = AudioStatus {
            position_ms: 30_000,
            duration_ms: 120_000,
            ..Default::default()
        };
        assert_eq!(status.progress(), Some(0.25));
        assert_eq!(status.position_secs(), 30);
        assert_eq!(status.duration_secs(), 120);
    }

    #[test]
    fn status_progress_clamps() {
        let status = AudioStatus {
            position_ms: 999_000,
            duration_ms: 1000,
            ..Default::default()
        };
        assert_eq!(status.progress(), Some(1.0));
    }

    #[test]
    fn controller_starts_and_shuts_down() {
        let controller = AudioController::spawn();
        assert_eq!(controller.status().transport, Transport::Stopped);
        // Dropping joins the thread — this test hangs if shutdown is broken.
        drop(controller);
    }

    #[test]
    fn volume_round_trips_through_the_controller() {
        let controller = AudioController::spawn();
        controller.set_volume(0.42);
        assert!((controller.volume() - 0.42).abs() < 1e-6);
    }

    #[test]
    fn volume_is_clamped() {
        let controller = AudioController::spawn();
        controller.set_volume(5.0);
        assert_eq!(controller.volume(), 1.0);
        controller.set_volume(-1.0);
        assert_eq!(controller.volume(), 0.0);
    }

    #[test]
    fn stop_clears_the_current_track() {
        let controller = AudioController::spawn();
        controller.stop();
        // Give the audio thread a moment to process the command.
        std::thread::sleep(Duration::from_millis(100));
        let status = controller.status();
        assert_eq!(status.transport, Transport::Stopped);
        assert!(status.current_id.is_none());
    }

    #[test]
    fn take_finished_fires_once() {
        let controller = AudioController::spawn();
        controller.finished.store(true, Ordering::SeqCst);
        assert!(controller.take_finished());
        assert!(!controller.take_finished());
    }

    #[test]
    fn generation_increments_on_each_play() {
        let controller = AudioController::spawn();
        let before = controller.generation();
        controller.play("aaaaaaaaaaa");
        controller.play("bbbbbbbbbbb");
        assert_eq!(controller.generation(), before + 2);
    }

    #[test]
    fn generation_increments_on_stop() {
        let controller = AudioController::spawn();
        let before = controller.generation();
        controller.stop();
        assert_eq!(controller.generation(), before + 1);
    }

    #[test]
    fn toggle_pause_pauses_when_playing() {
        let controller = AudioController::spawn();
        // Wait for the thread to either open a device or report it can't.
        wait_for_device(&controller);
        if !controller.status().device_ready {
            // Headless CI has no output device; the fallback path ignores
            // transport commands by design.
            return;
        }

        publish(&controller.status, |s| s.transport = Transport::Playing);
        controller.toggle_pause();
        wait_for_transport(&controller, Transport::Paused);
        assert_eq!(controller.status().transport, Transport::Paused);
    }

    #[test]
    fn toggle_pause_resumes_when_paused() {
        let controller = AudioController::spawn();
        wait_for_device(&controller);
        if !controller.status().device_ready {
            return;
        }

        publish(&controller.status, |s| s.transport = Transport::Paused);
        controller.toggle_pause();
        wait_for_transport(&controller, Transport::Playing);
        assert_eq!(controller.status().transport, Transport::Playing);
    }

    #[test]
    fn status_reports_whether_a_device_is_available() {
        let controller = AudioController::spawn();
        wait_for_device(&controller);
        // Either way the field must be meaningful rather than stuck default.
        let status = controller.status();
        if !status.device_ready {
            assert!(
                status.error.is_some(),
                "a missing device should come with an explanation"
            );
        }
    }

    /// Wait until the audio thread has settled into a known state.
    fn wait_for_device(controller: &AudioController) {
        for _ in 0..100 {
            let status = controller.status();
            if status.device_ready || status.error.is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Busy-wait for a transport transition to propagate to shared state.
    /// Commands cross an mpsc channel and are applied on the audio thread,
    /// so fixed sleeps are flaky on loaded CI machines — poll instead.
    fn wait_for_transport(controller: &AudioController, want: Transport) {
        for _ in 0..200 {
            if controller.status().transport == want {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn eq_and_spectrum_are_shared_with_the_thread() {
        let controller = AudioController::spawn();
        assert_eq!(controller.eq().lock().unwrap().band(0), 0.0);
        assert_eq!(controller.spectrum().lock().unwrap().num_bars(), NUM_BARS);
    }

    #[test]
    fn take_error_clears_the_error() {
        let controller = AudioController::spawn();
        publish(&controller.status, |s| s.error = Some("boom".into()));
        assert_eq!(controller.take_error().as_deref(), Some("boom"));
        assert!(controller.take_error().is_none());
    }

    #[test]
    fn poll_interval_matches_the_engine_poll_step() {
        // The engine measures starvation by counting poll ticks, so if the
        // audio thread polled at a different rate the timeout would be wrong.
        assert_eq!(POLL_INTERVAL, sonus_audio::POLL_STEP);
    }

    #[test]
    fn starvation_timeout_is_generous_enough_for_a_slow_start() {
        // yt-dlp resolving formats on a poor connection can take a while. The
        // gate only counts time with a completely dry buffer, so this timeout
        // must not be so short that a legitimate start gets killed.
        assert!(
            STARVATION_TIMEOUT >= Duration::from_secs(20),
            "a short starvation timeout would abandon healthy streams"
        );
    }

    #[test]
    fn transport_round_trips_through_the_shared_status() {
        // `publish` is how the audio thread talks to the GUI; the copy in
        // `status()` must reflect every field.
        let controller = AudioController::spawn();
        publish(&controller.status, |s| {
            s.transport = Transport::Buffering;
            s.current_id = Some("abcdefghijk".into());
            s.position_ms = 1_500;
            s.duration_ms = 60_000;
            s.buffered_samples = 42;
        });

        let status = controller.status();
        assert_eq!(status.transport, Transport::Buffering);
        assert_eq!(status.current_id.as_deref(), Some("abcdefghijk"));
        assert_eq!(status.position_ms, 1_500);
        assert_eq!(status.buffered_samples, 42);
        assert_eq!(status.progress(), Some(0.025));
    }

    #[test]
    fn buffering_is_not_reported_as_playing() {
        // The GUI keys its "buffering" spinner and frozen clock off this, so a
        // gated track must never look like it's playing.
        let controller = AudioController::spawn();
        publish(&controller.status, |s| s.transport = Transport::Buffering);
        assert!(!matches!(controller.status().transport, Transport::Playing));
    }
}
