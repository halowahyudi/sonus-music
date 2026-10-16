use rustfft::num_complex::Complex;
use rustfft::FftPlanner;

/// Number of visualizer bars. 24 maps neatly onto the pixel grid.
pub const NUM_BARS: usize = 24;

/// FFT window size. 1024 samples at 48kHz is ~21ms — short enough that the
/// display reacts to transients, long enough to resolve bass frequencies.
const FFT_SIZE: usize = 1024;

/// Real-time spectrum analyzer for the visualizer.
///
/// Samples are fed in one at a time as they're decoded. Every `FFT_SIZE`
/// samples the buffer is transformed and reduced to `NUM_BARS` magnitudes.
pub struct SpectrumAnalyzer {
    fft: std::sync::Arc<dyn rustfft::Fft<f32>>,
    buffer: Vec<Complex<f32>>,
    pos: usize,
    /// Scratch space reused across transforms to avoid per-frame allocation.
    scratch: Vec<Complex<f32>>,
    bars: Vec<f32>,
    peaks: Vec<f32>,
    /// Smoothing state, applied per bar to stop the display flickering.
    smoothed: Vec<f32>,
}

impl SpectrumAnalyzer {
    pub fn new(num_bars: usize) -> Self {
        let num_bars = num_bars.max(1);
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        Self {
            fft,
            buffer: vec![Complex::new(0.0, 0.0); FFT_SIZE],
            pos: 0,
            scratch: vec![Complex::new(0.0, 0.0); FFT_SIZE],
            bars: vec![0.0; num_bars],
            peaks: vec![0.0; num_bars],
            smoothed: vec![0.0; num_bars],
        }
    }

    /// Feed one mono sample. Runs an FFT whenever the window fills up.
    pub fn push_sample(&mut self, sample: f32) {
        // Guard against NaN/Inf escaping the EQ into the FFT, which would
        // poison every subsequent frame.
        let sample = if sample.is_finite() { sample } else { 0.0 };
        self.buffer[self.pos] = Complex::new(sample, 0.0);
        self.pos += 1;
        if self.pos >= FFT_SIZE {
            self.pos = 0;
            self.compute();
        }
    }

    fn compute(&mut self) {
        self.scratch.copy_from_slice(&self.buffer);
        self.fft.process(&mut self.scratch);

        let num_bars = self.bars.len();
        let half = FFT_SIZE / 2;

        for (i, bar) in self.bars.iter_mut().enumerate() {
            // Log-spaced bands: linear FFT bins would cram every audible
            // frequency into the first few bars and leave the rest flat.
            let start = bin_for(i, num_bars, half);
            let end = bin_for(i + 1, num_bars, half).max(start + 1).min(half);

            let sum: f32 = self.scratch[start..end].iter().map(|c| c.norm_sqr()).sum();
            let rms = (sum / (end - start) as f32).sqrt();

            // Compress the dynamic range so quiet passages still register.
            let target = (rms * 6.0).sqrt().min(1.0);

            // Asymmetric smoothing: snap up for punch, decay slowly so bars
            // fall gracefully instead of strobing.
            let previous = self.smoothed[i];
            let value = if target > previous {
                previous * 0.4 + target * 0.6
            } else {
                previous * 0.82 + target * 0.18
            };
            self.smoothed[i] = value;
            *bar = value;
        }

        // Peak-hold markers fall slowly and never drop below the bar.
        for (peak, &bar) in self.peaks.iter_mut().zip(self.bars.iter()) {
            *peak = if bar >= *peak {
                bar
            } else {
                (*peak - 0.012).max(bar)
            };
        }
    }

    /// Bar magnitudes in `0.0..=1.0`.
    pub fn bars(&self) -> &[f32] {
        &self.bars
    }

    /// Peak-hold values for the falling marker above each bar.
    pub fn peaks(&self) -> &[f32] {
        &self.peaks
    }

    pub fn num_bars(&self) -> usize {
        self.bars.len()
    }

    pub fn reset(&mut self) {
        self.bars.fill(0.0);
        self.peaks.fill(0.0);
        self.smoothed.fill(0.0);
        self.buffer.fill(Complex::new(0.0, 0.0));
        self.pos = 0;
    }
}

/// Map bar index to an FFT bin, spacing bars logarithmically.
///
/// Bin index grows exponentially with the bar index so each bar covers a
/// roughly constant musical interval rather than a constant Hz range.
fn bin_for(bar: usize, num_bars: usize, half: usize) -> usize {
    if num_bars == 0 {
        return 0;
    }
    let t = bar as f32 / num_bars as f32;
    let min_bin = 1.0_f32;
    let max_bin = half as f32;
    (min_bin * (max_bin / min_bin).powf(t)) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn analyzer() -> SpectrumAnalyzer {
        SpectrumAnalyzer::new(NUM_BARS)
    }

    #[test]
    fn starts_silent() {
        let a = analyzer();
        assert_eq!(a.bars().len(), NUM_BARS);
        assert!(a.bars().iter().all(|&v| v == 0.0));
        assert!(a.peaks().iter().all(|&v| v == 0.0));
    }

    #[test]
    fn clamps_bar_count_to_at_least_one() {
        assert_eq!(SpectrumAnalyzer::new(0).num_bars(), 1);
    }

    #[test]
    fn silence_produces_no_energy() {
        let mut a = analyzer();
        for _ in 0..FFT_SIZE * 2 {
            a.push_sample(0.0);
        }
        assert!(
            a.bars().iter().all(|&v| v < 0.01),
            "silence should not register: {:?}",
            a.bars()
        );
    }

    #[test]
    fn a_loud_tone_produces_energy() {
        let mut a = analyzer();
        // 1kHz sine at 48kHz — a strong, unambiguous signal.
        for i in 0..FFT_SIZE * 4 {
            let t = i as f32 / 48000.0;
            a.push_sample((2.0 * PI * 1000.0 * t).sin() * 0.9);
        }
        let total: f32 = a.bars().iter().sum();
        assert!(total > 0.0, "a loud tone must produce visible bars");
        assert!(a.bars().iter().all(|&v| (0.0..=1.0).contains(&v)));
    }

    #[test]
    fn values_stay_in_range_for_extreme_input() {
        let mut a = analyzer();
        for i in 0..FFT_SIZE * 2 {
            a.push_sample(if i % 2 == 0 { 1.0 } else { -1.0 });
        }
        for &v in a.bars() {
            assert!((0.0..=1.0).contains(&v), "bar out of range: {v}");
        }
    }

    #[test]
    fn non_finite_samples_are_ignored() {
        let mut a = analyzer();
        a.push_sample(f32::NAN);
        a.push_sample(f32::INFINITY);
        a.push_sample(f32::NEG_INFINITY);
        for _ in 0..FFT_SIZE {
            a.push_sample(0.0);
        }
        assert!(
            a.bars().iter().all(|v| v.is_finite()),
            "NaN leaked into the bars"
        );
    }

    #[test]
    fn peaks_never_sit_below_their_bar() {
        let mut a = analyzer();
        for i in 0..FFT_SIZE * 4 {
            let t = i as f32 / 48000.0;
            a.push_sample((2.0 * PI * 440.0 * t).sin());
        }
        for (bar, peak) in a.bars().iter().zip(a.peaks().iter()) {
            assert!(peak >= bar, "peak {peak} below bar {bar}");
        }
    }

    #[test]
    fn peaks_decay_after_the_signal_stops() {
        let mut a = analyzer();
        for i in 0..FFT_SIZE * 4 {
            let t = i as f32 / 48000.0;
            a.push_sample((2.0 * PI * 440.0 * t).sin());
        }
        let peak_before: f32 = a.peaks().iter().sum();
        assert!(peak_before > 0.0);

        for _ in 0..FFT_SIZE * 20 {
            a.push_sample(0.0);
        }
        let peak_after: f32 = a.peaks().iter().sum();
        assert!(peak_after < peak_before, "peaks should fall during silence");
    }

    #[test]
    fn reset_clears_everything() {
        let mut a = analyzer();
        for i in 0..FFT_SIZE * 2 {
            let t = i as f32 / 48000.0;
            a.push_sample((2.0 * PI * 440.0 * t).sin());
        }
        a.reset();
        assert!(a.bars().iter().all(|&v| v == 0.0));
        assert!(a.peaks().iter().all(|&v| v == 0.0));
    }

    #[test]
    fn bin_spacing_is_monotonic_and_in_bounds() {
        let half = FFT_SIZE / 2;
        let mut previous = 0;
        for i in 0..=NUM_BARS {
            let bin = bin_for(i, NUM_BARS, half);
            assert!(bin >= previous, "bins must not go backwards");
            assert!(bin <= half, "bin {bin} past the Nyquist limit");
            previous = bin;
        }
    }

    #[test]
    fn bin_spacing_is_logarithmic() {
        let half = FFT_SIZE / 2;
        // The first band should be far narrower than the last — that's what
        // makes the display look right instead of bass-heavy.
        let first = bin_for(1, NUM_BARS, half) - bin_for(0, NUM_BARS, half);
        let last = bin_for(NUM_BARS, NUM_BARS, half) - bin_for(NUM_BARS - 1, NUM_BARS, half);
        assert!(last > first, "high bands should be wider than low bands");
    }

    #[test]
    fn bin_for_zero_bars_does_not_panic() {
        assert_eq!(bin_for(0, 0, 512), 0);
    }
}
