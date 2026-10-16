use egui::{Color32, Pos2, Rect, Sense, Ui, Vec2, ViewportCommand, FontId, Align2};

/// A Winamp-style panel with a title bar (centered title, ornaments, min/close).
/// Returns the content area rect for custom painting.
pub struct Panel;

impl Panel {
    /// Draw a panel with title bar into the given Ui.
    /// Returns the content rect (below the title bar).
    pub fn show(
        ui: &mut Ui,
        title: &str,
        bg: Color32,
        border: Color32,
        accent: Color32,
        fg: Color32,
        height: f32,
        on_close: Option<&dyn Fn()>,
    ) -> Rect {
        let total_h = height;
        let (panel_rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), total_h), Sense::hover());

        // Panel background
        ui.painter().rect_filled(panel_rect, 0.0, bg);
        ui.painter().rect_stroke(panel_rect, 0.0, egui::Stroke::new(1.0_f32, border));

        // Title bar
        let title_h = 18.0;
        let title_rect = Rect::from_min_size(panel_rect.min, Vec2::new(panel_rect.width(), title_h));
        ui.painter().rect_filled(title_rect, 0.0, border);

        // Double-line ornaments on both sides of title
        let title_text_w = title.len() as f32 * 6.0;
        let center_x = title_rect.center().x;
        let text_left = center_x - title_text_w / 2.0;
        let text_right = center_x + title_text_w / 2.0;
        let orn_y = title_rect.center().y;

        // Left ornaments
        for i in 0..2 {
            let x = text_left - 12.0 - i as f32 * 4.0;
            ui.painter().line_segment(
                [Pos2::new(x, orn_y - 3.0), Pos2::new(x, orn_y + 3.0)],
                egui::Stroke::new(1.0_f32, accent),
            );
        }
        // Right ornaments
        for i in 0..2 {
            let x = text_right + 8.0 + i as f32 * 4.0;
            ui.painter().line_segment(
                [Pos2::new(x, orn_y - 3.0), Pos2::new(x, orn_y + 3.0)],
                egui::Stroke::new(1.0_f32, accent),
            );
        }

        // Title text
        ui.painter().text(
            Pos2::new(center_x, orn_y),
            Align2::CENTER_CENTER,
            title,
            FontId::proportional(11.0),
            fg,
        );

        // Close button (top right)
        let close_rect = Rect::from_min_size(
            Pos2::new(panel_rect.right() - 16.0, panel_rect.min.y + 3.0),
            Vec2::new(12.0, 12.0),
        );
        let close_resp = ui.interact(close_rect, ui.id().with("close"), Sense::click());
        ui.painter().text(
            close_rect.center(),
            Align2::CENTER_CENTER,
            "x",
            FontId::monospace(10.0),
            if close_resp.hovered() { Color32::from_rgb(0xff, 0x55, 0x55) } else { fg },
        );
        if close_resp.clicked() {
            if let Some(cb) = on_close {
                cb();
            }
        }

        // Drag area on title bar (for frameless window)
        let drag_resp = ui.interact(
            Rect::from_min_size(panel_rect.min, Vec2::new(panel_rect.width() - 30.0, title_h)),
            ui.id().with("drag"),
            Sense::drag(),
        );
        if drag_resp.drag_started() {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }

        // Content area (below title bar)
        Rect::from_min_size(
            Pos2::new(panel_rect.min.x, panel_rect.min.y + title_h),
            Vec2::new(panel_rect.width(), total_h - title_h),
        )
    }
}
