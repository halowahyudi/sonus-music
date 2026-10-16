use egui::{Color32, Pos2, Rect, Response, Sense, Ui, Vec2};

/// Custom Winamp-style horizontal slider — green fill, square thumb.
pub struct WinampSlider {
    value: f32,
    min: f32,
    max: f32,
    fill_color: Color32,
    bg_color: Color32,
    thumb_color: Color32,
    height: f32,
}

impl WinampSlider {
    pub fn new(value: f32, min: f32, max: f32) -> Self {
        Self {
            value, min, max,
            fill_color: Color32::from_rgb(0x2e, 0xe8, 0x6b),
            bg_color: Color32::from_rgb(0x1b, 0x22, 0x27),
            thumb_color: Color32::from_rgb(0x2e, 0xe8, 0x6b),
            height: 10.0,
        }
    }

    pub fn colors(mut self, fill: Color32, bg: Color32, thumb: Color32) -> Self {
        self.fill_color = fill;
        self.bg_color = bg;
        self.thumb_color = thumb;
        self
    }

    pub fn height(mut self, h: f32) -> Self { self.height = h; self }

    /// Show the slider in the UI. Returns the response and new value.
    pub fn show(self, ui: &mut Ui, _id_str: &str) -> (Response, f32) {
        let desired = Vec2::new(ui.available_width(), self.height + 10.0);
        let (rect, response) = ui.allocate_exact_size(desired, Sense::drag());

        let mut value = self.value;
        if response.dragged() {
            let t = if rect.width() > 0.0 {
                ((response.interact_pointer_pos().unwrap_or(rect.center()).x - rect.left()) / rect.width()).clamp(0.0, 1.0)
            } else { 0.0 };
            value = self.min + t * (self.max - self.min);
        }

        let painter = ui.painter();
        let track_rect = Rect::from_min_size(
            Pos2::new(rect.left(), rect.center().y - self.height / 2.0),
            Vec2::new(rect.width(), self.height),
        );

        // Background
        painter.rect_filled(track_rect, 1.0, self.bg_color);

        // Fill
        let t = (value - self.min) / (self.max - self.min).max(0.001);
        let fill_w = track_rect.width() * t;
        painter.rect_filled(
            Rect::from_min_size(track_rect.min, Vec2::new(fill_w, self.height)),
            1.0, self.fill_color,
        );

        // Thumb — square with grip lines
        let thumb_size = self.height + 4.0;
        let thumb_x = track_rect.left() + fill_w - thumb_size / 2.0;
        let thumb_rect = Rect::from_min_size(
            Pos2::new(thumb_x, track_rect.center().y - thumb_size / 2.0),
            Vec2::new(thumb_size, thumb_size),
        );
        painter.rect_filled(thumb_rect, 1.0, self.thumb_color);

        // Grip lines on thumb
        let grip_color = self.bg_color;
        for i in 0..3 {
            let gx = thumb_rect.center().x - 2.0 + i as f32 * 2.0;
            painter.line_segment(
                [Pos2::new(gx, thumb_rect.top() + 2.0), Pos2::new(gx, thumb_rect.bottom() - 2.0)],
                egui::Stroke::new(1.0_f32, grip_color),
            );
        }

        (response, value)
    }
}

/// Vertical slider for EQ bands.
pub struct WinampSliderBuilder;

impl WinampSliderBuilder {
    /// Draw a vertical EQ slider. Returns (response, new_db_value).
    pub fn vertical(
        ui: &mut Ui,
        _id_str: &str,
        value: f32,
        min: f32,
        max: f32,
        center: f32,
        fill: Color32,
        bg: Color32,
    ) -> (Response, f32) {
        let w = 20.0;
        let h = 80.0;
        let (rect, response) = ui.allocate_exact_size(Vec2::new(w, h), Sense::drag());

        let mut val = value;
        if response.dragged() {
            let t = if h > 0.0 {
                1.0 - ((response.interact_pointer_pos().unwrap_or(rect.center()).y - rect.top()) / h).clamp(0.0, 1.0)
            } else { 0.0 };
            val = min + t * (max - min);
        }

        let painter = ui.painter();
        let track = Rect::from_min_size(Pos2::new(rect.center().x - 3.0, rect.top()), Vec2::new(6.0, h));

        // Background
        painter.rect_filled(track, 1.0, bg);

        // Center line
        let center_y = track.bottom() - ((center - min) / (max - min)) * track.height();
        painter.line_segment(
            [Pos2::new(track.left() - 2.0, center_y), Pos2::new(track.right() + 2.0, center_y)],
            egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(0x55, 0x55, 0x55)),
        );

        // Fill from center to value
        let val_y = track.bottom() - ((val - min) / (max - min)) * track.height();
        if val >= center {
            painter.rect_filled(
                Rect::from_two_pos(Pos2::new(track.left(), center_y), Pos2::new(track.right(), val_y)),
                0.0, fill,
            );
        } else {
            painter.rect_filled(
                Rect::from_two_pos(Pos2::new(track.left(), val_y), Pos2::new(track.right(), center_y)),
                0.0, fill,
            );
        }

        // Thumb
        let thumb_h = 8.0;
        let thumb_rect = Rect::from_min_size(
            Pos2::new(rect.center().x - 8.0, val_y - thumb_h / 2.0),
            Vec2::new(16.0, thumb_h),
        );
        painter.rect_filled(thumb_rect, 1.0, fill);

        (response, val)
    }
}
