//! Pixel-art list rows.

use egui::{Align2, Color32, Pos2, Rect, Response, Sense, Ui, Vec2};

use super::font;
use super::paint;
use super::palette::Palette;

/// Standard row height. Odd so the selection marker sits on a clean pixel.
pub const ROW_HEIGHT: f32 = 22.0;

/// What a row should look like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    Normal,
    /// Alternating stripe — subtle background tint.
    Alternate,
    Hovered,
    /// This is the track currently loaded.
    Playing,
    /// This is playing and paused.
    Paused,
}

/// Everything a row needs to render.
pub struct RowContent<'a> {
    /// Number shown on the left, or `None` to leave it blank.
    pub index: Option<usize>,
    pub title: &'a str,
    pub subtitle: Option<&'a str>,
    pub duration: Option<&'a str>,
    /// Badge drawn on the right — "PLAYING", "LIKED", etc.
    pub badge: Option<&'a str>,
}

/// Draw a list row. Returns the row's response.
///
/// `width` should be the full width of the list; the caller is responsible for
/// scrolling.
pub fn row(
    ui: &mut Ui,
    width: f32,
    content: &RowContent<'_>,
    state: RowState,
    palette: &Palette,
    is_liked: bool,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, ROW_HEIGHT), Sense::click());
    let rect = paint::snap_rect(rect);

    if !ui.is_rect_visible(rect) {
        return response;
    }

    let hovered = response.hovered();
    let painter = ui.painter();

    // Background: playing > hovered > stripe.
    let background = match state {
        RowState::Playing | RowState::Paused => palette.highlight,
        _ if hovered => palette.row_hover,
        RowState::Alternate => palette.row_alt,
        RowState::Normal | RowState::Hovered => Color32::TRANSPARENT,
    };
    if background != Color32::TRANSPARENT {
        paint::fill(painter, rect, background);
    }

    // A 2px bar down the left edge marks the playing row — visible even when
    // the row is scrolled half out of view.
    if matches!(state, RowState::Playing | RowState::Paused) {
        let marker = Rect::from_min_size(rect.min, Vec2::new(2.0, rect.height()));
        let color = if state == RowState::Paused {
            palette.primary_dim
        } else {
            palette.primary
        };
        paint::fill(painter, marker, color);
    }

    let text_color = match state {
        RowState::Playing => palette.primary,
        RowState::Paused => palette.primary_dim,
        _ => palette.fg,
    };

    let mut cursor_x = rect.left() + 6.0;

    // Index or playing indicator.
    if let Some(index) = content.index {
        let label = match state {
            RowState::Playing => ">".to_string(),
            RowState::Paused => "||".to_string(),
            _ => format!("{}", index + 1),
        };
        paint::text(
            painter,
            Pos2::new(cursor_x, rect.center().y),
            Align2::LEFT_CENTER,
            &label,
            font::body(font::size::SMALL),
            if matches!(state, RowState::Playing | RowState::Paused) {
                palette.primary
            } else {
                palette.fg_dim
            },
        );
    }
    cursor_x += 22.0;

    // Reserve space on the right for the duration and badge.
    let duration_width = content.duration.map_or(0.0, |d| d.len() as f32 * 7.0 + 8.0);
    let badge_width = content.badge.map_or(0.0, |b| b.len() as f32 * 6.0 + 10.0);
    let liked_width = if is_liked { 14.0 } else { 0.0 };
    let right_reserve = duration_width + badge_width + liked_width + 8.0;

    let text_area = Rect::from_min_max(
        Pos2::new(cursor_x, rect.top()),
        Pos2::new(rect.right() - right_reserve, rect.bottom()),
    );

    // Title, with the artist appended in a dimmer color when there's room.
    let title_width = text_area.width();
    match content.subtitle {
        Some(subtitle) if !subtitle.is_empty() => {
            // Split the width: title gets what it needs, subtitle the rest.
            let title_budget = (title_width * 0.6).max(40.0);
            let title = paint::ellipsize(
                painter,
                content.title,
                &font::body(font::size::BODY),
                title_budget,
            );
            let drawn_width = painter
                .layout_no_wrap(title.clone(), font::body(font::size::BODY), Color32::WHITE)
                .size()
                .x;

            paint::text(
                painter,
                Pos2::new(text_area.left(), rect.center().y),
                Align2::LEFT_CENTER,
                &title,
                font::body(font::size::BODY),
                text_color,
            );

            // Only draw the subtitle if there's meaningful room left.
            let remaining = title_width - drawn_width - 8.0;
            if remaining > 30.0 {
                let subtitle_rect = Rect::from_min_max(
                    Pos2::new(text_area.left() + drawn_width + 6.0, rect.top()),
                    text_area.max,
                );
                paint::text_clipped(
                    painter,
                    subtitle_rect,
                    Pos2::new(subtitle_rect.left(), rect.center().y),
                    Align2::LEFT_CENTER,
                    subtitle,
                    font::body(font::size::SMALL),
                    palette.fg_dim,
                );
            }
        }
        _ => {
            paint::text_clipped(
                painter,
                text_area,
                Pos2::new(text_area.left(), rect.center().y),
                Align2::LEFT_CENTER,
                content.title,
                font::body(font::size::BODY),
                text_color,
            );
        }
    }

    // Right-aligned: liked heart, badge, duration.
    let mut right_x = rect.right() - 6.0;

    if let Some(duration) = content.duration {
        paint::text(
            painter,
            Pos2::new(right_x, rect.center().y),
            Align2::RIGHT_CENTER,
            duration,
            font::body(font::size::SMALL),
            palette.fg_dim,
        );
        right_x -= duration_width;
    }

    if let Some(badge) = content.badge {
        let badge_rect = Rect::from_min_size(
            Pos2::new(
                right_x - badge.len() as f32 * 6.0 - 4.0,
                rect.center().y - 6.0,
            ),
            Vec2::new(badge.len() as f32 * 6.0 + 6.0, 12.0),
        );
        paint::fill(painter, badge_rect, palette.primary_dim);
        paint::text(
            painter,
            badge_rect.center(),
            Align2::CENTER_CENTER,
            badge,
            font::body(font::size::TINY),
            palette.highlight_fg,
        );
        right_x -= badge_rect.width() + 4.0;
    }

    if is_liked {
        paint::text(
            painter,
            Pos2::new(right_x - 6.0, rect.center().y),
            Align2::RIGHT_CENTER,
            "*",
            font::body(font::size::BODY),
            palette.accent,
        );
    }

    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    response
}

/// Draw a section header inside a list.
pub fn header(ui: &mut Ui, width: f32, label: &str, count: Option<usize>, palette: &Palette) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 18.0), Sense::hover());
    let rect = paint::snap_rect(rect);

    if !ui.is_rect_visible(rect) {
        return;
    }

    let painter = ui.painter();
    paint::fill(painter, rect, palette.panel_dark);

    let text = match count {
        Some(count) => format!("{label}  ({count})"),
        None => label.to_string(),
    };
    paint::text(
        painter,
        Pos2::new(rect.left() + 6.0, rect.center().y),
        Align2::LEFT_CENTER,
        &text,
        font::body(font::size::TINY),
        palette.primary_dim,
    );

    paint::hline(
        painter,
        rect.left(),
        rect.right(),
        rect.bottom() - 1.0,
        palette.border,
    );
}

/// Draw a placeholder message centered in the available space.
pub fn empty_state(ui: &mut Ui, title: &str, hint: &str, palette: &Palette) {
    ui.add_space(24.0);
    ui.vertical_centered(|ui| {
        ui.label(
            egui::RichText::new(title)
                .font(font::body(font::size::BODY))
                .color(palette.fg_dim),
        );
        if !hint.is_empty() {
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(hint)
                    .font(font::body(font::size::SMALL))
                    .color(palette.disabled),
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::pixel::test_support::{in_frame, palette};

    fn content() -> RowContent<'static> {
        RowContent {
            index: Some(0),
            title: "Bonobo - Rosewood",
            subtitle: Some("Black Sands"),
            duration: Some("4:12"),
            badge: None,
        }
    }

    #[test]
    fn row_reports_its_height() {
        let pal = palette();
        in_frame(|ui| {
            let response = row(ui, 300.0, &content(), RowState::Normal, &pal, false);
            assert_eq!(response.rect.height(), ROW_HEIGHT);
            assert_eq!(response.rect.width(), 300.0);
        });
    }

    #[test]
    fn every_row_state_draws() {
        let pal = palette();
        in_frame(|ui| {
            for state in [
                RowState::Normal,
                RowState::Alternate,
                RowState::Hovered,
                RowState::Playing,
                RowState::Paused,
            ] {
                row(ui, 300.0, &content(), state, &pal, false);
            }
        });
    }

    #[test]
    fn row_without_optional_fields_draws() {
        let pal = palette();
        in_frame(|ui| {
            let bare = RowContent {
                index: None,
                title: "Just A Title",
                subtitle: None,
                duration: None,
                badge: None,
            };
            row(ui, 300.0, &bare, RowState::Normal, &pal, false);
        });
    }

    #[test]
    fn row_with_badge_and_like_draws() {
        let pal = palette();
        in_frame(|ui| {
            let full = RowContent {
                badge: Some("PLAYING"),
                ..content()
            };
            row(ui, 300.0, &full, RowState::Playing, &pal, true);
        });
    }

    #[test]
    fn row_survives_a_narrow_width() {
        let pal = palette();
        in_frame(|ui| {
            // Narrower than the reserved right-hand space.
            row(ui, 40.0, &content(), RowState::Normal, &pal, true);
        });
    }

    #[test]
    fn row_handles_empty_strings() {
        let pal = palette();
        in_frame(|ui| {
            let empty = RowContent {
                index: Some(0),
                title: "",
                subtitle: Some(""),
                duration: Some(""),
                badge: Some(""),
            };
            row(ui, 300.0, &empty, RowState::Normal, &pal, false);
        });
    }

    #[test]
    fn row_with_multibyte_text_does_not_panic() {
        let pal = palette();
        in_frame(|ui| {
            let text = RowContent {
                index: Some(0),
                title: "日本語のタイトル",
                subtitle: Some("アーティスト"),
                duration: Some("4:12"),
                badge: None,
            };
            row(ui, 120.0, &text, RowState::Normal, &pal, false);
        });
    }

    #[test]
    fn header_draws_with_and_without_a_count() {
        let pal = palette();
        in_frame(|ui| {
            header(ui, 300.0, "UP NEXT", Some(12), &pal);
            header(ui, 300.0, "RESULTS", None, &pal);
        });
    }

    #[test]
    fn empty_state_draws_with_and_without_a_hint() {
        let pal = palette();
        in_frame(|ui| {
            empty_state(ui, "Nothing here", "Search for something", &pal);
            empty_state(ui, "Nothing here", "", &pal);
        });
    }

    #[test]
    fn zero_width_row_is_safe() {
        let pal = palette();
        in_frame(|ui| {
            row(ui, 0.0, &content(), RowState::Normal, &pal, false);
        });
    }
}
