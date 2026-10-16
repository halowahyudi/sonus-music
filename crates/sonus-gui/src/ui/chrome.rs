//! Title bar and status bar — the thin chrome around the player.

use egui::{Align2, Pos2, Rect, Sense, Vec2};

use crate::app::{Notice, SonusApp};
use crate::pixel::{body, font_size, paint, ButtonStyle, Palette, PixelButton};

/// Title bar height. Doubles as the window drag handle.
pub const TITLEBAR_HEIGHT: f32 = 20.0;
/// Status bar height.
pub const STATUS_HEIGHT: f32 = 18.0;

// The chrome must stay thin — these are checked at compile time so a careless
// edit to the constants fails the build rather than silently eating the
// window's usable area.
const _: () = assert!(TITLEBAR_HEIGHT <= 24.0);
const _: () = assert!(STATUS_HEIGHT <= 20.0);

/// Draw the title bar: app name, theme button, minimise and close.
pub fn titlebar(ui: &mut egui::Ui, app: &mut SonusApp, palette: &Palette) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, TITLEBAR_HEIGHT), Sense::hover());
    let rect = paint::snap_rect(rect);

    paint::fill(ui.painter(), rect, palette.panel_dark);
    paint::hline(
        ui.painter(),
        rect.left(),
        rect.right(),
        rect.bottom() - 1.0,
        palette.border,
    );

    // Title, with a small LED that pulses while playing.
    let playing = app.audio.status().transport.is_active();
    let led_on = playing && ((app.elapsed * 2.0) as u32).is_multiple_of(2);
    let led_rect = Rect::from_min_size(
        Pos2::new(rect.left() + 5.0, rect.center().y - 3.0),
        Vec2::new(5.0, 5.0),
    );
    paint::led(ui.painter(), led_rect, led_on, palette, palette.primary);

    paint::text(
        ui.painter(),
        Pos2::new(rect.left() + 15.0, rect.center().y),
        Align2::LEFT_CENTER,
        "SONUS",
        body(font_size::SMALL),
        palette.fg,
    );

    // Track count / queue depth, right after the name.
    let status_text = format!(
        "{}  {}",
        app.player.queue.len(),
        app.player.queue.source().label()
    );
    paint::text(
        ui.painter(),
        Pos2::new(rect.left() + 62.0, rect.center().y),
        Align2::LEFT_CENTER,
        &status_text,
        body(font_size::TINY),
        palette.fg_dim,
    );

    // ---- right-hand controls ---------------------------------------------
    let mut right = rect.right() - 4.0;

    // Close.
    let close_rect = Rect::from_min_size(
        Pos2::new(right - 18.0, rect.top() + 2.0),
        Vec2::new(18.0, 16.0),
    );
    if PixelButton::new("X")
        .size(Vec2::new(18.0, 16.0))
        .font_size(font_size::SMALL)
        .icon_only()
        .show_at(ui, close_rect, palette)
        .clicked()
    {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
    }
    right -= 20.0;

    // Minimise.
    let min_rect = Rect::from_min_size(
        Pos2::new(right - 18.0, rect.top() + 2.0),
        Vec2::new(18.0, 16.0),
    );
    if PixelButton::new("-")
        .size(Vec2::new(18.0, 16.0))
        .font_size(font_size::SMALL)
        .icon_only()
        .show_at(ui, min_rect, palette)
        .clicked()
    {
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
    }
    right -= 20.0;

    // Theme picker.
    let theme_rect = Rect::from_min_size(
        Pos2::new(right - 40.0, rect.top() + 2.0),
        Vec2::new(40.0, 16.0),
    );
    if PixelButton::new("THEME")
        .size(Vec2::new(40.0, 16.0))
        .font_size(font_size::TINY)
        .style(if app.show_themes {
            ButtonStyle::Active
        } else {
            ButtonStyle::Ghost
        })
        .show_at(ui, theme_rect, palette)
        .clicked()
    {
        app.show_themes = !app.show_themes;
    }
    right -= 42.0;

    // EQ toggle.
    let eq_rect = Rect::from_min_size(
        Pos2::new(right - 24.0, rect.top() + 2.0),
        Vec2::new(24.0, 16.0),
    );
    if PixelButton::new("EQ")
        .size(Vec2::new(24.0, 16.0))
        .font_size(font_size::TINY)
        .style(if app.show_eq {
            ButtonStyle::Active
        } else {
            ButtonStyle::Ghost
        })
        .show_at(ui, eq_rect, palette)
        .clicked()
    {
        app.show_eq = !app.show_eq;
    }
    right -= 26.0;

    // Help.
    let help_rect = Rect::from_min_size(
        Pos2::new(right - 18.0, rect.top() + 2.0),
        Vec2::new(18.0, 16.0),
    );
    if PixelButton::new("?")
        .size(Vec2::new(18.0, 16.0))
        .font_size(font_size::SMALL)
        .style(if app.show_help {
            ButtonStyle::Active
        } else {
            ButtonStyle::Ghost
        })
        .show_at(ui, help_rect, palette)
        .clicked()
    {
        app.show_help = !app.show_help;
    }

    // ---- drag handle ------------------------------------------------------
    // Everything between the label and the buttons drags the window.
    let drag_left = rect.left() + 62.0 + status_text.len() as f32 * 5.0 + 8.0;
    let drag_right = right - 22.0;
    if drag_right > drag_left {
        let drag_rect = Rect::from_min_size(
            Pos2::new(drag_left, rect.top()),
            Vec2::new(drag_right - drag_left, rect.height()),
        );
        let drag = ui.interact(drag_rect, ui.id().with("titlebar_drag"), Sense::drag());
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        // Double-click to maximise, like a normal title bar.
        if drag.double_clicked() {
            let maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
        }
    }
}

/// Draw the status bar: current notice, dependency warnings, queue stats.
pub fn statusbar(ui: &mut egui::Ui, app: &SonusApp, palette: &Palette) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, STATUS_HEIGHT), Sense::hover());
    let rect = paint::snap_rect(rect);

    paint::fill(ui.painter(), rect, palette.panel_dark);
    paint::hline(
        ui.painter(),
        rect.left(),
        rect.right(),
        rect.top(),
        palette.border,
    );

    let painter = ui.painter();

    // Left: the most recent notice, or a startup problem, or a hint.
    let (message, color) = if let Some(problem) = app.dependency_problems.first() {
        (problem.clone(), palette.danger)
    } else if let Some(Notice { text, is_error, .. }) = app.active_notice() {
        (
            text.clone(),
            if *is_error {
                palette.danger
            } else {
                palette.accent
            },
        )
    } else if let Some(problem) = app.themes.warnings().first() {
        (format!("Theme: {problem}"), palette.danger)
    } else {
        (
            "Space play/pause  Arrows seek  N/P track  / search".to_string(),
            palette.fg_dim,
        )
    };

    paint::text_clipped(
        painter,
        Rect::from_min_max(rect.min, Pos2::new(rect.right() - 120.0, rect.bottom())),
        Pos2::new(rect.left() + 5.0, rect.center().y),
        Align2::LEFT_CENTER,
        &message,
        body(font_size::TINY),
        color,
    );

    // Right: buffering indicator and remaining time.
    let status = app.audio.status();
    let right_text = if status.buffered_samples > 0 && status.transport.is_active() {
        let remaining = app.player.queue.remaining_duration_secs();
        format!("BUF  {} left", format_duration(remaining))
    } else {
        let remaining = app.player.queue.remaining_duration_secs();
        format!("{} left", format_duration(remaining))
    };

    paint::text(
        painter,
        Pos2::new(rect.right() - 5.0, rect.center().y),
        Align2::RIGHT_CENTER,
        &right_text,
        body(font_size::TINY),
        palette.fg_dim,
    );
}

fn format_duration(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_formatting() {
        assert_eq!(format_duration(0), "0:00");
        assert_eq!(format_duration(65), "1:05");
        assert_eq!(format_duration(3725), "1:02:05");
    }
}
