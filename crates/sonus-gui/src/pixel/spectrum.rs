//! Spectrum analyzer visualizer.

use egui::{Color32, Pos2, Rect, Vec2};

use super::paint;
use super::palette::Palette;

/// Classic bar-graph spectrum display.
///
/// Bars grow from the bottom and change color as they climb — green through
/// the accent at the top, like the LED meters on a real amplifier.
pub struct Spectrum {
    /// Smoothed bar magnitudes in `0.0..=1.0`.
    bars: Vec<f32>,
    /// Peak-hold markers, which fall more slowly than the bars.
    peaks: Vec<f32>,
    /// Display style.
    style: Style,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// Solid bars.
    Bars,
    /// Bars made of discrete blocks — chunkier, more retro.
    Blocks,
    /// A filled area with a bright top edge.
    Line,
}

impl Spectrum {
    pub fn new(num_bars: usize, style: Style) -> Self {
        let num_bars = num_bars.max(1);
        Self {
            bars: vec![0.0; num_bars],
            peaks: vec![0.0; num_bars],
            style,
        }
    }

    pub fn num_bars(&self) -> usize {
        self.bars.len()
    }

    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Feed the latest magnitudes from the analyzer.
    ///
    /// If the analyzer's bar count doesn't match, resample — that way changing
    /// the display resolution can't misalign the two.
    pub fn update(&mut self, bars: &[f32], peaks: &[f32]) {
        let n = self.bars.len();
        if bars.len() != n {
            for i in 0..n {
                self.bars[i] = resample(bars, i, n);
                self.peaks[i] = resample(peaks, i, n);
            }
        } else {
            self.bars.copy_from_slice(bars);
            for (peak, value) in self.peaks.iter_mut().zip(peaks.iter()) {
                *peak = *value;
            }
        }
    }

    pub fn reset(&mut self) {
        self.bars.fill(0.0);
        self.peaks.fill(0.0);
    }

    /// True when nothing is showing — lets the caller skip repaints.
    pub fn is_idle(&self) -> bool {
        self.bars.iter().all(|&v| v <= 0.001) && self.peaks.iter().all(|&v| v <= 0.001)
    }

    /// Draw into `rect`.
    pub fn paint(&self, painter: &egui::Painter, rect: Rect, palette: &Palette) {
        let rect = paint::snap_rect(rect);
        if rect.width() < 2.0 || rect.height() < 2.0 {
            return;
        }

        // Sunken glass backdrop.
        paint::surface(painter, rect, palette.lcd_bg, paint::Bevel::Sunken, palette);

        let inner = rect.shrink(2.0);
        if inner.width() < 1.0 || inner.height() < 1.0 {
            return;
        }

        match self.style {
            Style::Bars => self.paint_bars(painter, inner, palette, false),
            Style::Blocks => self.paint_bars(painter, inner, palette, true),
            Style::Line => self.paint_line(painter, inner, palette),
        }
    }

    fn paint_bars(&self, painter: &egui::Painter, inner: Rect, palette: &Palette, blocks: bool) {
        let n = self.bars.len();
        let slot = inner.width() / n as f32;
        // Leave a gap between bars so they stay distinct.
        let bar_width = (slot - 1.0).max(1.0);

        // Block height for the chunky style.
        let block_h = 3.0;

        for (i, &value) in self.bars.iter().enumerate() {
            let x = paint::snap(inner.left() + i as f32 * slot);
            let bar_h = paint::snap((value.clamp(0.0, 1.0) * inner.height()).max(0.0));
            if bar_h < 1.0 {
                continue;
            }

            let bar = Rect::from_min_size(
                Pos2::new(x, inner.bottom() - bar_h),
                Vec2::new(bar_width, bar_h),
            );

            if blocks {
                // Discrete LED blocks with a 1px gap between them.
                let count = (bar_h / block_h).ceil() as usize;
                for b in 0..count {
                    let y = inner.bottom() - (b + 1) as f32 * block_h;
                    let segment = Rect::from_min_size(
                        Pos2::new(x, paint::snap(y)),
                        Vec2::new(bar_width, block_h - 1.0),
                    );
                    let t = 1.0 - (b as f32 / count.max(1) as f32);
                    paint::fill(painter, segment, bar_color(t, palette));
                }
            } else {
                // Vertical gradient: accent at the top, primary at the base.
                let steps = 12;
                for s in 0..steps {
                    let t0 = s as f32 / steps as f32;
                    let t1 = (s + 1) as f32 / steps as f32;
                    let y0 = bar.bottom() - bar.height() * t0;
                    let y1 = bar.bottom() - bar.height() * t1;
                    let slice = Rect::from_min_max(
                        Pos2::new(bar.left(), paint::snap(y1)),
                        Pos2::new(bar.right(), paint::snap(y0)),
                    );
                    // `t` is height up the bar; the accent lives at the top.
                    paint::fill(painter, slice, bar_color(t0, palette));
                }
            }
        }

        // Peak-hold markers.
        for (i, &peak) in self.peaks.iter().enumerate() {
            let y = paint::snap(inner.bottom() - peak.clamp(0.0, 1.0) * inner.height());
            if y >= inner.bottom() - 1.0 {
                continue;
            }
            let x = paint::snap(inner.left() + i as f32 * slot);
            paint::hline(
                painter,
                x,
                x + bar_width,
                y,
                palette.lighten(palette.accent, 0.3),
            );
        }
    }

    fn paint_line(&self, painter: &egui::Painter, inner: Rect, palette: &Palette) {
        let n = self.bars.len();
        if n < 2 {
            return;
        }
        let step = inner.width() / (n - 1) as f32;

        let points: Vec<Pos2> = self
            .bars
            .iter()
            .enumerate()
            .map(|(i, &value)| {
                Pos2::new(
                    paint::snap(inner.left() + i as f32 * step),
                    paint::snap(inner.bottom() - value.clamp(0.0, 1.0) * inner.height()),
                )
            })
            .collect();

        // Fill under the curve, then draw the bright top edge over it.
        let mut mesh = egui::Mesh::default();
        let fill_color = palette.primary_dim;
        for pair in points.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let base = mesh.vertices.len() as u32;
            mesh.colored_vertex(a, fill_color);
            mesh.colored_vertex(b, fill_color);
            mesh.colored_vertex(Pos2::new(b.x, inner.bottom()), fill_color);
            mesh.colored_vertex(Pos2::new(a.x, inner.bottom()), fill_color);
            mesh.add_triangle(base, base + 1, base + 2);
            mesh.add_triangle(base, base + 2, base + 3);
        }
        painter.add(egui::Shape::mesh(mesh));

        for pair in points.windows(2) {
            painter.line_segment(
                [pair[0], pair[1]],
                egui::Stroke::new(2.0_f32, palette.primary),
            );
        }
    }
}

/// Color for a point at height `t` up a bar: primary at the base, accent near
/// the top, danger at the very peak.
fn bar_color(t: f32, palette: &Palette) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    if t > 0.88 {
        palette.danger
    } else if t > 0.65 {
        paint::blend(palette.accent, palette.danger, (t - 0.65) / 0.23)
    } else {
        paint::blend(palette.primary, palette.accent, t / 0.65)
    }
}

/// Nearest-neighbour resample so a bar-count mismatch can't misalign.
///
/// Out-of-range indices clamp to the nearest end rather than returning zero —
/// a bar silently dropping to silence looks like a bug in the audio, not in
/// the display.
fn resample(values: &[f32], index: usize, count: usize) -> f32 {
    if values.is_empty() || count == 0 {
        return 0.0;
    }
    let source = (index * values.len() / count).min(values.len() - 1);
    values[source]
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::pixel::test_support::{painter, palette};

    fn rect() -> Rect {
        Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(200.0, 60.0))
    }

    #[test]
    fn starts_idle() {
        let spec = Spectrum::new(24, Style::Bars);
        assert!(spec.is_idle());
        assert_eq!(spec.num_bars(), 24);
    }

    #[test]
    fn clamps_bar_count_to_at_least_one() {
        assert_eq!(Spectrum::new(0, Style::Bars).num_bars(), 1);
    }

    #[test]
    fn update_copies_matching_lengths() {
        let mut spec = Spectrum::new(4, Style::Bars);
        spec.update(&[0.1, 0.2, 0.3, 0.4], &[0.2, 0.3, 0.4, 0.5]);
        assert_eq!(spec.bars, vec![0.1, 0.2, 0.3, 0.4]);
        assert!(!spec.is_idle());
    }

    #[test]
    fn update_resamples_mismatched_lengths() {
        let mut spec = Spectrum::new(3, Style::Bars);
        spec.update(&[1.0, 0.0, 1.0, 0.0], &[0.0; 4]);
        assert_eq!(spec.bars.len(), 3);
        assert!(spec.bars.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn update_with_empty_input_is_safe() {
        let mut spec = Spectrum::new(4, Style::Bars);
        spec.update(&[], &[]);
        assert!(spec.bars.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn reset_clears_everything() {
        let mut spec = Spectrum::new(4, Style::Bars);
        spec.update(&[1.0; 4], &[1.0; 4]);
        spec.reset();
        assert!(spec.is_idle());
    }

    #[test]
    fn paints_in_every_style() {
        let p = painter();
        let pal = palette();
        for style in [Style::Bars, Style::Blocks, Style::Line] {
            let mut spec = Spectrum::new(24, style);
            spec.update(
                &(0..24).map(|i| i as f32 / 24.0).collect::<Vec<_>>(),
                &(0..24)
                    .map(|i| (i as f32 / 24.0) + 0.05)
                    .collect::<Vec<_>>(),
            );
            spec.paint(&p, rect(), &pal);
        }
    }

    #[test]
    fn paints_an_idle_spectrum() {
        let p = painter();
        Spectrum::new(24, Style::Bars).paint(&p, rect(), &palette());
    }

    #[test]
    fn paints_with_extreme_values() {
        let p = painter();
        let pal = palette();
        let mut spec = Spectrum::new(8, Style::Bars);
        // Out-of-range values must be clamped, not drawn off-canvas.
        spec.update(&[-5.0, 0.0, 0.5, 1.0, 99.0, f32::NAN, 0.3, 0.7], &[0.0; 8]);
        spec.paint(&p, rect(), &pal);
    }

    #[test]
    fn degenerate_rect_is_a_noop() {
        let p = painter();
        let zero = Rect::from_min_size(Pos2::ZERO, Vec2::ZERO);
        Spectrum::new(8, Style::Bars).paint(&p, zero, &palette());
    }

    #[test]
    fn single_bar_line_style_does_not_panic() {
        let p = painter();
        let mut spec = Spectrum::new(1, Style::Line);
        spec.update(&[0.5], &[0.5]);
        spec.paint(&p, rect(), &palette());
    }

    #[test]
    fn bar_color_climbs_through_the_palette() {
        let pal = palette();
        // Low bars use the primary, high ones shift to the warning colors.
        assert_eq!(bar_color(0.0, &pal), pal.primary);
        assert_eq!(bar_color(1.0, &pal), pal.danger);
    }

    #[test]
    fn bar_color_clamps_out_of_range_input() {
        let pal = palette();
        assert_eq!(bar_color(-1.0, &pal), bar_color(0.0, &pal));
        assert_eq!(bar_color(2.0, &pal), bar_color(1.0, &pal));
    }

    #[test]
    fn resample_handles_degenerate_inputs() {
        assert_eq!(resample(&[], 0, 4), 0.0);
        assert_eq!(resample(&[1.0], 0, 0), 0.0);
        assert_eq!(resample(&[1.0, 2.0], 99, 4), 2.0);
    }
}
