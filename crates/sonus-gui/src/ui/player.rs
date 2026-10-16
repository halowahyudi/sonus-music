//! The main player panel: LCD readout, transport buttons, seek and volume.
//!
//! This is the part of the UI that never goes away — the views below it swap
//! out, but the player is always there. It's laid out top-down like a piece of
//! hardware: display, then controls, then the sliders.

use egui::{Align2, Pos2, Rect, Sense, Vec2};

use crate::app::SonusApp;
use crate::audio::Transport;
use crate::pixel::palette::Palette;
use crate::pixel::{
    body, display, font_size, format_clock, paint, ButtonStyle, PixelButton, SevenSeg,
};

/// Total height of the player panel.
pub const HEIGHT: f32 = 116.0;

// Display (44) + button row (20) + seek bar (14) + labels (12) must fit.
// Checked at compile time so the layout can't silently clip.
const _: () = assert!(HEIGHT >= 44.0 + 20.0 + 14.0 + 12.0);

/// Draw the player panel.
pub fn render(ui: &mut egui::Ui, app: &mut SonusApp, palette: &Palette) {
    let width = ui.available_width();

    let (panel, _) = ui.allocate_exact_size(Vec2::new(width, HEIGHT), Sense::hover());
    let panel = paint::snap_rect(panel);

    paint::surface(
        ui.painter(),
        panel,
        palette.panel,
        paint::Bevel::Flat,
        palette,
    );

    let inner = panel.shrink(4.0);

    render_display(ui, inner, app, palette);
    render_controls(ui, inner, app, palette);
}

/// The LCD readout — title, time, status and a mini spectrum.
fn render_display(ui: &mut egui::Ui, area: Rect, app: &mut SonusApp, palette: &Palette) {
    let display_rect = Rect::from_min_size(
        Pos2::new(area.left(), area.top()),
        Vec2::new(area.width(), 44.0),
    );

    // Sunken glass panel.
    paint::surface(
        ui.painter(),
        display_rect,
        palette.lcd_bg,
        paint::Bevel::Sunken,
        palette,
    );
    let inner = display_rect.shrink(4.0);

    // Scanline overlay — subtle horizontal stripes that sell the CRT look.
    {
        let painter = ui.painter();
        let mut y = inner.top();
        while y < inner.bottom() {
            paint::hline(
                painter,
                inner.left(),
                inner.right(),
                y,
                palette.dim(palette.lcd_bg, 0.35),
            );
            y += 3.0;
        }
    }

    // ---- now playing line -------------------------------------------------
    let title_area = Rect::from_min_max(
        inner.min,
        Pos2::new(inner.right() - 78.0, inner.top() + 14.0),
    );

    let (title_text, title_color) = match app.current_track() {
        Some(track) => (track.label(), palette.lcd_fg),
        None => ("Sonus".to_string(), palette.lcd_dim),
    };

    // Marquee long titles so nothing is permanently hidden.
    let char_width = 7.0;
    let marquee = paint::marquee(&title_text, title_area.width(), char_width, app.elapsed);

    paint::text_clipped(
        ui.painter(),
        title_area,
        Pos2::new(title_area.left(), title_area.top() + 1.0),
        Align2::LEFT_TOP,
        &marquee,
        display(font_size::TITLE),
        title_color,
    );

    // ---- status and bitrate line -----------------------------------------
    let status = app.audio.status();
    let status_text = if !status.device_ready {
        "NO AUDIO DEVICE"
    } else {
        status.transport.label()
    };
    let status_color = match status.transport {
        Transport::Playing => palette.lcd_fg,
        Transport::Paused => palette.accent,
        Transport::Buffering => palette.accent,
        Transport::Stopped => palette.lcd_dim,
    };

    paint::text(
        ui.painter(),
        Pos2::new(inner.left(), inner.top() + 16.0),
        Align2::LEFT_TOP,
        status_text,
        body(font_size::TINY),
        status_color,
    );

    // Bitrate/sample-rate readout, like the hardware displays had.
    let format_text = if status.device_ready {
        "48 kHz  STEREO"
    } else {
        "--- kHz  ------"
    };
    paint::text(
        ui.painter(),
        Pos2::new(inner.left() + 86.0, inner.top() + 16.0),
        Align2::LEFT_TOP,
        format_text,
        body(font_size::TINY),
        palette.lcd_dim,
    );

    // ---- time readout (right side, seven-segment) -------------------------
    let position = if let Some(scrub) = app.scrubbing {
        scrub
    } else {
        app.position_secs
    };
    let duration = app
        .current_track()
        .and_then(|t| t.duration_secs)
        .unwrap_or(0) as f32;

    let clock = format_clock(position.max(0.0) as u64);
    let total = if duration > 0.0 {
        format_clock(duration as u64)
    } else {
        "--:--".to_string()
    };

    let seg_height = 16.0;
    let clock_seg = SevenSeg::new(&clock, palette.lcd_fg, palette.lcd_dim).height(seg_height);
    let total_seg = SevenSeg::new(&total, palette.lcd_dim, palette.lcd_dim).height(seg_height);

    let clock_width = clock_seg.measure();
    let total_width = total_seg.measure();
    let clock_x = inner.right() - total_width - clock_width - 10.0;

    clock_seg.paint(ui.painter(), Pos2::new(clock_x, inner.top()));
    total_seg.paint(
        ui.painter(),
        Pos2::new(inner.right() - total_width, inner.top()),
    );

    // Small blinking separator between the two times.
    if status.transport == Transport::Playing && ((app.elapsed * 2.0) as u32).is_multiple_of(2) {
        paint::text(
            ui.painter(),
            Pos2::new(clock_x + clock_width + 3.0, inner.top() + 4.0),
            Align2::LEFT_TOP,
            "/",
            body(font_size::TINY),
            palette.lcd_dim,
        );
    }

    // ---- mini spectrum in the display ------------------------------------
    let spectrum_rect = Rect::from_min_max(
        Pos2::new(inner.left(), inner.top() + 28.0),
        Pos2::new(
            inner.right() - total_width - clock_width - 14.0,
            inner.bottom(),
        ),
    );
    if spectrum_rect.width() > 20.0 {
        app.spectrum.paint(ui.painter(), spectrum_rect, palette);
    }
}

/// Transport buttons, seek bar and volume.
fn render_controls(ui: &mut egui::Ui, area: Rect, app: &mut SonusApp, palette: &Palette) {
    let controls_top = area.top() + 48.0;
    let has_track = app.current_track().is_some();
    let transport = app.audio.status().transport;
    let ready = app.audio.status().device_ready;

    // ---- button row -------------------------------------------------------
    let mut cursor = Pos2::new(area.left(), controls_top);

    let button_size = Vec2::new(26.0, 20.0);

    // Previous
    let prev_rect = Rect::from_min_size(cursor, button_size);
    if PixelButton::new("|<")
        .size(button_size)
        .font_size(font_size::TINY)
        .icon_only()
        .show_at(ui, prev_rect, palette)
        .clicked()
    {
        app.previous_track();
    }
    cursor.x += button_size.x + 2.0;

    // Play / pause
    let play_rect = Rect::from_min_size(cursor, button_size);
    let (icon, style) = match transport {
        Transport::Playing | Transport::Buffering => ("||", ButtonStyle::Active),
        Transport::Paused => (">", ButtonStyle::Normal),
        Transport::Stopped => (">", ButtonStyle::Normal),
    };
    let icon = if !has_track || !ready { ">" } else { icon };
    let style = if !has_track || !ready {
        ButtonStyle::Disabled
    } else {
        style
    };
    if PixelButton::new(icon)
        .size(button_size)
        .font_size(font_size::SMALL)
        .style(style)
        .show_at(ui, play_rect, palette)
        .clicked()
        && has_track
        && ready
    {
        app.toggle_play_pause();
    }
    cursor.x += button_size.x + 2.0;

    // Stop
    let stop_rect = Rect::from_min_size(cursor, button_size);
    if PixelButton::new("[]")
        .size(button_size)
        .font_size(font_size::TINY)
        .icon_only()
        .style(if has_track {
            ButtonStyle::Normal
        } else {
            ButtonStyle::Disabled
        })
        .show_at(ui, stop_rect, palette)
        .clicked()
        && has_track
    {
        app.stop_playback();
    }
    cursor.x += button_size.x + 2.0;

    // Next
    let next_rect = Rect::from_min_size(cursor, button_size);
    if PixelButton::new(">|")
        .size(button_size)
        .font_size(font_size::TINY)
        .icon_only()
        .style(if ready {
            ButtonStyle::Normal
        } else {
            ButtonStyle::Disabled
        })
        .show_at(ui, next_rect, palette)
        .clicked()
        && ready
    {
        app.next_track();
    }
    cursor.x += button_size.x + 6.0;

    // Shuffle
    let shuffle_rect = Rect::from_min_size(cursor, Vec2::new(30.0, 20.0));
    let shuffled = app.player.queue.is_shuffled();
    if PixelButton::new("SHUF")
        .size(Vec2::new(30.0, 20.0))
        .font_size(font_size::TINY)
        .style(if shuffled {
            ButtonStyle::Active
        } else {
            ButtonStyle::Normal
        })
        .show_at(ui, shuffle_rect, palette)
        .clicked()
    {
        app.toggle_shuffle();
    }
    cursor.x += 32.0;

    // Repeat — cycles through the modes.
    let repeat_rect = Rect::from_min_size(cursor, Vec2::new(30.0, 20.0));
    let mode = app.player.queue.repeat_mode();
    if PixelButton::new(mode.label())
        .size(Vec2::new(30.0, 20.0))
        .font_size(font_size::TINY)
        .style(if mode == sonus_core::RepeatMode::Off {
            ButtonStyle::Normal
        } else {
            ButtonStyle::Active
        })
        .show_at(ui, repeat_rect, palette)
        .clicked()
    {
        app.cycle_repeat();
    }
    cursor.x += 34.0;

    // Like the current track.
    let like_rect = Rect::from_min_size(cursor, Vec2::new(24.0, 20.0));
    let liked = app
        .current_track()
        .is_some_and(|t| app.is_liked(&t.youtube_id));
    if PixelButton::new("*")
        .size(Vec2::new(24.0, 20.0))
        .font_size(font_size::BODY)
        .icon_only()
        .style(if liked {
            ButtonStyle::Active
        } else if has_track {
            ButtonStyle::Normal
        } else {
            ButtonStyle::Disabled
        })
        .show_at(ui, like_rect, palette)
        .clicked()
    {
        if let Some(track) = app.current_track().cloned() {
            app.toggle_like(&track);
        }
    }

    // ---- volume, right-aligned -------------------------------------------
    let volume_width = 90.0;
    let volume_rect = Rect::from_min_size(
        Pos2::new(area.right() - volume_width, controls_top + 2.0),
        Vec2::new(volume_width, 16.0),
    );

    paint::text(
        ui.painter(),
        Pos2::new(volume_rect.left(), volume_rect.center().y),
        Align2::LEFT_CENTER,
        "VOL",
        body(font_size::TINY),
        palette.fg_dim,
    );

    let slider_rect = Rect::from_min_max(
        Pos2::new(volume_rect.left() + 26.0, volume_rect.top() + 2.0),
        volume_rect.max,
    );
    let mut volume = app.player.volume;
    if let Some(new_volume) = volume_slider(ui, slider_rect, volume, palette) {
        volume = new_volume;
        app.set_volume(volume);
    }

    // ---- seek bar ---------------------------------------------------------
    let seek_top = controls_top + 26.0;
    let seek_rect = Rect::from_min_size(
        Pos2::new(area.left(), seek_top),
        Vec2::new(area.width(), 14.0),
    );

    render_seek_bar(ui, seek_rect, app, palette);
}

/// Draw the seek bar and handle scrubbing.
fn render_seek_bar(ui: &mut egui::Ui, rect: Rect, app: &mut SonusApp, palette: &Palette) {
    let has_track = app.current_track().is_some();
    let duration = app
        .current_track()
        .and_then(|t| t.duration_secs)
        .unwrap_or(0);

    let response = ui.interact(
        rect,
        ui.id().with("seek"),
        if has_track {
            Sense::click_and_drag()
        } else {
            Sense::hover()
        },
    );

    // Scrubbing: track the drag locally and only commit on release, so we
    // don't spawn a fresh yt-dlp pipeline for every pixel of movement.
    if has_track && duration > 0 {
        if response.drag_started() {
            app.scrubbing = Some(app.progress());
        }
        if response.dragged() {
            if let Some(pointer) = response.interact_pointer_pos() {
                let t = ((pointer.x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0);
                app.scrubbing = Some(t);
            }
        }
        if response.drag_stopped() {
            if let Some(t) = app.scrubbing.take() {
                app.seek_to(t * duration as f32);
            }
        }
        // A plain click jumps straight there.
        if response.clicked() {
            if let Some(pointer) = response.interact_pointer_pos() {
                let t = ((pointer.x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0);
                app.seek_to(t * duration as f32);
            }
        }
    }

    let progress = app.progress();
    let position = if let Some(scrub) = app.scrubbing {
        scrub * duration as f32
    } else {
        app.position_secs
    };

    // Buffering indicator: a dim fill ahead of the play head.
    let buffered = app.audio.status().buffered_samples;
    let buffered_fraction = if buffered > 0 {
        (progress + (buffered as f32 / 96_000.0 / duration.max(1) as f32)).min(1.0)
    } else {
        progress
    };

    let painter = ui.painter();
    paint::surface(painter, rect, palette.lcd_bg, paint::Bevel::Sunken, palette);

    let inner = rect.shrink(1.0);

    // Buffered region.
    if buffered_fraction > progress {
        let buffered_rect = Rect::from_min_size(
            inner.min,
            Vec2::new((inner.width() * buffered_fraction).round(), inner.height()),
        );
        paint::fill(painter, buffered_rect, palette.dim(palette.primary, 0.75));
    }

    // Played region.
    let played_rect = Rect::from_min_size(
        inner.min,
        Vec2::new((inner.width() * progress).round(), inner.height()),
    );
    if played_rect.width() >= 1.0 {
        paint::fill(painter, played_rect, palette.primary);
    }

    // Play head.
    if has_track {
        let head_x = paint::snap(inner.left() + inner.width() * progress)
            .clamp(inner.left(), inner.right() - 3.0);
        let head = Rect::from_min_size(
            Pos2::new(head_x, rect.top() - 1.0),
            Vec2::new(3.0, rect.height() + 2.0),
        );
        paint::fill(painter, head, palette.lighten(palette.primary, 0.4));
    }

    // Time labels under the bar.
    let position_label = format_clock(position.max(0.0) as u64);
    let duration_label = if duration > 0 {
        format_clock(duration)
    } else {
        "--:--".to_string()
    };

    paint::text(
        painter,
        Pos2::new(rect.left(), rect.bottom() + 1.0),
        Align2::LEFT_TOP,
        &position_label,
        body(font_size::TINY),
        palette.fg_dim,
    );
    paint::text(
        painter,
        Pos2::new(rect.right(), rect.bottom() + 1.0),
        Align2::RIGHT_TOP,
        &duration_label,
        body(font_size::TINY),
        palette.fg_dim,
    );

    if response.hovered() && has_track {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
}

/// A compact volume slider drawn directly into a rect.
///
/// Returns the new value when the user changes it.
fn volume_slider(ui: &mut egui::Ui, rect: Rect, value: f32, palette: &Palette) -> Option<f32> {
    let response = ui.interact(rect, ui.id().with("volume"), Sense::click_and_drag());
    let mut new_value = None;

    if let Some(pointer) = response.interact_pointer_pos() {
        if response.dragged() || response.clicked() {
            new_value = Some(((pointer.x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0));
        }
    }

    let shown = new_value.unwrap_or(value);
    let painter = ui.painter();
    paint::surface(painter, rect, palette.lcd_bg, paint::Bevel::Sunken, palette);

    // Blocky meter rather than a smooth bar — reads better at this size.
    const SEGMENTS: usize = 12;
    let lit = (shown * SEGMENTS as f32).round() as usize;
    paint::meter(
        painter,
        rect.shrink(1.0),
        SEGMENTS,
        lit,
        palette,
        palette.primary,
        palette.accent,
    );

    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    new_value
}
