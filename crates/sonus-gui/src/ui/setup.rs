//! First-run setup overlay.
//!
//! Shown when yt-dlp or ffmpeg is missing. The window is otherwise dead
//! without them, so this replaces the whole frame: a small panel listing what
//! needs installing, a progress bar per tool, and a single "INSTALL" action.

use egui::{Align2, Pos2, Rect, Vec2};

use crate::app::SonusApp;
use crate::pixel::{body, font_size, paint, ButtonStyle, Palette, PixelButton};

/// Draw the setup overlay, blocking the rest of the window.
pub fn render(app: &mut SonusApp, ctx: &egui::Context, palette: &Palette) {
    let panel_w = 320.0;
    let panel_h = 210.0;
    let rect = Rect::from_center_size(
        Pos2::new(ctx.screen_rect().center().x, ctx.screen_rect().center().y),
        Vec2::new(panel_w, panel_h),
    );
    let rect = paint::snap_rect(rect);

    // Dim the rest of the window behind the panel.
    let dimmer = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("setup_dimmer"),
    ));
    paint::fill(&dimmer, ctx.screen_rect(), palette.dim(palette.panel_dark, 0.4));

    // The panel itself lives in an Area so we get a real Ui to lay buttons
    // into, while still drawing the chrome by hand.
    egui::Area::new(egui::Id::new("setup_overlay"))
        .fixed_pos(rect.min)
        .interactable(true)
        .show(ctx, |ui| {
            ui.set_min_size(rect.size());
            ui.set_max_size(rect.size());

            let painter = ui.painter();
            let content = paint::titled_panel(
                painter,
                rect,
                "SETUP",
                palette,
                body(font_size::HEADER),
            );
            let inner = Rect::from_min_max(
                Pos2::new(content.left() + 8.0, content.top() + 6.0),
                Pos2::new(content.right() - 8.0, content.bottom() - 8.0),
            );

            let mut y = inner.top();

            let intro = if app.dependency_problems.len() == 1 {
                "Sonus needs one tool to stream audio:"
            } else {
                "Sonus needs two tools to stream audio:"
            };
            paint::text(
                painter,
                Pos2::new(inner.left(), y),
                Align2::LEFT_TOP,
                intro,
                body(font_size::TINY),
                palette.fg,
            );
            y += 16.0;

            let entries = app.setup.entries();
            if entries.is_empty() {
                paint::text(
                    painter,
                    Pos2::new(inner.left(), y),
                    Align2::LEFT_TOP,
                    "Preparing…",
                    body(font_size::TINY),
                    palette.fg_dim,
                );
            } else {
                for entry in &entries {
                    let name = entry.dep.name();
                    let (status, color) = if entry.done {
                        ("OK", palette.primary)
                    } else if let Some(err) = &entry.error {
                        (err.as_str(), palette.danger)
                    } else {
                        (entry.label.as_str(), palette.accent)
                    };
                    paint::text(
                        painter,
                        Pos2::new(inner.left(), y),
                        Align2::LEFT_TOP,
                        name,
                        body(font_size::TINY),
                        palette.fg,
                    );
                    paint::text(
                        painter,
                        Pos2::new(inner.right(), y),
                        Align2::RIGHT_TOP,
                        status,
                        body(font_size::TINY),
                        color,
                    );
                    y += 14.0;

                    if !entry.done && entry.error.is_none() {
                        let bar_rect =
                            Rect::from_min_max(Pos2::new(inner.left(), y), Pos2::new(inner.right(), y + 6.0));
                        paint::outline(painter, bar_rect, palette.border);
                        let filled = bar_rect.width() * entry.progress.clamp(0.0, 1.0);
                        paint::fill(
                            painter,
                            Rect::from_min_max(bar_rect.min, Pos2::new(bar_rect.left() + filled, bar_rect.bottom())),
                            palette.primary,
                        );
                        y += 10.0;
                    }
                }
            }

            // Action buttons at the bottom.
            let btn_w = 72.0;
            let btn_h = 22.0;
            let gap = 8.0;
            let total = btn_w * 2.0 + gap;
            let left = inner.left() + (inner.width() - total) / 2.0;
            let btn_top = inner.bottom() - btn_h;

            let install_rect = Rect::from_min_size(Pos2::new(left, btn_top), Vec2::new(btn_w, btn_h));
            let quit_rect = Rect::from_min_size(
                Pos2::new(left + btn_w + gap, btn_top),
                Vec2::new(btn_w, btn_h),
            );

            let install_label = if app.setup.has_failed() {
                "RETRY"
            } else if app.setup.is_running() {
                "…"
            } else {
                "INSTALL"
            };
            let install_style = if app.setup.is_running() {
                ButtonStyle::Disabled
            } else {
                ButtonStyle::Normal
            };

            if PixelButton::new(install_label)
                .size(install_rect.size())
                .style(install_style)
                .show_at(ui, install_rect, palette)
                .clicked()
            {
                app.start_dependency_setup();
            }

            if PixelButton::new("QUIT")
                .size(quit_rect.size())
                .style(ButtonStyle::Normal)
                .show_at(ui, quit_rect, palette)
                .clicked()
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });

    // Keep repainting while an install is in flight so the progress bars move.
    if app.setup.is_running() {
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }
}
