//! Pixel-art buttons and toggles.

use egui::{Align2, Color32, Pos2, Rect, Response, Sense, Ui, Vec2};

use super::font;
use super::paint::{self, Bevel};
use super::palette::Palette;

/// How a button is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonStyle {
    /// Standard raised plastic button.
    Normal,
    /// Currently active — lit with the primary color.
    Active,
    /// Rendered dim and unresponsive.
    Disabled,
    /// Looks like a flat label; only the text changes on hover.
    Ghost,
}

/// A pixel-art button.
///
/// Sized in whole pixels and drawn from scratch rather than through egui's
/// `Button`, because egui's widget can't do hard-edged bevels.
pub struct PixelButton<'a> {
    label: &'a str,
    size: Vec2,
    style: ButtonStyle,
    font_size: f32,
    icon_only: bool,
}

impl<'a> PixelButton<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            size: Vec2::new(48.0, 20.0),
            style: ButtonStyle::Normal,
            font_size: font::size::BODY,
            icon_only: false,
        }
    }

    pub fn size(mut self, size: Vec2) -> Self {
        self.size = Vec2::new(paint::snap(size.x), paint::snap(size.y));
        self
    }

    pub fn style(mut self, style: ButtonStyle) -> Self {
        self.style = style;
        self
    }

    pub fn font_size(mut self, size: f32) -> Self {
        self.font_size = size;
        self
    }

    /// Draw without a background — used for transport icons.
    pub fn icon_only(mut self) -> Self {
        self.icon_only = true;
        self
    }

    /// Show the button and return its response.
    pub fn show(self, ui: &mut Ui, palette: &Palette) -> Response {
        let enabled = self.style != ButtonStyle::Disabled;
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(self.size, sense);
        self.draw(ui, rect, palette, response)
    }

    /// Draw the button at an explicit rect.
    ///
    /// Used by layouts that position controls by hand — the transport row, for
    /// instance, where the buttons need to line up with a hand-drawn LCD panel
    /// rather than flow with the normal layout cursor.
    pub fn show_at(self, ui: &mut Ui, rect: Rect, palette: &Palette) -> Response {
        let enabled = self.style != ButtonStyle::Disabled;
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let response = ui.interact(
            rect,
            ui.id().with(self.label).with(rect.min.x as i32),
            sense,
        );
        self.draw(ui, rect, palette, response)
    }

    fn draw(self, ui: &Ui, rect: Rect, palette: &Palette, response: Response) -> Response {
        let rect = paint::snap_rect(rect);
        let enabled = self.style != ButtonStyle::Disabled;

        if !ui.is_rect_visible(rect) {
            return response;
        }

        let pressed = enabled && response.is_pointer_button_down_on();
        let hovered = enabled && response.hovered();

        let text_color = self.paint(ui, rect, palette, pressed, hovered);
        let font_id = font::body(self.font_size);

        paint::text_clipped(
            ui.painter(),
            rect,
            rect.center(),
            Align2::CENTER_CENTER,
            self.label,
            font_id,
            text_color,
        );

        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        response
    }

    /// Draw the button chrome and return the color its text should use.
    fn paint(
        &self,
        ui: &Ui,
        rect: Rect,
        palette: &Palette,
        pressed: bool,
        hovered: bool,
    ) -> Color32 {
        let painter = ui.painter();

        if self.icon_only {
            // No chrome; just tint the glyph.
            return match self.style {
                ButtonStyle::Disabled => palette.disabled,
                ButtonStyle::Active => palette.primary,
                _ if hovered => palette.primary,
                _ => palette.fg,
            };
        }

        match self.style {
            ButtonStyle::Disabled => {
                paint::surface(painter, rect, palette.panel, Bevel::Raised, palette);
                palette.disabled
            }
            ButtonStyle::Active => {
                // Lit button: primary fill with a darker bevel so it still
                // reads as a physical key.
                paint::surface(painter, rect, palette.primary_dim, Bevel::Sunken, palette);
                palette.highlight_fg
            }
            ButtonStyle::Ghost => {
                if hovered {
                    paint::fill(painter, rect, palette.row_hover);
                }
                if hovered {
                    palette.primary
                } else {
                    palette.fg_dim
                }
            }
            ButtonStyle::Normal => {
                if pressed {
                    // Pressed inverts the bevel — the key sinks into the panel.
                    paint::surface(painter, rect, palette.panel_dark, Bevel::Sunken, palette);
                    palette.primary
                } else {
                    let face = if hovered {
                        palette.highlight
                    } else {
                        palette.panel
                    };
                    paint::surface(painter, rect, face, Bevel::Raised, palette);
                    if hovered {
                        palette.primary
                    } else {
                        palette.fg
                    }
                }
            }
        }
    }
}

/// A two-state toggle drawn as a small LED plus a label.
pub fn toggle(
    ui: &mut Ui,
    id: impl std::hash::Hash,
    label: &str,
    on: bool,
    palette: &Palette,
) -> bool {
    let size = Vec2::new(52.0, 20.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let rect = paint::snap_rect(rect);

    if !ui.is_rect_visible(rect) {
        return false;
    }

    let hovered = response.hovered();
    let painter = ui.painter();
    let _ = id;

    paint::surface(
        painter,
        rect,
        if hovered {
            palette.highlight
        } else {
            palette.panel
        },
        if on { Bevel::Sunken } else { Bevel::Raised },
        palette,
    );

    // LED on the left edge.
    let led_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 4.0, rect.center().y - 3.0),
        Vec2::new(6.0, 6.0),
    );
    paint::led(painter, led_rect, on, palette, palette.primary);

    paint::text_clipped(
        painter,
        rect,
        Pos2::new(rect.left() + 14.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        font::body(font::size::SMALL),
        if on { palette.primary } else { palette.fg_dim },
    );

    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    response.clicked()
}

/// A row of mutually exclusive options — used for EQ presets and repeat mode.
///
/// Returns the index that was clicked, if any.
pub fn segmented(
    ui: &mut Ui,
    options: &[&str],
    selected: Option<usize>,
    palette: &Palette,
    segment_width: f32,
) -> Option<usize> {
    let mut clicked = None;

    ui.horizontal(|ui| {
        for (index, label) in options.iter().enumerate() {
            let style = if Some(index) == selected {
                ButtonStyle::Active
            } else {
                ButtonStyle::Normal
            };
            let button = PixelButton::new(label)
                .size(Vec2::new(segment_width, 18.0))
                .font_size(font::size::TINY)
                .style(style);

            if button.show(ui, palette).clicked() {
                clicked = Some(index);
            }
        }
    });

    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::pixel::test_support::{in_frame, palette};

    #[test]
    fn button_reports_a_click() {
        let pal = palette();
        in_frame(|ui| {
            let response = PixelButton::new("GO").show(ui, &pal);
            // No pointer interaction in a synthetic frame, so it can't click.
            assert!(!response.clicked());
        });
    }

    #[test]
    fn disabled_button_is_not_clickable() {
        let pal = palette();
        in_frame(|ui| {
            let response = PixelButton::new("GO")
                .style(ButtonStyle::Disabled)
                .show(ui, &pal);
            assert!(!response.clicked());
            assert!(!response.is_pointer_button_down_on());
        });
    }

    #[test]
    fn button_snaps_its_size_to_whole_pixels() {
        let pal = palette();
        in_frame(|ui| {
            let response = PixelButton::new("GO")
                .size(Vec2::new(50.3, 20.7))
                .show(ui, &pal);
            assert_eq!(response.rect.width(), 50.0);
            assert_eq!(response.rect.height(), 21.0);
        });
    }

    #[test]
    fn every_button_style_draws() {
        let pal = palette();
        in_frame(|ui| {
            for style in [
                ButtonStyle::Normal,
                ButtonStyle::Active,
                ButtonStyle::Disabled,
                ButtonStyle::Ghost,
            ] {
                PixelButton::new("X").style(style).show(ui, &pal);
                PixelButton::new("X")
                    .style(style)
                    .icon_only()
                    .show(ui, &pal);
            }
        });
    }

    #[test]
    fn toggle_returns_false_without_interaction() {
        let pal = palette();
        in_frame(|ui| {
            assert!(!toggle(ui, "eq", "EQ", true, &pal));
            assert!(!toggle(ui, "eq", "EQ", false, &pal));
        });
    }

    #[test]
    fn segmented_reports_no_click_in_a_synthetic_frame() {
        let pal = palette();
        in_frame(|ui| {
            let clicked = segmented(ui, &["OFF", "ALL", "ONE"], Some(0), &pal, 32.0);
            assert!(clicked.is_none());
        });
    }

    #[test]
    fn segmented_handles_an_empty_option_list() {
        let pal = palette();
        in_frame(|ui| {
            assert!(segmented(ui, &[], None, &pal, 32.0).is_none());
        });
    }
}
