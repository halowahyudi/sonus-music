//! Pixel-art sliders.

use egui::{Color32, Pos2, Rect, Response, Sense, Ui, Vec2};

use super::font;
use super::paint::{self, Bevel};
use super::palette::Palette;

/// A horizontal slider with a chunky square thumb.
///
/// Used for both the seek bar and the volume control; the caller decides how
/// to interpret the returned value.
pub struct PixelSlider {
    value: f32,
    min: f32,
    max: f32,
    height: f32,
    fill_color: Option<Color32>,
    /// Draw the value as text to the right of the track.
    show_value: bool,
    /// Optional text shown centered inside the track (e.g. `1:23 / 4:56`).
    overlay: Option<String>,
}

impl PixelSlider {
    pub fn new(value: f32, min: f32, max: f32) -> Self {
        Self {
            value,
            min,
            max,
            height: 12.0,
            fill_color: None,
            show_value: false,
            overlay: None,
        }
    }

    pub fn height(mut self, height: f32) -> Self {
        self.height = paint::snap(height);
        self
    }

    pub fn fill(mut self, color: Color32) -> Self {
        self.fill_color = Some(color);
        self
    }

    pub fn show_value(mut self) -> Self {
        self.show_value = true;
        self
    }

    pub fn overlay(mut self, text: impl Into<String>) -> Self {
        self.overlay = Some(text.into());
        self
    }

    /// Show the slider. Returns the response and the (possibly new) value.
    pub fn show(self, ui: &mut Ui, palette: &Palette) -> (Response, f32) {
        let label_width = if self.show_value { 36.0 } else { 0.0 };
        let width = ui.available_width() - label_width;
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(width, self.height), Sense::click_and_drag());
        let rect = paint::snap_rect(rect);

        let mut value = self.value;
        if let Some(pointer) = response.interact_pointer_pos() {
            if response.dragged() || response.clicked() {
                let t = ((pointer.x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0);
                value = self.min + t * (self.max - self.min);
            }
        }

        if ui.is_rect_visible(rect) {
            self.paint(
                ui,
                rect,
                value,
                palette,
                response.hovered() || response.dragged(),
            );
        }

        if self.show_value {
            let text = match self.overlay.as_deref() {
                Some(_) => String::new(),
                None => format!("{:.0}%", value * 100.0),
            };
            if !text.is_empty() {
                paint::text(
                    ui.painter(),
                    Pos2::new(rect.right() + 6.0, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    &text,
                    font::body(font::size::SMALL),
                    palette.fg_dim,
                );
            }
        }

        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        (response, value)
    }

    fn paint(&self, ui: &Ui, rect: Rect, value: f32, palette: &Palette, active: bool) {
        let painter = ui.painter();

        // Sunken track — reads as a groove cut into the panel.
        paint::surface(painter, rect, palette.lcd_bg, Bevel::Sunken, palette);

        let t = ((value - self.min) / (self.max - self.min).max(f32::EPSILON)).clamp(0.0, 1.0);
        let fill_color = self.fill_color.unwrap_or(palette.primary);

        // Filled portion, inset by 1px so it doesn't cover the bevel.
        let inner = rect.shrink(1.0);
        let filled = Rect::from_min_size(
            inner.min,
            Vec2::new((inner.width() * t).round(), inner.height()),
        );
        if filled.width() >= 1.0 {
            paint::fill(painter, filled, fill_color);
            // A lighter line along the top of the fill gives it a glassy look.
            paint::hline(
                painter,
                filled.left(),
                filled.right(),
                filled.top(),
                palette.lighten(fill_color, 0.35),
            );
        }

        // Square thumb with grip lines, like a physical fader cap.
        let thumb_w = 8.0;
        let thumb = Rect::from_min_size(
            Pos2::new(
                paint::snap(rect.left() + inner.width() * t - thumb_w / 2.0)
                    .clamp(rect.left(), rect.right() - thumb_w),
                rect.top() - 1.0,
            ),
            Vec2::new(thumb_w, rect.height() + 2.0),
        );
        let thumb_color = if active {
            palette.lighten(fill_color, 0.25)
        } else {
            palette.panel
        };
        paint::surface(painter, thumb, thumb_color, Bevel::Raised, palette);
        // Grip lines down the middle of the cap.
        for offset in [-1.0_f32, 1.0] {
            paint::vline(
                painter,
                thumb.center().x + offset,
                thumb.top() + 3.0,
                thumb.bottom() - 3.0,
                palette.dim(thumb_color, 0.5),
            );
        }

        // Centered overlay text (seek position, etc).
        if let Some(overlay) = &self.overlay {
            paint::text_clipped(
                painter,
                rect,
                rect.center(),
                egui::Align2::CENTER_CENTER,
                overlay,
                font::body(font::size::TINY),
                palette.fg,
            );
        }
    }
}

/// A vertical slider for EQ bands.
///
/// Draws a center line at `center` and fills from there to the value, so a
/// boost reads as upward and a cut as downward — how a hardware EQ looks.
pub struct BandSlider;

impl BandSlider {
    /// Show one band. Returns the response and the new value.
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        ui: &mut Ui,
        value: f32,
        min: f32,
        max: f32,
        center: f32,
        size: Vec2,
        palette: &Palette,
        fill_color: Color32,
    ) -> (Response, f32) {
        let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let rect = paint::snap_rect(rect);

        let mut new_value = value;
        if let Some(pointer) = response.interact_pointer_pos() {
            if response.dragged() || response.clicked() {
                // Vertical: top of the track is the maximum.
                let t = 1.0 - ((pointer.y - rect.top()) / rect.height().max(1.0)).clamp(0.0, 1.0);
                new_value = min + t * (max - min);
            }
        }

        if ui.is_rect_visible(rect) {
            let painter = ui.painter();
            let active = response.hovered() || response.dragged();

            let track = Rect::from_min_size(
                Pos2::new(rect.center().x - 3.0, rect.top()),
                Vec2::new(6.0, rect.height()),
            );
            paint::surface(painter, track, palette.lcd_bg, Bevel::Sunken, palette);

            let to_y = |v: f32| {
                paint::snap(
                    track.bottom() - ((v - min) / (max - min).max(f32::EPSILON)) * track.height(),
                )
            };
            let center_y = to_y(center);
            let value_y = to_y(new_value);

            // Fill from the center line to the value.
            let (top, bottom) = if value_y <= center_y {
                (value_y, center_y)
            } else {
                (center_y, value_y)
            };
            if bottom - top >= 1.0 {
                paint::fill(
                    painter,
                    Rect::from_min_max(
                        Pos2::new(track.left() + 1.0, top),
                        Pos2::new(track.right() - 1.0, bottom),
                    ),
                    fill_color,
                );
            }

            // Zero line — always visible, so you can see where flat is.
            paint::hline(
                painter,
                track.left() - 1.0,
                track.right() + 1.0,
                center_y,
                palette.dim(fill_color, 0.6),
            );

            // Thumb.
            let thumb = Rect::from_min_size(
                Pos2::new(rect.center().x - 7.0, value_y - 3.0),
                Vec2::new(14.0, 6.0),
            );
            paint::surface(
                painter,
                thumb,
                if active {
                    palette.lighten(fill_color, 0.25)
                } else {
                    palette.panel
                },
                Bevel::Raised,
                palette,
            );
            paint::hline(
                painter,
                thumb.left() + 2.0,
                thumb.right() - 2.0,
                thumb.center().y,
                palette.dim(fill_color, 0.4),
            );
        }

        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        }

        (response, new_value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::pixel::test_support::{in_frame, palette};

    #[test]
    fn slider_returns_its_value_unchanged_without_interaction() {
        let pal = palette();
        in_frame(|ui| {
            let (_, value) = PixelSlider::new(0.5, 0.0, 1.0).show(ui, &pal);
            assert_eq!(value, 0.5);
        });
    }

    #[test]
    fn slider_draws_in_every_configuration() {
        let pal = palette();
        in_frame(|ui| {
            PixelSlider::new(0.3, 0.0, 1.0).show(ui, &pal);
            PixelSlider::new(0.3, 0.0, 1.0).show_value().show(ui, &pal);
            PixelSlider::new(0.3, 0.0, 1.0)
                .overlay("1:23 / 4:56")
                .height(16.0)
                .fill(Color32::RED)
                .show(ui, &pal);
        });
    }

    #[test]
    fn slider_handles_a_degenerate_range() {
        let pal = palette();
        in_frame(|ui| {
            // min == max would divide by zero without the epsilon guard.
            let (_, value) = PixelSlider::new(5.0, 5.0, 5.0).show(ui, &pal);
            assert!(value.is_finite());
        });
    }

    #[test]
    fn slider_clamps_values_outside_its_range() {
        let pal = palette();
        in_frame(|ui| {
            // Should draw without panicking even though 5.0 is out of range.
            let (_, value) = PixelSlider::new(5.0, 0.0, 1.0).show(ui, &pal);
            assert!(value.is_finite());
        });
    }

    #[test]
    fn band_slider_returns_its_value_unchanged() {
        let pal = palette();
        in_frame(|ui| {
            let (_, value) = BandSlider::show(
                ui,
                -3.0,
                -12.0,
                12.0,
                0.0,
                Vec2::new(16.0, 60.0),
                &pal,
                pal.primary,
            );
            assert_eq!(value, -3.0);
        });
    }

    #[test]
    fn band_slider_draws_above_and_below_center() {
        let pal = palette();
        in_frame(|ui| {
            for value in [-12.0, -1.0, 0.0, 1.0, 12.0] {
                BandSlider::show(
                    ui,
                    value,
                    -12.0,
                    12.0,
                    0.0,
                    Vec2::new(16.0, 60.0),
                    &pal,
                    pal.primary,
                );
            }
        });
    }

    #[test]
    fn band_slider_handles_a_degenerate_range() {
        let pal = palette();
        in_frame(|ui| {
            let (_, value) = BandSlider::show(
                ui,
                0.0,
                0.0,
                0.0,
                0.0,
                Vec2::new(16.0, 60.0),
                &pal,
                pal.primary,
            );
            assert!(value.is_finite());
        });
    }
}
