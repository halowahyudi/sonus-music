use egui::*;

use crate::app::SonusApp;

/// Winamp-style LCD display + playback controls.
pub fn render(ctx: &egui::Context, app: &mut SonusApp) {
    let colors = app.theme_colors.clone();
    let snap = &app.last_snapshot;

    egui::TopBottomPanel::top("player")
        .exact_height(80.0)
        .frame(
            egui::Frame::default()
                .fill(colors.panel)
                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                .stroke(egui::Stroke::new(1.0_f32, colors.border)),
        )
        .show(ctx, |ui| {
            // === LCD Display ===
            let lcd_bg = Color32::from_rgb(0x0c, 0x12, 0x0c);
            let lcd_fg = Color32::from_rgb(0x00, 0xff, 0x41);
            let lcd_dim = Color32::from_rgb(0x00, 0x88, 0x22);

            let avail_w = ui.available_width();
            let (lcd_rect, _) = ui.allocate_exact_size(egui::vec2(avail_w, 36.0), egui::Sense::hover());
            ui.painter().rect_filled(lcd_rect, 3.0, lcd_bg);

            // Border around LCD
            ui.painter().rect_stroke(lcd_rect, 3.0, egui::Stroke::new(1.0_f32, colors.border));

            // Track title
            let title_text = match &snap.current_track {
                Some(track) => {
                    let artist = track.artist.as_deref().unwrap_or("Unknown");
                    format!("* {} - {}", artist, track.title)
                }
                None => "Sonus Music Player".to_string(),
            };

            ui.painter().text(
                lcd_rect.min + egui::vec2(8.0, 4.0),
                Align2::LEFT_TOP,
                &title_text,
                egui::FontId::monospace(12.0),
                lcd_fg,
            );

            // Status line
            let status = if snap.is_paused { "[PAUSED]" }
                else if snap.current_track.is_some() { "[PLAYING]" }
                else { "[STOPPED]" };

            ui.painter().text(
                lcd_rect.min + egui::vec2(8.0, 20.0),
                Align2::LEFT_TOP,
                status,
                egui::FontId::monospace(10.0),
                lcd_dim,
            );

            // Volume on LCD right
            let vol_str = format!("VOL {:3.0}%", snap.volume * 100.0);
            ui.painter().text(
                lcd_rect.max - egui::vec2(8.0, 16.0),
                Align2::RIGHT_TOP,
                &vol_str,
                egui::FontId::monospace(10.0),
                lcd_dim,
            );

            ui.add_space(4.0);

            // === Control buttons ===
            ui.horizontal(|ui| {
                let btn = |ui: &mut Ui, label: &str, color: Color32| -> Response {
                    ui.add(
                        egui::Button::new(RichText::new(label).color(color).size(14.0).strong())
                            .fill(Color32::TRANSPARENT)
                            .min_size(egui::vec2(28.0, 24.0)),
                    )
                };

                // Prev
                if btn(ui, "\u{23EE}", colors.secondary).clicked() {}

                // Play/Pause
                let has_track = snap.current_track.is_some();
                let (icon, color) = if snap.is_paused && has_track {
                    ("\u{25B6}", colors.primary)
                } else if has_track {
                    ("\u{23F8}", colors.primary)
                } else {
                    ("\u{25B6}", colors.border)
                };

                if btn(ui, icon, color).clicked() && has_track {
                    if snap.is_paused { app.playback.resume(); }
                    else { app.playback.pause(); }
                }

                // Stop
                if btn(ui, "\u{23F9}", colors.secondary).clicked() {
                    app.playback.stop();
                }

                // Next
                if btn(ui, "\u{23ED}", colors.secondary).clicked() {}

                ui.add_space(6.0);

                // Volume slider
                let mut vol = snap.volume;
                let vr = ui.add_sized(
                    [80.0, 16.0],
                    egui::Slider::new(&mut vol, 0.0..=1.0).show_value(false),
                );
                if vr.changed() { app.playback.set_volume(vol); }
            });
        });
}
