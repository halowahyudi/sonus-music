use egui::{Color32, FontId, Painter, Pos2, Rect, Align2};

/// Seven-segment digital display — classic Winamp timer.
/// Each digit is drawn with lit/unlit segments using the Painter.
pub struct SevenSeg<'a> {
    text: &'a str,
    color: Color32,
    dim: Color32,
    size: f32,
}

impl<'a> SevenSeg<'a> {
    pub fn new(text: &'a str, color: Color32, dim: Color32) -> Self {
        Self { text, color, dim, size: 28.0 }
    }

    pub fn size(mut self, s: f32) -> Self { self.size = s; self }

    /// Paint the display at the given position.
    pub fn paint(&self, painter: &Painter, pos: Pos2) {
        let char_w = self.size * 0.6;
        let gap = self.size * 0.15;
        let mut x = pos.x;
        for ch in self.text.chars() {
            match ch {
                '0'..='9' | '-' | ' ' => {
                    let segs = segments(ch);
                    self.paint_digit(painter, Pos2::new(x, pos.y), segs);
                    x += char_w + gap;
                }
                ':' | '.' => {
                    painter.circle_filled(Pos2::new(x + char_w * 0.3, pos.y + self.size * 0.35), self.size * 0.06, self.color);
                    painter.circle_filled(Pos2::new(x + char_w * 0.3, pos.y + self.size * 0.65), self.size * 0.06, self.color);
                    x += char_w * 0.5 + gap;
                }
                _ => {
                    painter.text(Pos2::new(x, pos.y), Align2::LEFT_TOP, ch, FontId::monospace(self.size), self.color);
                    x += char_w + gap;
                }
            }
        }
    }

    fn paint_digit(&self, painter: &Painter, pos: Pos2, segs: [bool; 7]) {
        let s = self.size;
        let w = s * 0.6;
        let t = s * 0.08; // segment thickness
        let h = s * 0.42; // half height
        let lit = self.color;
        let unlit = self.dim;

        // Segment layout (standard 7-seg):
        //  _aaa_
        // f|    |b
        //  _ggg_
        // e|    |c
        //  _ddd_

        // a (top)
        painter.rect_filled(Rect::from_min_size(Pos2::new(pos.x + t, pos.y), egui::vec2(w - 2.0 * t, t)), 0.0, if segs[0] { lit } else { unlit });
        // b (top-right)
        painter.rect_filled(Rect::from_min_size(Pos2::new(pos.x + w - t, pos.y + t), egui::vec2(t, h - t)), 0.0, if segs[1] { lit } else { unlit });
        // c (bottom-right)
        painter.rect_filled(Rect::from_min_size(Pos2::new(pos.x + w - t, pos.y + h + t), egui::vec2(t, h - t)), 0.0, if segs[2] { lit } else { unlit });
        // d (bottom)
        painter.rect_filled(Rect::from_min_size(Pos2::new(pos.x + t, pos.y + s - t), egui::vec2(w - 2.0 * t, t)), 0.0, if segs[3] { lit } else { unlit });
        // e (bottom-left)
        painter.rect_filled(Rect::from_min_size(Pos2::new(pos.x, pos.y + h + t), egui::vec2(t, h - t)), 0.0, if segs[4] { lit } else { unlit });
        // f (top-left)
        painter.rect_filled(Rect::from_min_size(Pos2::new(pos.x, pos.y + t), egui::vec2(t, h - t)), 0.0, if segs[5] { lit } else { unlit });
        // g (middle)
        painter.rect_filled(Rect::from_min_size(Pos2::new(pos.x + t, pos.y + h), egui::vec2(w - 2.0 * t, t)), 0.0, if segs[6] { lit } else { unlit });
    }
}

fn segments(ch: char) -> [bool; 7] {
    // [a, b, c, d, e, f, g]
    match ch {
        '0' => [true, true, true, true, true, true, false],
        '1' => [false, true, true, false, false, false, false],
        '2' => [true, true, false, true, true, false, true],
        '3' => [true, true, true, true, false, false, true],
        '4' => [false, true, true, false, false, true, true],
        '5' => [true, false, true, true, false, true, true],
        '6' => [true, false, true, true, true, true, true],
        '7' => [true, true, true, false, false, false, false],
        '8' => [true, true, true, true, true, true, true],
        '9' => [true, true, true, true, false, true, true],
        '-' => [false, false, false, false, false, false, true],
        _ => [false; 7],
    }
}
