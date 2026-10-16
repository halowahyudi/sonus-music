use egui::{Color32, Painter, Pos2, Rect, Stroke};

/// EQ response curve — Catmull-Rom spline through band gains.
pub struct EqCurve;

impl EqCurve {
    /// Paint the response curve through 10 band values (0..1, 0.5 = center).
    pub fn paint(
        painter: &Painter,
        rect: Rect,
        band_values: &[f32],
        color: Color32,
    ) {
        if band_values.len() < 2 { return; }

        let w = rect.width();
        let h = rect.height();
        let n = band_values.len() as f32;

        // Center line
        painter.line_segment(
            [Pos2::new(rect.left(), rect.center().y), Pos2::new(rect.right(), rect.center().y)],
            Stroke::new(1.0_f32, Color32::from_rgb(0x44, 0x44, 0x44)),
        );

        // Catmull-Rom spline interpolation
        let points: Vec<Pos2> = (0..band_values.len())
            .map(|i| {
                let x = rect.left() + (i as f32 / (n - 1.0)) * w;
                let y = rect.bottom() - band_values[i] * h;
                Pos2::new(x, y)
            })
            .collect();

        // Draw line segments between interpolated points
        let segments = 80;
        let mut last = points[0];
        for s in 1..=segments {
            let t = s as f32 / segments as f32 * (n - 1.0);
            let pos = catmull_rom(band_values, t, rect, w, h);
            painter.line_segment([last, pos], Stroke::new(1.5_f32, color));
            last = pos;
        }
    }
}

fn catmull_rom(values: &[f32], t: f32, rect: Rect, w: f32, h: f32) -> Pos2 {
    let last = values.len() - 1;
    let i = (t.floor() as usize).min(last.saturating_sub(1));
    let u = (t - i as f32).clamp(0.0, 1.0);
    let at = |idx: isize| values[idx.clamp(0, last as isize) as usize];
    let (p0, p1, p2, p3) = (at(i as isize - 1), at(i as isize), at(i as isize + 1), at(i as isize + 2));
    let val = 0.5 * (2.0 * p1 + (-p0 + p2) * u + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * u * u + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * u * u * u);
    let val = val.clamp(0.0, 1.0);
    let x = rect.left() + t / (last as f32) * w;
    let y = rect.bottom() - val * h;
    Pos2::new(x, y)
}
