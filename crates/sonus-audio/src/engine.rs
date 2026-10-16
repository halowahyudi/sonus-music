use std::collections::VecDeque;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use thiserror::Error;

use rodio::Source;

use crate::equalizer::Equalizer;
use crate::spectrum::SpectrumAnalyzer;
use crate::volume::Volume;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("audio output error: {0}")]
    Output(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// How much audio must be buffered before the sink is allowed to run.
///
/// Releasing the sink the moment it's created means rodio spends the whole of
/// yt-dlp's and ffmpeg's startup consuming the silence `PcmSource` pads with.
/// The clock then runs ahead of the sound, and the ring buffer never gets a
/// cushion, so the first network hiccup stutters. Holding back for a pre-roll
/// fixes both: nothing plays until there's something to hear, and the timeline
/// only moves when the audio does.
const PREROLL: Duration = Duration::from_millis(400);

/// Samples needed to cover [`PREROLL`] at a given output format.
fn preroll_for(sample_rate: u32, channels: u16) -> usize {
    let frames = sample_rate.max(1) as u64 * PREROLL.as_millis() as u64 / 1000;
    (frames * channels.max(1) as u64).max(1) as usize
}

/// How often the caller is expected to call [`AudioEngine::poll_gate`].
///
/// The engine can't time this itself without spawning a thread, and it needs
/// the interval to know how long the gate has been shut. The audio thread
/// polls on exactly this cadence.
pub const POLL_STEP: Duration = Duration::from_millis(50);

/// Ring buffer shared between the pipe reader and rodio's mixer thread.
///
/// The reader blocks on the condvar when the buffer is full instead of
/// spinning. That matters: the pipe applies backpressure to yt-dlp/ffmpeg, so
/// the whole chain produces audio at roughly playback rate and memory stays
/// flat regardless of track length.
struct RingBuffer {
    samples: Mutex<VecDeque<f32>>,
    /// Signalled when the consumer drains space.
    space_available: Condvar,
    /// Signalled when the producer adds data or finishes.
    data_available: Condvar,
    capacity: usize,
    producer_done: AtomicBool,
    /// Set when the session is torn down; the producer checks this to exit.
    cancelled: AtomicBool,
}

impl RingBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            samples: Mutex::new(VecDeque::with_capacity(capacity)),
            space_available: Condvar::new(),
            data_available: Condvar::new(),
            capacity,
            producer_done: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
        }
    }

    /// Block until there's room, then push. Returns false if cancelled.
    fn push(&self, chunk: &[f32]) -> bool {
        let mut samples = self.samples.lock().unwrap();
        while samples.len() + chunk.len() > self.capacity {
            if self.cancelled.load(Ordering::Relaxed) {
                return false;
            }
            // 20ms is short enough to feel responsive on teardown and long
            // enough not to burn CPU while waiting.
            let (guard, timeout) = self
                .space_available
                .wait_timeout(samples, Duration::from_millis(20))
                .unwrap();
            samples = guard;
            if timeout.timed_out() && self.cancelled.load(Ordering::Relaxed) {
                return false;
            }
        }
        samples.extend(chunk.iter().copied());
        drop(samples);
        self.data_available.notify_one();
        true
    }

    fn pop(&self) -> Option<f32> {
        let mut samples = self.samples.lock().unwrap();
        let sample = samples.pop_front();
        if sample.is_some() {
            drop(samples);
            self.space_available.notify_one();
        }
        sample
    }

    fn len(&self) -> usize {
        self.samples.lock().unwrap().len()
    }

    fn clear(&self) {
        self.samples.lock().unwrap().clear();
        self.space_available.notify_all();
    }

    fn mark_done(&self) {
        self.producer_done.store(true, Ordering::Relaxed);
        self.data_available.notify_all();
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
        self.space_available.notify_all();
        self.data_available.notify_all();
    }

    fn is_done(&self) -> bool {
        self.producer_done.load(Ordering::Relaxed)
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

/// rodio source that drains the ring buffer.
///
/// While the producer is still running, an empty buffer yields silence rather
/// than ending the stream — that keeps the sink alive across a network stall
/// instead of cutting the track short. Only when the producer signals it's
/// finished does `next()` return `None`.
///
/// That silence is exactly why [`AudioEngine`] gates the sink: rodio counts it
/// as playback time, so an ungated source would let the clock run ahead of the
/// audio whenever the buffer ran dry. The gate is what keeps this padding from
/// being audible, or visible on the progress bar.
struct PcmSource {
    buffer: Arc<RingBuffer>,
    sample_rate: u32,
    channels: u16,
}

impl Iterator for PcmSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if let Some(sample) = self.buffer.pop() {
            return Some(sample);
        }
        if self.buffer.is_done() || self.buffer.is_cancelled() {
            None
        } else {
            Some(0.0)
        }
    }
}

impl Source for PcmSource {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }
    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    fn channels(&self) -> u16 {
        self.channels
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// Owns the audio output and one track's worth of decoding state.
///
/// Not `Send`: rodio's `OutputStream` holds platform-specific handles. The GUI
/// keeps one of these pinned to a dedicated thread and talks to it over a
/// channel.
pub struct AudioEngine {
    _stream: rodio::OutputStream,
    stream_handle: rodio::OutputStreamHandle,
    sink: rodio::Sink,
    buffer: Option<Arc<RingBuffer>>,
    /// Decoded audio waiting to be handed to the sink once the gate opens.
    ///
    /// Holding the source back rather than appending it paused is what makes
    /// the pre-roll exact. rodio only applies a pause on its next control tick,
    /// so an appended-but-paused source still gets pulled for up to one device
    /// buffer — enough to advance the clock a few tens of milliseconds past
    /// where the audio actually is. An empty sink can't drift at all.
    pending: Option<PcmSource>,
    volume: Volume,
    eq: Arc<Mutex<Equalizer>>,
    spectrum: Arc<Mutex<SpectrumAnalyzer>>,
    /// How far into the track the current sink started. A seek restarts the
    /// pipeline, so the sink's own position resets to zero and this offset is
    /// what keeps the displayed time continuous.
    position_offset: Duration,
    /// True once a track has been handed to the sink.
    loaded: bool,
    /// Samples the ring buffer must hold before the sink is released.
    buffer_target: usize,
    /// True while the sink is held back waiting for audio to arrive. Set for
    /// the initial pre-roll and again if the buffer runs dry mid-track.
    gated: bool,
    /// How long the gate has been shut. A stream that dies without sending EOF
    /// leaves the gate shut forever; this is what lets the UI notice.
    gated_for: Duration,
    /// User-requested pause. Tracked separately from the sink's own flag
    /// because the gate pauses the sink too, and a gate must never be mistaken
    /// for the user pressing pause.
    paused: bool,
}

impl AudioEngine {
    pub fn new() -> Result<Self, EngineError> {
        let eq = Arc::new(Mutex::new(Equalizer::new(48000)));
        let spectrum = Arc::new(Mutex::new(SpectrumAnalyzer::new(crate::spectrum::NUM_BARS)));
        Self::new_with(eq, spectrum)
    }

    pub fn new_with(
        eq: Arc<Mutex<Equalizer>>,
        spectrum: Arc<Mutex<SpectrumAnalyzer>>,
    ) -> Result<Self, EngineError> {
        let (stream, stream_handle) =
            rodio::OutputStream::try_default().map_err(|e| EngineError::Output(e.to_string()))?;
        let sink =
            rodio::Sink::try_new(&stream_handle).map_err(|e| EngineError::Output(e.to_string()))?;
        let volume = Volume::default();
        sink.set_volume(volume.as_f32());

        Ok(Self {
            _stream: stream,
            stream_handle,
            sink,
            buffer: None,
            pending: None,
            volume,
            eq,
            spectrum,
            position_offset: Duration::ZERO,
            loaded: false,
            buffer_target: 0,
            gated: false,
            gated_for: Duration::ZERO,
            paused: false,
        })
    }

    pub fn eq(&self) -> Arc<Mutex<Equalizer>> {
        self.eq.clone()
    }

    pub fn spectrum(&self) -> Arc<Mutex<SpectrumAnalyzer>> {
        self.spectrum.clone()
    }

    pub fn volume(&self) -> Volume {
        self.volume
    }

    pub fn set_volume(&mut self, volume: Volume) {
        self.volume = volume;
        self.sink.set_volume(volume.as_f32());
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn pause(&mut self) {
        self.paused = true;
        self.sink.pause();
    }

    pub fn resume(&mut self) {
        self.paused = false;
        // A sink still waiting for its pre-roll must stay held back — the gate
        // opens on its own once there's audio to play.
        if !self.gated {
            self.sink.play();
        }
    }

    /// True while the sink is held back waiting for audio to arrive.
    ///
    /// The UI shows "buffering" and freezes the timeline while this is set.
    pub fn is_gated(&self) -> bool {
        self.gated
    }

    /// How long the gate has been shut with nothing arriving.
    ///
    /// A pipeline that dies without sending EOF leaves `PcmSource` padding
    /// silence forever, so the track never "finishes" on its own. The caller
    /// uses this to give up on it.
    pub fn gated_for(&self) -> Duration {
        self.gated_for
    }

    /// Advance the buffering gate. Call once per poll from the audio thread.
    ///
    /// The gate exists to stop the timeline running ahead of the audio. It is
    /// shut in two situations:
    ///
    /// * **Before the first audio arrives.** yt-dlp and ffmpeg take seconds to
    ///   produce a stream. `PcmSource` pads an empty buffer with silence, and
    ///   rodio counts that silence as playback time, so the clock would run
    ///   ahead of the sound — most visibly after a forward seek, where the
    ///   position jumps to the target and then keeps climbing. Here the source
    ///   is held back from the sink entirely, which freezes the clock exactly.
    /// * **Mid-track underrun.** The same silence padding would silently eat
    ///   seconds of the track during a network stall. The source is already
    ///   playing by then, so pausing the sink is the only way to freeze the
    ///   clock.
    pub fn poll_gate(&mut self) {
        if !self.loaded {
            return;
        }

        let buffered = self.buffered_samples();
        let producer_done = self.buffer.as_ref().is_none_or(|b| b.is_done());

        if self.gated {
            // Measure *starvation*, not how long the gate has been shut. A
            // slow stream that trickles samples in is making progress and must
            // not be mistaken for a dead one, so any data at all resets the
            // clock. Only a completely dry buffer counts.
            self.gated_for = if buffered > 0 {
                Duration::ZERO
            } else {
                self.gated_for + POLL_STEP
            };

            // A short track may never reach the pre-roll; once the producer is
            // done, play out whatever did arrive rather than waiting forever.
            let ready = buffered >= self.buffer_target || producer_done;
            if ready {
                self.open_gate();
            }
            return;
        }

        // Mid-track underrun: shut the gate rather than let `PcmSource` pad
        // the gap with silence. `producer_done` is excluded because an empty
        // buffer at EOF is the normal end of a track, not a stall.
        //
        // The source is already in the sink by now, so pausing is the only way
        // to freeze the clock. rodio applies it on its next control tick, which
        // costs a few milliseconds of drift — invisible mid-track, unlike the
        // unbounded drift of letting the sink chew through silence.
        if buffered == 0 && !producer_done {
            self.gated = true;
            self.gated_for = Duration::ZERO;
            self.sink.pause();
        }
    }

    /// Hand the pending source to the sink and start playing.
    fn open_gate(&mut self) {
        self.gated = false;
        self.gated_for = Duration::ZERO;

        if let Some(source) = self.pending.take() {
            self.sink.append(source);
        }
        if !self.paused {
            self.sink.play();
        }
    }

    /// Stop playback and release the current source.
    pub fn stop(&mut self) {
        self.teardown();
        // Replace the sink — a stopped sink can't be reused for new audio.
        if let Ok(sink) = rodio::Sink::try_new(&self.stream_handle) {
            sink.set_volume(self.volume.as_f32());
            self.sink = sink;
        }
        self.loaded = false;
        self.position_offset = Duration::ZERO;
        self.buffer_target = 0;
        self.gated = false;
        self.gated_for = Duration::ZERO;
        self.pending = None;
        self.paused = false;
        if let Ok(mut sp) = self.spectrum.lock() {
            sp.reset();
        }
    }

    /// Cancel the producer thread and drop the ring buffer.
    fn teardown(&mut self) {
        if let Some(buffer) = self.buffer.take() {
            buffer.cancel();
            buffer.clear();
        }
        // A source that never reached the sink must not leak into the next
        // track — it still holds a reader on the old pipeline.
        self.pending = None;
        self.sink.stop();
    }

    /// Begin playing raw PCM from `source`.
    ///
    /// `position_offset` is how far into the track this source starts, so a
    /// seek can be reported as a continuous timeline.
    ///
    /// The sink starts gated: paused until [`AudioEngine::poll_gate`] sees a
    /// pre-roll's worth of audio. Without that, rodio burns through the silence
    /// `PcmSource` pads with while ffmpeg renders, and the position runs ahead
    /// of the sound.
    pub fn play_pcm(
        &mut self,
        source: impl Read + Send + 'static,
        sample_rate: u32,
        channels: u16,
        position_offset: Duration,
    ) {
        self.teardown();

        let sink = match rodio::Sink::try_new(&self.stream_handle) {
            Ok(sink) => sink,
            Err(e) => {
                eprintln!("[audio] failed to create sink: {e}");
                return;
            }
        };
        sink.set_volume(self.volume.as_f32());

        // ~2 seconds at 48kHz stereo. Big enough to absorb a network hiccup,
        // small enough that seeking and skipping feel instant.
        const CAPACITY: usize = 96_000 * 2;
        let buffer = Arc::new(RingBuffer::new(CAPACITY));

        {
            let buffer = buffer.clone();
            let eq = self.eq.clone();
            let spectrum = self.spectrum.clone();

            std::thread::spawn(move || {
                produce(source, buffer, eq, spectrum);
            });
        }

        self.sink = sink;
        // Deliberately *not* appended yet. The sink is empty until the gate
        // opens, so rodio has nothing to pull and the clock cannot move while
        // the stream is being prepared.
        self.pending = Some(PcmSource {
            buffer: buffer.clone(),
            sample_rate,
            channels,
        });
        self.buffer = Some(buffer);
        self.position_offset = position_offset;
        self.loaded = true;

        // Never ask for more than the buffer can hold, or the gate would only
        // open on a full buffer — which a slow stream might never reach.
        self.buffer_target = preroll_for(sample_rate, channels).min(CAPACITY);
        self.gated = true;
        self.gated_for = Duration::ZERO;
        self.paused = false;
    }

    /// Current playback position within the track.
    ///
    /// rodio counts the silence we insert during a buffer underrun, so this
    /// tracks wall-clock playback rather than decoded audio. For a progress bar
    /// that's the behaviour you want.
    ///
    /// While the pre-roll gate is shut the sink is paused, which freezes
    /// rodio's counter — so the position holds at the seek target instead of
    /// running ahead of audio that hasn't been rendered yet.
    pub fn position(&self) -> Duration {
        if !self.loaded {
            return Duration::ZERO;
        }
        self.position_offset + self.sink.get_pos()
    }

    /// True once the track has played to the end.
    ///
    /// The sink empties exactly when our source returns `None`, which only
    /// happens after the producer has read EOF from ffmpeg.
    ///
    /// A gated track is never finished, even though its sink is empty — the
    /// source is still waiting to be handed over, not drained.
    pub fn is_finished(&self) -> bool {
        self.loaded
            && !self.gated
            && self.pending.is_none()
            && self.sink.empty()
            && self.buffer.as_ref().is_none_or(|b| b.is_done())
    }

    /// How many samples are buffered but not yet played. Used by the UI to
    /// show a "buffering" indicator.
    pub fn buffered_samples(&self) -> usize {
        self.buffer.as_ref().map_or(0, |b| b.len())
    }
}

/// Read the pipe, run EQ and spectrum analysis, feed the ring buffer.
fn produce(
    mut source: impl Read,
    buffer: Arc<RingBuffer>,
    eq: Arc<Mutex<Equalizer>>,
    spectrum: Arc<Mutex<SpectrumAnalyzer>>,
) {
    // 8 KiB ≈ 2048 stereo frames ≈ 43ms at 48kHz.
    let mut raw = [0u8; 8192];
    let mut scratch: Vec<f32> = Vec::with_capacity(raw.len() / 2);

    loop {
        if buffer.is_cancelled() {
            break;
        }

        match source.read(&mut raw) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                scratch.clear();
                let frames = n / 2;

                // One lock acquisition per chunk rather than per sample.
                {
                    let mut eq = match eq.lock() {
                        Ok(eq) => eq,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                    let mut sp = match spectrum.lock() {
                        Ok(sp) => sp,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                    for i in 0..frames {
                        let sample = i16::from_le_bytes([raw[i * 2], raw[i * 2 + 1]]) as f32
                            / i16::MAX as f32;
                        let processed = eq.process(sample);
                        sp.push_sample(processed);
                        scratch.push(processed);
                    }
                }

                if !buffer.push(&scratch) {
                    break;
                }
            }
        }
    }

    buffer.mark_done();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// A live engine, or `None` when this machine has no audio device — CI
    /// runners usually have none, and a missing sound card shouldn't fail a
    /// logic test. Tests that need the device return early on `None`.
    fn engine() -> Option<AudioEngine> {
        AudioEngine::new().ok()
    }

    #[test]
    fn ring_buffer_push_pop_round_trip() {
        let buf = RingBuffer::new(16);
        assert!(buf.push(&[1.0, 2.0, 3.0]));
        assert_eq!(buf.len(), 3);
        assert_eq!(buf.pop(), Some(1.0));
        assert_eq!(buf.pop(), Some(2.0));
        assert_eq!(buf.pop(), Some(3.0));
        assert_eq!(buf.pop(), None);
    }

    #[test]
    fn ring_buffer_blocks_when_full_and_resumes_after_drain() {
        let buf = Arc::new(RingBuffer::new(4));
        assert!(buf.push(&[1.0, 2.0, 3.0, 4.0]));

        // Fill it from another thread — this push must block until we drain.
        let producer = {
            let buf = buf.clone();
            std::thread::spawn(move || buf.push(&[5.0]))
        };

        std::thread::sleep(Duration::from_millis(50));
        assert!(!producer.is_finished(), "producer should be blocked");

        buf.pop();
        buf.pop();

        // Wait for the blocked push to land.
        for _ in 0..100 {
            if producer.is_finished() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(producer.is_finished(), "producer should have unblocked");
    }

    #[test]
    fn ring_buffer_cancel_unblocks_producer() {
        let buf = Arc::new(RingBuffer::new(2));
        assert!(buf.push(&[1.0, 2.0]));

        let producer = {
            let buf = buf.clone();
            std::thread::spawn(move || buf.push(&[3.0]))
        };

        std::thread::sleep(Duration::from_millis(30));
        buf.cancel();

        for _ in 0..100 {
            if producer.is_finished() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(producer.is_finished(), "cancel should release the producer");
        assert!(!buf.push(&[4.0]), "cancelled buffer refuses pushes");
    }

    #[test]
    fn ring_buffer_marks_done() {
        let buf = RingBuffer::new(8);
        assert!(!buf.is_done());
        buf.mark_done();
        assert!(buf.is_done());
    }

    #[test]
    fn producer_marks_done_at_eof() {
        let buf = Arc::new(RingBuffer::new(1024));
        let eq = Arc::new(Mutex::new(Equalizer::new(48000)));
        let sp = Arc::new(Mutex::new(SpectrumAnalyzer::new(8)));

        // 4 frames of silence, s16le stereo.
        let pcm = vec![0u8; 16];
        produce(Cursor::new(pcm), buf.clone(), eq, sp);

        assert!(buf.is_done());
        assert_eq!(buf.len(), 8, "4 stereo frames = 8 f32 samples");
    }

    #[test]
    fn producer_stops_early_when_cancelled() {
        let buf = Arc::new(RingBuffer::new(1024));
        buf.cancel();
        let eq = Arc::new(Mutex::new(Equalizer::new(48000)));
        let sp = Arc::new(Mutex::new(SpectrumAnalyzer::new(8)));

        let pcm = vec![0u8; 1024];
        produce(Cursor::new(pcm), buf.clone(), eq, sp);

        assert_eq!(
            buf.len(),
            0,
            "cancelled producer should not fill the buffer"
        );
        assert!(buf.is_done());
    }

    #[test]
    fn pcm_source_returns_silence_while_producer_runs() {
        let buf = Arc::new(RingBuffer::new(8));
        let mut src = PcmSource {
            buffer: buf.clone(),
            sample_rate: 48000,
            channels: 2,
        };

        // Nothing buffered and the producer is alive — pad with silence
        // instead of ending the track.
        assert_eq!(src.next(), Some(0.0));

        buf.push(&[0.5]);
        assert_eq!(src.next(), Some(0.5));
    }

    #[test]
    fn pcm_source_ends_when_producer_is_done() {
        let buf = Arc::new(RingBuffer::new(8));
        let mut src = PcmSource {
            buffer: buf.clone(),
            sample_rate: 48000,
            channels: 2,
        };
        buf.mark_done();
        assert_eq!(src.next(), None);
    }

    #[test]
    fn pcm_source_reports_format() {
        let buf = Arc::new(RingBuffer::new(8));
        let src = PcmSource {
            buffer: buf,
            sample_rate: 44100,
            channels: 1,
        };
        assert_eq!(src.sample_rate(), 44100);
        assert_eq!(src.channels(), 1);
        assert_eq!(src.total_duration(), None);
        assert_eq!(src.current_frame_len(), None);
    }

    #[test]
    fn volume_round_trips_through_the_engine() {
        let Some(mut e) = engine() else { return };
        e.set_volume(Volume::new(0.25));
        assert_eq!(e.volume().as_f32(), 0.25);
    }

    #[test]
    fn fresh_engine_reports_zero_position_and_not_finished() {
        let Some(e) = engine() else { return };
        assert_eq!(e.position(), Duration::ZERO);
        assert!(!e.is_finished());
        assert_eq!(e.buffered_samples(), 0);
    }

    #[test]
    fn playing_pcm_sets_position_offset() {
        let Some(mut e) = engine() else { return };
        let pcm = Cursor::new(vec![0u8; 4096]);
        e.play_pcm(pcm, 48000, 2, Duration::from_secs(90));

        // The producer runs on its own thread, so wait for it to land rather
        // than assuming it has already pushed by the time we look.
        for _ in 0..200 {
            if e.buffered_samples() > 0 || e.is_finished() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            e.buffered_samples() > 0 || e.is_finished(),
            "producer never delivered any samples"
        );

        // Position starts at the seek offset and only grows from there.
        assert!(e.position() >= Duration::from_secs(90));
    }

    #[test]
    fn stop_resets_position() {
        let Some(mut e) = engine() else { return };
        let pcm = Cursor::new(vec![0u8; 4096]);
        e.play_pcm(pcm, 48000, 2, Duration::from_secs(30));
        e.stop();
        assert_eq!(e.position(), Duration::ZERO);
        assert!(!e.is_finished());
    }

    #[test]
    fn finished_after_a_short_source_drains() {
        let Some(mut e) = engine() else { return };
        // A tiny source: the producer hits EOF almost immediately.
        let pcm = Cursor::new(vec![0u8; 32]);
        e.play_pcm(pcm, 48000, 2, Duration::ZERO);

        for _ in 0..200 {
            // The real audio thread drives the gate on every poll; without it
            // the sink stays paused and the track never drains.
            e.poll_gate();
            if e.is_finished() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("engine never reported the track as finished");
    }

    /// A source that blocks until released, so a test can hold the buffer dry.
    ///
    /// Stands in for yt-dlp still resolving and ffmpeg still rendering: the
    /// pipe exists, but no PCM has come out of it yet.
    struct BlockingSource {
        data: Cursor<Vec<u8>>,
        gate: Arc<(Mutex<bool>, Condvar)>,
    }

    impl BlockingSource {
        fn new(data: Vec<u8>) -> (Self, Arc<(Mutex<bool>, Condvar)>) {
            let gate = Arc::new((Mutex::new(false), Condvar::new()));
            (
                Self {
                    data: Cursor::new(data),
                    gate: gate.clone(),
                },
                gate,
            )
        }
    }

    impl Read for BlockingSource {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            // Block "on the network" until the test releases us.
            let (lock, cvar) = &*self.gate;
            let mut open = lock.lock().unwrap();
            while !*open {
                open = cvar.wait(open).unwrap();
            }
            drop(open);
            self.data.read(buf)
        }
    }

    /// Release a [`BlockingSource`] so its producer can finish.
    fn release(gate: &Arc<(Mutex<bool>, Condvar)>) {
        let (lock, cvar) = &**gate;
        *lock.lock().unwrap() = true;
        cvar.notify_all();
    }

    #[test]
    fn sink_stays_gated_until_the_preroll_lands() {
        let Some(mut e) = engine() else { return };
        // 4 seconds of silence: more than the 400ms pre-roll.
        let pcm = Cursor::new(vec![0u8; 48_000 * 2 * 2 * 4]);
        e.play_pcm(pcm, 48000, 2, Duration::ZERO);

        assert!(e.is_gated(), "a fresh sink must start gated");

        // Nothing has been rendered yet, so the position must not move even
        // though the sink exists and the clock is running.
        let before = e.position();
        std::thread::sleep(Duration::from_millis(120));
        assert_eq!(
            e.position(),
            before,
            "the clock must not advance while the pre-roll is outstanding"
        );

        // Wait for the producer, then open the gate the way the audio thread
        // does.
        for _ in 0..200 {
            e.poll_gate();
            if !e.is_gated() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            !e.is_gated(),
            "the gate should open once the pre-roll lands"
        );
    }

    #[test]
    fn forward_seek_does_not_move_the_clock_while_buffering() {
        let Some(mut e) = engine() else { return };
        // A source that delivers nothing until released, standing in for
        // yt-dlp still resolving and ffmpeg still rendering.
        let (source, gate) = BlockingSource::new(vec![0u8; 48_000 * 2 * 2]);

        e.play_pcm(source, 48000, 2, Duration::from_secs(120));

        // This is the reported bug: seeking forward set the position to the
        // target and the clock then ran on while the stream was still being
        // prepared, so the display raced ahead of the audio.
        let target = Duration::from_secs(120);
        for _ in 0..8 {
            e.poll_gate();
            assert_eq!(
                e.position(),
                target,
                "position ran ahead of the audio during a forward seek"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(e.is_gated(), "a dry stream must keep the gate shut");

        // Release the source — the gate should now open and time may advance.
        release(&gate);
        for _ in 0..200 {
            e.poll_gate();
            if !e.is_gated() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!e.is_gated(), "gate never opened after data arrived");
    }

    #[test]
    fn gate_reports_starvation_only_while_completely_dry() {
        let Some(mut e) = engine() else { return };
        let (source, gate) = BlockingSource::new(vec![0u8; 48_000 * 2 * 2]);
        e.play_pcm(source, 48000, 2, Duration::ZERO);

        // Nothing has arrived: the starvation clock runs.
        for _ in 0..5 {
            e.poll_gate();
        }
        assert!(
            e.gated_for() > Duration::ZERO,
            "a completely dry stream must accumulate starvation time"
        );
        release(&gate);
    }

    #[test]
    fn gated_engine_reports_a_frozen_position() {
        let Some(mut e) = engine() else { return };
        let (source, gate) = BlockingSource::new(vec![0u8; 4096]);
        e.play_pcm(source, 48000, 2, Duration::from_secs(45));

        assert!(e.is_gated());
        assert_eq!(e.position(), Duration::from_secs(45));
        // `is_finished` must stay false — the track is buffering, not over.
        assert!(!e.is_finished());
        release(&gate);
    }

    #[test]
    fn stop_clears_the_gate() {
        let Some(mut e) = engine() else { return };
        let pcm = Cursor::new(vec![0u8; 4096]);
        e.play_pcm(pcm, 48000, 2, Duration::ZERO);
        assert!(e.is_gated());

        e.stop();
        assert!(!e.is_gated(), "stop must not leave the gate armed");
        assert_eq!(e.gated_for(), Duration::ZERO);
    }

    #[test]
    fn pause_holds_the_gate_shut_until_resumed() {
        let Some(mut e) = engine() else { return };
        // Long enough to satisfy the pre-roll immediately.
        let pcm = Cursor::new(vec![0u8; 48_000 * 2 * 2 * 4]);
        e.play_pcm(pcm, 48000, 2, Duration::ZERO);

        e.pause();
        for _ in 0..200 {
            e.poll_gate();
            if e.buffered_samples() >= 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }

        // The gate may have opened internally, but the user's pause must win:
        // resuming is what actually restarts the sink.
        assert!(e.is_paused());
        e.resume();
        assert!(!e.is_paused());
    }

    #[test]
    fn pause_and_resume_are_reflected() {
        let Some(mut e) = engine() else { return };
        let pcm = Cursor::new(vec![0u8; 4096]);
        e.play_pcm(pcm, 48000, 2, Duration::ZERO);

        assert!(!e.is_paused());
        e.pause();
        assert!(e.is_paused());
        e.resume();
        assert!(!e.is_paused());
    }

    #[test]
    fn preroll_is_bounded_by_the_ring_buffer_capacity() {
        // A target larger than the buffer would never be reached, leaving the
        // gate shut forever on a slow stream.
        let capacity = 96_000 * 2;
        assert!(preroll_for(48000, 2) <= capacity);
        assert!(preroll_for(192_000, 2) <= capacity);
    }

    #[test]
    fn preroll_scales_with_the_sample_rate() {
        assert!(preroll_for(48_000, 2) > preroll_for(24_000, 2));
        assert!(preroll_for(48_000, 2) > preroll_for(48_000, 1));
        // Degenerate input must still ask for something.
        assert!(preroll_for(0, 0) >= 1);
    }
}
