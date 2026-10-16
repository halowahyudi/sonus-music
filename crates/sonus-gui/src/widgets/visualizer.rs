use egui::{Color32, Painter, Pos2, Rect, Vec2};

/// Spectrum bar visualizer — classic Winamp analyzer.
/// Draws vertical bars from the bottom up, colored by height.
pub struct SpectrumBars {
    pub values: Vec<f32>,
    num_bars: usize,
}

impl SpectrumBars {
    pub fn new(num_bars: usize) -> Self {
        Self { values: vec![0.0; num_bars], num_bars }
    }

    /// Update from the audio engine's spectrum analyzer.
    pub fn update(&mut self, bars: &[f32]) {
        for i in 0..self.num_bars.min(bars.len()) {
            self.values[i] = self.values[i] * 0.5 + bars[i] * 0.5;
        }
    }

    /// Paint bars into the given rect. `color` is the bar color,
    /// `peak_color` is the peak hold line.
    pub fn paint(&self, painter: &Painter, rect: Rect, color: Color32, _peak_color: Color32) {
        let bar_w = rect.width() / self.num_bars as f32;
        let gap = bar_w * 0.2;
        let actual_w = bar_w - gap;
        let h = rect.height();

        for (i, &val) in self.values.iter().enumerate() {
            let bar_h = (val * h).min(h);
            let x = rect.left() + i as f32 * bar_w + gap / 2.0;
            let bar_rect = Rect::from_min_size(
                Pos2::new(x, rect.bottom() - bar_h),
                Vec2::new(actual_w, bar_h),
            );
            painter.rect_filled(bar_rect, 0.0, color);
        }
    }
}
