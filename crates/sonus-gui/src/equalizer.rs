use egui::*;

use crate::app::SonusApp;
use crate::widgets::panel::Panel;
use sonus_audio::{EQ_BANDS, EQ_MIN_DB, EQ_MAX_DB};

pub fn render(ctx: &egui::Context, app: &mut SonusApp) {
    let c = app.theme_colors.clone();

    egui::TopBottomPanel::top("eq_panel")
        .exact_height(140.0)
        .frame(egui::Frame::default().fill(c.bg).inner_margin(egui::Margin::same(4.0)))
        .show(ctx, |ui| {
            let content = Panel::show(
                ui, "EQUALIZER", c.panel, c.border, c.primary, c.fg, 132.0,
                Some(&|| {}),
            );

            let painter = ui.painter();

            // ON/AUTO toggles + presets
            let mut eq = app.playback.eq.lock().unwrap();

            // ON button
            let on_rect = Rect::from_min_size(
                content.min + vec2(8.0, 4.0),
                vec2(30.0, 14.0),
            );
            let on_resp = ui.interact(on_rect, ui.id().with("eq_on"), Sense::click());
            painter.rect(on_rect, 0.0, if eq.is_enabled() { c.highlight } else { c.bg }, egui::Stroke::new(1.0_f32, c.border));
            painter.text(on_rect.center(), Align2::CENTER_CENTER, "ON",
                egui::FontId::proportional(9.0), if eq.is_enabled() { c.primary } else { c.secondary });
            if on_resp.clicked() {
                let enabled = eq.is_enabled();
                eq.set_enabled(!enabled);
            }

            // AUTO button (flat)
            let auto_rect = Rect::from_min_size(content.min + vec2(42.0, 4.0), vec2(36.0, 14.0));
            painter.rect(auto_rect, 0.0, c.bg, egui::Stroke::new(1.0_f32, c.border));
            painter.text(auto_rect.center(), Align2::CENTER_CENTER, "AUTO", egui::FontId::proportional(9.0), c.secondary);

            // Presets dropdown
            let preset_rect = Rect::from_min_size(content.min + vec2(content.width() - 80.0, 4.0), vec2(72.0, 14.0));
            let _preset_resp = ui.interact(preset_rect, ui.id().with("eq_preset"), Sense::click());
            painter.rect(preset_rect, 0.0, c.bg, egui::Stroke::new(1.0_f32, c.border));
            painter.text(preset_rect.center(), Align2::CENTER_CENTER, "PRESETS", egui::FontId::proportional(9.0), c.secondary);

            // Response curve graph
            let graph_rect = Rect::from_min_size(
                pos2(content.min.x + 86.0, content.min.y + 4.0),
                Vec2::new(content.width() - 170.0, 20.0),
            );
            painter.rect_filled(graph_rect, 0.0, c.bg);
            // Map band values to 0..1 (0.5 = center)
            let band_vals: Vec<f32> = (0..10).map(|i| {
                (eq.band(i) - EQ_MIN_DB) / (EQ_MAX_DB - EQ_MIN_DB)
            }).collect();
            crate::widgets::eq_graph::EqCurve::paint(&painter, graph_rect, &band_vals, c.primary);

            // PREAMP slider (placeholder — vertical slider not used here yet)
            let preamp_x = content.min.x + 8.0;
            let _preamp_y = content.min.y + 30.0;

            // 10 band sliders
            let band_start_x = content.min.x + 60.0;
            let band_spacing = (content.width() - 80.0) / 10.0;
            let band_y = content.min.y + 30.0;
            let band_h = 70.0;

            for i in 0..10 {
                let bx = band_start_x + i as f32 * band_spacing;
                let band_val = (eq.band(i) - EQ_MIN_DB) / (EQ_MAX_DB - EQ_MIN_DB);

                // Draw vertical slider manually
                let track = Rect::from_min_size(pos2(bx, band_y), vec2(4.0, band_h));
                painter.rect_filled(track, 0.0, c.bg);

                // Center line
                let center_y = band_y + band_h * 0.5;
                painter.line_segment(
                    [pos2(bx - 2.0, center_y), pos2(bx + 6.0, center_y)],
                    egui::Stroke::new(1.0_f32, Color32::from_rgb(0x44, 0x44, 0x44)),
                );

                // Fill from center
                let val_y = band_y + band_h - band_val * band_h;
                if band_val > 0.5 {
                    painter.rect_filled(Rect::from_two_pos(pos2(bx, center_y), pos2(bx + 4.0, val_y)), 0.0, c.primary);
                } else {
                    painter.rect_filled(Rect::from_two_pos(pos2(bx, val_y), pos2(bx + 4.0, center_y)), 0.0, c.primary);
                }

                // Thumb
                let thumb_rect = Rect::from_min_size(pos2(bx - 4.0, val_y - 3.0), vec2(12.0, 6.0));
                painter.rect_filled(thumb_rect, 0.0, c.primary);

                // Interact
                let interact_rect = Rect::from_min_size(pos2(bx - 6.0, band_y), vec2(16.0, band_h));
                let resp = ui.interact(interact_rect, ui.id().with("eq_band").with(i), Sense::click_and_drag());
                if resp.dragged() || resp.clicked() {
                    if let Some(pos) = resp.interact_pointer_pos() {
                        let t = (1.0 - ((pos.y - band_y) / band_h).clamp(0.0, 1.0)).clamp(0.0, 1.0);
                        let db = EQ_MIN_DB + t * (EQ_MAX_DB - EQ_MIN_DB);
                        eq.set_band(i, db);
                    }
                }

                // Frequency label
                let freq = EQ_BANDS[i];
                let label = if freq >= 1000.0 { format!("{:.0}K", freq / 1000.0) } else { format!("{:.0}", freq) };
                painter.text(
                    pos2(bx + 2.0, band_y + band_h + 2.0),
                    Align2::CENTER_TOP,
                    &label,
                    egui::FontId::proportional(7.0),
                    c.secondary,
                );
            }

            // PREAMP label + slider
            painter.text(pos2(preamp_x + 2.0, band_y + band_h + 2.0), Align2::CENTER_TOP, "PRE", egui::FontId::proportional(7.0), c.secondary);
        });
}
