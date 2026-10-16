/// 10-band peaking EQ built from cascaded biquad filters.
///
/// Bands follow the classic Winamp frequencies. Each is ±12 dB, processed at
/// the engine's 48 kHz sample rate.
use std::f64::consts::PI;

pub const EQ_BANDS: [f64; EQ_NUM_BANDS] = [
    60.0, 170.0, 310.0, 600.0, 1000.0, 3000.0, 6000.0, 12000.0, 14000.0, 16000.0,
];
pub const EQ_NUM_BANDS: usize = 10;
pub const EQ_MIN_DB: f32 = -12.0;
pub const EQ_MAX_DB: f32 = 12.0;

/// Short labels for the slider row, matching how hardware EQs print them.
pub fn band_label(index: usize) -> String {
    match EQ_BANDS.get(index) {
        Some(&freq) if freq >= 1000.0 => format!("{:.0}K", freq / 1000.0),
        Some(&freq) => format!("{freq:.0}"),
        None => String::new(),
    }
}

/// A single peaking-EQ biquad in transposed direct form II.
#[derive(Debug, Clone, Copy)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    /// Identity filter — passes the signal through untouched.
    fn bypass() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    /// RBJ cookbook peaking EQ.
    fn peaking(sample_rate: f64, freq: f64, gain_db: f64, q: f64) -> Self {
        let a = 10.0_f64.powf(gain_db / 40.0);
        let w0 = 2.0 * PI * (freq / sample_rate).min(0.49);
        let (sin_w0, cos_w0) = w0.sin_cos();
        let alpha = sin_w0 / (2.0 * q);

        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cos_w0;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha / a;

        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// A named band configuration.
#[derive(Debug, Clone, Copy)]
pub struct Preset {
    pub name: &'static str,
    pub gains: [f32; EQ_NUM_BANDS],
}

pub const PRESETS: &[Preset] = &[
    Preset {
        name: "Flat",
        gains: [0.0; EQ_NUM_BANDS],
    },
    Preset {
        name: "Bass Boost",
        gains: [8.0, 7.0, 5.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    },
    Preset {
        name: "Bass Cut",
        gains: [-8.0, -7.0, -5.0, -2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    },
    Preset {
        name: "Treble Boost",
        gains: [0.0, 0.0, 0.0, 0.0, 0.0, 2.0, 4.0, 6.0, 7.0, 7.0],
    },
    Preset {
        name: "Vocal",
        gains: [-3.0, -2.0, 0.0, 3.0, 5.0, 5.0, 3.0, 1.0, 0.0, -1.0],
    },
    Preset {
        name: "Rock",
        gains: [5.0, 4.0, 2.0, -1.0, -2.0, 0.0, 2.0, 4.0, 5.0, 5.0],
    },
    Preset {
        name: "Pop",
        gains: [-1.0, 0.0, 2.0, 4.0, 4.0, 2.0, 0.0, -1.0, -1.0, -2.0],
    },
    Preset {
        name: "Electronic",
        gains: [6.0, 5.0, 1.0, 0.0, -2.0, 2.0, 1.0, 1.0, 5.0, 6.0],
    },
    Preset {
        name: "Loudness",
        gains: [6.0, 4.0, 0.0, -2.0, -3.0, -2.0, 0.0, 3.0, 5.0, 6.0],
    },
];

/// The 10-band equalizer.
#[derive(Debug, Clone)]
pub struct Equalizer {
    bands: [f32; EQ_NUM_BANDS],
    preamp_db: f32,
    enabled: bool,
    filters: [Biquad; EQ_NUM_BANDS],
    sample_rate: f64,
    /// Cached preamp gain so we don't call `powf` per sample.
    preamp_gain: f64,
}

impl Equalizer {
    pub fn new(sample_rate: u32) -> Self {
        let mut eq = Self {
            bands: [0.0; EQ_NUM_BANDS],
            preamp_db: 0.0,
            // Off by default: an unmodified EQ shouldn't colour the sound.
            enabled: false,
            filters: [Biquad::bypass(); EQ_NUM_BANDS],
            sample_rate: sample_rate.max(1) as f64,
            preamp_gain: 1.0,
        };
        eq.rebuild();
        eq
    }

    pub fn set_band(&mut self, index: usize, db: f32) {
        if index < EQ_NUM_BANDS {
            let clamped = db.clamp(EQ_MIN_DB, EQ_MAX_DB);
            if (self.bands[index] - clamped).abs() > f32::EPSILON {
                self.bands[index] = clamped;
                self.rebuild();
            }
        }
    }

    pub fn band(&self, index: usize) -> f32 {
        self.bands.get(index).copied().unwrap_or(0.0)
    }

    pub fn bands(&self) -> &[f32; EQ_NUM_BANDS] {
        &self.bands
    }

    pub fn set_preamp(&mut self, db: f32) {
        let clamped = db.clamp(EQ_MIN_DB, EQ_MAX_DB);
        if (self.preamp_db - clamped).abs() > f32::EPSILON {
            self.preamp_db = clamped;
            self.preamp_gain = 10.0_f64.powf(clamped as f64 / 20.0);
        }
    }

    pub fn preamp(&self) -> f32 {
        self.preamp_db
    }

    pub fn set_enabled(&mut self, on: bool) {
        if self.enabled != on {
            self.enabled = on;
            // Stale filter state from before the toggle would click.
            self.reset_filters();
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn toggle(&mut self) -> bool {
        self.set_enabled(!self.enabled);
        self.enabled
    }

    /// Normalised band values for drawing (0.0 = -12dB, 1.0 = +12dB).
    pub fn normalized_bands(&self) -> [f32; EQ_NUM_BANDS] {
        let range = EQ_MAX_DB - EQ_MIN_DB;
        std::array::from_fn(|i| (self.bands[i] - EQ_MIN_DB) / range)
    }

    /// Normalised preamp, same scale as the bands.
    pub fn normalized_preamp(&self) -> f32 {
        (self.preamp_db - EQ_MIN_DB) / (EQ_MAX_DB - EQ_MIN_DB)
    }

    /// True when every band sits at 0 dB.
    pub fn is_flat(&self) -> bool {
        self.bands.iter().all(|db| db.abs() < f32::EPSILON)
    }

    /// Name of the preset matching the current bands, if any.
    pub fn matching_preset(&self) -> Option<&'static str> {
        PRESETS
            .iter()
            .find(|p| {
                p.gains
                    .iter()
                    .zip(self.bands.iter())
                    .all(|(a, b)| (a - b).abs() < 0.01)
            })
            .map(|p| p.name)
    }

    pub fn apply_preset(&mut self, preset: &Preset) {
        self.bands = preset.gains;
        self.rebuild();
        self.reset_filters();
    }

    /// Apply a preset by name. Returns false if the name is unknown.
    pub fn apply_preset_by_name(&mut self, name: &str) -> bool {
        match PRESETS.iter().find(|p| p.name.eq_ignore_ascii_case(name)) {
            Some(preset) => {
                self.apply_preset(preset);
                true
            }
            None => false,
        }
    }

    /// Process one sample through preamp and every band.
    pub fn process(&mut self, sample: f32) -> f32 {
        // A bypassed EQ must be truly transparent, not a round-trip through
        // f32→f64→f32 with unity filters.
        if !self.enabled && (self.preamp_db - 0.0).abs() < f32::EPSILON {
            return sample;
        }

        let mut s = sample as f64 * self.preamp_gain;
        if self.enabled {
            for filter in &mut self.filters {
                s = filter.process(s);
            }
        }
        // Clamp rather than wrap — a boosted band can push past ±1.0.
        (s as f32).clamp(-1.0, 1.0)
    }

    fn rebuild(&mut self) {
        for (i, filter) in self.filters.iter_mut().enumerate() {
            let gain = self.bands[i] as f64;
            *filter = if gain.abs() < 0.01 {
                // A 0 dB band is a no-op — skip the multiply entirely.
                Biquad::bypass()
            } else {
                // Wider Q at the low end keeps bass adjustments musical;
                // narrower up top avoids harshness on the high bands.
                let q = if EQ_BANDS[i] < 250.0 {
                    0.7
                } else if EQ_BANDS[i] < 2000.0 {
                    1.0
                } else {
                    1.2
                };
                Biquad::peaking(self.sample_rate, EQ_BANDS[i], gain, q)
            };
        }
    }

    fn reset_filters(&mut self) {
        for filter in &mut self.filters {
            filter.reset();
        }
    }
}

impl Default for Equalizer {
    fn default() -> Self {
        Self::new(48000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f64, sample_rate: f64, len: usize) -> Vec<f32> {
        // Quarter amplitude — a full-scale sine would clip against the EQ's
        // output limiter and hide the very gain changes we're measuring.
        (0..len)
            .map(|i| 0.25 * (2.0 * PI * freq * i as f64 / sample_rate).sin() as f32)
            .collect()
    }

    /// Peak amplitude of the second half of the signal, after the filters have
    /// settled. The first half is skipped so transient ringing doesn't skew it.
    fn settled_peak(samples: &[f32]) -> f32 {
        samples[samples.len() / 2..]
            .iter()
            .fold(0.0_f32, |m, s| m.max(s.abs()))
    }

    #[test]
    fn band_labels_format_correctly() {
        assert_eq!(band_label(0), "60");
        assert_eq!(band_label(4), "1K");
        assert_eq!(band_label(9), "16K");
        assert_eq!(band_label(99), "");
    }

    #[test]
    fn starts_flat_and_disabled() {
        let eq = Equalizer::new(48000);
        assert!(eq.is_flat());
        assert!(!eq.is_enabled());
        assert_eq!(eq.preamp(), 0.0);
        assert!(eq.bands().iter().all(|&b| b == 0.0));
    }

    #[test]
    fn disabled_eq_is_bit_transparent() {
        let mut eq = Equalizer::new(48000);
        for &s in &[-1.0_f32, -0.5, 0.0, 0.25, 1.0] {
            assert_eq!(eq.process(s), s);
        }
    }

    #[test]
    fn preamp_applies_even_when_eq_is_off() {
        let mut eq = Equalizer::new(48000);
        eq.set_preamp(6.0);
        // +6 dB ≈ 2x
        let out = eq.process(0.25);
        assert!((out - 0.5).abs() < 0.01, "expected ~0.5, got {out}");
    }

    #[test]
    fn output_is_clamped_to_unit_range() {
        let mut eq = Equalizer::new(48000);
        eq.set_enabled(true);
        eq.set_preamp(12.0);
        for i in 0..EQ_NUM_BANDS {
            eq.set_band(i, 12.0);
        }
        let out = eq.process(1.0);
        assert!((-1.0..=1.0).contains(&out), "output escaped range: {out}");
    }

    #[test]
    fn band_gains_are_clamped() {
        let mut eq = Equalizer::new(48000);
        eq.set_band(0, 999.0);
        assert_eq!(eq.band(0), EQ_MAX_DB);
        eq.set_band(0, -999.0);
        assert_eq!(eq.band(0), EQ_MIN_DB);
    }

    #[test]
    fn out_of_range_band_index_is_ignored() {
        let mut eq = Equalizer::new(48000);
        eq.set_band(99, 6.0);
        assert_eq!(eq.band(99), 0.0);
        assert!(eq.is_flat());
    }

    #[test]
    fn boost_raises_the_target_frequency() {
        let sample_rate = 48000.0;
        let input = sine(60.0, sample_rate, 9600);

        let mut flat = Equalizer::new(48000);
        flat.set_enabled(true);
        let baseline: Vec<f32> = input.iter().map(|&s| flat.process(s)).collect();

        let mut boosted = Equalizer::new(48000);
        boosted.set_enabled(true);
        boosted.set_band(0, 12.0); // 60 Hz
        let out: Vec<f32> = input.iter().map(|&s| boosted.process(s)).collect();

        assert!(
            settled_peak(&out) > settled_peak(&baseline) * 1.5,
            "60Hz boost should lift a 60Hz tone: {} vs {}",
            settled_peak(&out),
            settled_peak(&baseline)
        );
    }

    #[test]
    fn cut_lowers_the_target_frequency() {
        let sample_rate = 48000.0;
        let input = sine(60.0, sample_rate, 9600);

        let mut flat = Equalizer::new(48000);
        flat.set_enabled(true);
        let baseline: Vec<f32> = input.iter().map(|&s| flat.process(s)).collect();

        let mut cut = Equalizer::new(48000);
        cut.set_enabled(true);
        cut.set_band(0, -12.0);
        let out: Vec<f32> = input.iter().map(|&s| cut.process(s)).collect();

        assert!(
            settled_peak(&out) < settled_peak(&baseline),
            "60Hz cut should attenuate a 60Hz tone"
        );
    }

    #[test]
    fn boosting_one_band_leaves_far_away_frequencies_alone() {
        let sample_rate = 48000.0;
        // A 6kHz tone shouldn't be much affected by a 60Hz boost.
        let input = sine(6000.0, sample_rate, 9600);

        let mut flat = Equalizer::new(48000);
        flat.set_enabled(true);
        let baseline: Vec<f32> = input.iter().map(|&s| flat.process(s)).collect();

        let mut boosted = Equalizer::new(48000);
        boosted.set_enabled(true);
        boosted.set_band(0, 12.0);
        let out: Vec<f32> = input.iter().map(|&s| boosted.process(s)).collect();

        let ratio = settled_peak(&out) / settled_peak(&baseline).max(1e-6);
        assert!(
            (0.8..1.25).contains(&ratio),
            "unrelated band changed by {ratio}x"
        );
    }

    #[test]
    fn toggling_resets_filter_state() {
        let mut eq = Equalizer::new(48000);
        eq.set_enabled(true);
        eq.set_band(0, 12.0);
        // Push some signal through so the filters hold state.
        for i in 0..1000 {
            eq.process((i as f32 * 0.01).sin());
        }
        eq.set_enabled(false);
        assert!(!eq.is_enabled());
        // Disabled again — back to transparent.
        assert_eq!(eq.process(0.5), 0.5);
    }

    #[test]
    fn toggle_flips_state() {
        let mut eq = Equalizer::new(48000);
        assert!(eq.toggle());
        assert!(!eq.toggle());
    }

    #[test]
    fn normalized_bands_span_zero_to_one() {
        let mut eq = Equalizer::new(48000);
        eq.set_band(0, EQ_MIN_DB);
        eq.set_band(1, EQ_MAX_DB);
        let norm = eq.normalized_bands();
        assert!((norm[0] - 0.0).abs() < 1e-6);
        assert!((norm[1] - 1.0).abs() < 1e-6);
        assert!(norm.iter().all(|v| (0.0..=1.0).contains(v)));
    }

    #[test]
    fn normalized_preamp_spans_zero_to_one() {
        let mut eq = Equalizer::new(48000);
        eq.set_preamp(EQ_MIN_DB);
        assert!((eq.normalized_preamp() - 0.0).abs() < 1e-6);
        eq.set_preamp(EQ_MAX_DB);
        assert!((eq.normalized_preamp() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn applying_a_preset_changes_the_bands() {
        let mut eq = Equalizer::new(48000);
        assert!(eq.apply_preset_by_name("Bass Boost"));
        assert!(!eq.is_flat());
        assert_eq!(eq.band(0), 8.0);
        assert_eq!(eq.matching_preset(), Some("Bass Boost"));
    }

    #[test]
    fn unknown_preset_name_returns_false() {
        let mut eq = Equalizer::new(48000);
        assert!(!eq.apply_preset_by_name("Nonexistent"));
        assert!(eq.is_flat());
    }

    #[test]
    fn preset_lookup_is_case_insensitive() {
        let mut eq = Equalizer::new(48000);
        assert!(eq.apply_preset_by_name("bass boost"));
    }

    #[test]
    fn flat_preset_is_detected() {
        let mut eq = Equalizer::new(48000);
        eq.apply_preset_by_name("Rock");
        eq.apply_preset_by_name("Flat");
        assert!(eq.is_flat());
        assert_eq!(eq.matching_preset(), Some("Flat"));
    }

    #[test]
    fn custom_settings_match_no_preset() {
        let mut eq = Equalizer::new(48000);
        eq.set_band(3, 5.5);
        assert_eq!(eq.matching_preset(), None);
    }

    #[test]
    fn every_preset_is_within_range_and_named() {
        for preset in PRESETS {
            assert!(!preset.name.is_empty());
            for &gain in &preset.gains {
                assert!(
                    (EQ_MIN_DB..=EQ_MAX_DB).contains(&gain),
                    "preset '{}' has an out-of-range gain: {gain}",
                    preset.name
                );
            }
        }
    }

    #[test]
    fn preset_names_are_unique() {
        for (i, a) in PRESETS.iter().enumerate() {
            for b in PRESETS.iter().skip(i + 1) {
                assert_ne!(a.name, b.name);
            }
        }
    }

    #[test]
    fn every_preset_round_trips_through_the_eq() {
        let mut eq = Equalizer::new(48000);
        for preset in PRESETS {
            eq.apply_preset(preset);
            assert_eq!(eq.matching_preset(), Some(preset.name));
        }
    }

    #[test]
    fn output_stays_finite_under_heavy_boost() {
        let mut eq = Equalizer::new(48000);
        eq.set_enabled(true);
        eq.set_preamp(12.0);
        for i in 0..EQ_NUM_BANDS {
            eq.set_band(i, 12.0);
        }
        let input = sine(1000.0, 48000.0, 4800);
        for &s in &input {
            let out = eq.process(s);
            assert!(out.is_finite(), "EQ produced a non-finite sample");
            assert!((-1.0..=1.0).contains(&out));
        }
    }
}
