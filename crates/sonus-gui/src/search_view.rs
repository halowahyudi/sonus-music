use egui::*;

use crate::app::SonusApp;

/// Search bar + playlist results — fills the remaining space.
pub fn render(ctx: &egui::Context, app: &mut SonusApp) {
    let colors = app.theme_colors.clone();

    egui::CentralPanel::default()
        .frame(egui::Frame::default().fill(colors.bg).inner_margin(0.0))
        .show(ctx, |ui| {
            // === Search bar ===
            ui.horizontal(|ui| {
                ui.add_space(8.0);

                let resp = ui.add(
                    egui::TextEdit::singleline(&mut app.search_input)
                        .hint_text("Search music...")
                        .desired_width(ui.available_width() - 60.0)
                        .clip_text(true)
                        .text_color(colors.fg)
                        .id(egui::Id::new("search_input")),
                );

                if app.focus_search {
                    resp.request_focus();
                    app.focus_search = false;
                }

                if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    app.start_search();
                }

                ui.add_space(4.0);

                if ui.add_sized(
                    [50.0, 22.0],
                    egui::Button::new(
                        RichText::new("Go")
                            .color(colors.bg)
                            .size(12.0)
                            .strong(),
                    )
                    .fill(colors.primary),
                ).clicked() {
                    app.start_search();
                }
            });

            ui.add_space(4.0);

            // Separator
            let (sep, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
            ui.painter().rect_filled(sep, 0.0, colors.border);

            // === Status / loading / results ===
            if app.search_rx.is_some() {
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.spinner();
                    ui.label(RichText::new("Searching...").color(colors.secondary).size(12.0));
                });
                return;
            }

            if let Some(err) = &app.search_error.clone() {
                ui.vertical(|ui| {
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        ui.label(
                            RichText::new(format!("Error: {err}"))
                                .color(Color32::from_rgb(0xff, 0x6b, 0x6b))
                                .size(12.0),
                        );
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        if ui.button("Retry").clicked() {
                            app.search_error = None;
                            app.start_search();
                        }
                    });
                });
                return;
            }

            if app.search_results.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.label(
                        RichText::new("Type a song name and press Go")
                            .color(colors.secondary)
                            .size(13.0),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("YouTube Music - songs only")
                            .color(colors.border)
                            .size(10.0),
                    );
                });
                return;
            }

            // === Playlist ===
            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    let current_id = app
                        .last_snapshot
                        .current_track
                        .as_ref()
                        .map(|t| t.youtube_id.clone())
                        .unwrap_or_default();

                    let results = app.search_results.clone();
                    let c = colors.clone();

                    for (i, result) in results.iter().enumerate() {
                        let is_playing = result.youtube_id == current_id;
                        let row_h = 28.0;
                        let (rect, resp) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), row_h),
                            egui::Sense::click(),
                        );

                        // Row background
                        let bg = if is_playing {
                            c.highlight
                        } else if resp.hovered() {
                            c.hover
                        } else if i % 2 == 1 {
                            egui::Color32::from_rgba_unmultiplied(c.bg.r(), c.bg.g(), c.bg.b(), 30)
                        } else {
                            Color32::TRANSPARENT
                        };

                        if bg != Color32::TRANSPARENT {
                            ui.painter().rect(rect, 2.0, bg, egui::Stroke::NONE);
                        }

                        // Index / playing icon
                        let idx_str = if is_playing { ">".to_string() } else { format!("{}", i + 1) };
                        ui.painter().text(
                            rect.min + egui::vec2(8.0, row_h / 2.0),
                            Align2::LEFT_CENTER,
                            &idx_str,
                            egui::FontId::monospace(10.0),
                            if is_playing { c.primary } else { c.secondary },
                        );

                        // Title - artist
                        let artist = result.artist.as_deref().unwrap_or("");
                        let display = if artist.is_empty() {
                            result.title.clone()
                        } else {
                            format!("{} - {}", result.title, artist)
                        };

                        ui.painter().text(
                            rect.min + egui::vec2(36.0, row_h / 2.0),
                            Align2::LEFT_CENTER,
                            &display,
                            egui::FontId::proportional(11.0),
                            if is_playing { c.primary } else { c.fg },
                        );

                        // Duration
                        if let Some(dur) = result.duration_secs {
                            let dur_str = format_duration(dur);
                            ui.painter().text(
                                rect.max - egui::vec2(8.0, row_h / 2.0),
                                Align2::RIGHT_CENTER,
                                &dur_str,
                                egui::FontId::monospace(10.0),
                                c.secondary,
                            );
                        }

                        if resp.clicked() {
                            app.selected_index = i;
                            app.play_selected();
                        }
                    }

                    ui.add_space(8.0);
                });
        });
}

fn format_duration(secs: u64) -> String {
    let m = secs / 60;
    let s = secs % 60;
    format!("{m}:{s:02}")
}
