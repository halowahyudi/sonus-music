//! Seven-segment digital display.
//!
//! Draws digits from lit/unlit segments rather than using a font, so the
//! readout looks like a real VFD/LED panel — including the faint "ghost"
//! segments you see on hardware when a digit is off.

use egui::{Align2, Color32, Painter, Pos2, Rect, Vec2};

use super::paint;

/// One digit's segment pattern, ordered `[a, b, c, d, e, f, g]`:
///
/// ```text
///    _a_
///  f|   |b
///    _g_
///  e|   |c
///    _d_
/// ```
type Segments = [bool; 7];

const DIGITS: [Segments; 10] = [
    [true, true, true, true, true, true, false],     // 0
    [false, true, true, false, false, false, false], // 1
    [true, true, false, true, true, false, true],    // 2
    [true, true, true, true, false, false, true],    // 3
    [false, true, true, false, false, true, true],   // 4
    [true, false, true, true, false, true, true],    // 5
    [true, false, true, true, true, true, true],     // 6
    [true, true, true, false, false, false, false],  // 7
    [true, true, true, true, true, true, true],      // 8
    [true, true, true, true, false, true, true],     // 9
];

const MINUS: Segments = [false, false, false, false, false, false, true];
const BLANK: Segments = [false; 7];

/// Geometry of a single digit, in fractions of the digit's height.
const SEGMENT_THICKNESS: f32 = 0.14;
const DIGIT_WIDTH: f32 = 0.62;
const DIGIT_GAP: f32 = 0.18;

/// A seven-segment readout.
pub struct SevenSeg<'a> {
    text: &'a str,
    lit: Color32,
    unlit: Color32,
    height: f32,
}

impl<'a> SevenSeg<'a> {
    pub fn new(text: &'a str, lit: Color32, unlit: Color32) -> Self {
        Self {
            text,
            lit,
            unlit,
            height: 20.0,
        }
    }

    pub fn height(mut self, height: f32) -> Self {
        self.height = paint::snap(height);
        self
    }

    /// Width the readout will occupy, for layout before drawing.
    pub fn measure(&self) -> f32 {
        let digit_w = self.height * DIGIT_WIDTH;
        let gap = self.height * DIGIT_GAP;
        self.text
            .chars()
            .map(|c| match c {
                ':' | '.' => digit_w * 0.35,
                _ => digit_w + gap,
            })
            .sum()
    }

    /// Draw the readout with its top-left corner at `pos`.
    pub fn paint(&self, painter: &Painter, pos: Pos2) {
        let mut x = paint::snap(pos.x);
        let y = paint::snap(pos.y);

        for ch in self.text.chars() {
            match ch {
                '0'..='9' => {
                    let segments = DIGITS[ch as usize - '0' as usize];
                    self.paint_digit(painter, Pos2::new(x, y), segments);
                    x += self.height * DIGIT_WIDTH + self.height * DIGIT_GAP;
                }
                '-' => {
                    self.paint_digit(painter, Pos2::new(x, y), MINUS);
                    x += self.height * DIGIT_WIDTH + self.height * DIGIT_GAP;
                }
                ' ' => {
                    self.paint_digit(painter, Pos2::new(x, y), BLANK);
                    x += self.height * DIGIT_WIDTH + self.height * DIGIT_GAP;
                }
                ':' => {
                    // Colon sits in the upper and lower thirds.
                    let r = (self.height * 0.07).max(1.0);
                    let cx = x + self.height * DIGIT_WIDTH * 0.2;
                    painter.circle_filled(Pos2::new(cx, y + self.height * 0.3), r, self.lit);
                    painter.circle_filled(Pos2::new(cx, y + self.height * 0.7), r, self.lit);
                    x += self.height * DIGIT_WIDTH * 0.35;
                }
                '.' => {
                    let r = (self.height * 0.08).max(1.0);
                    painter.circle_filled(
                        Pos2::new(x + self.height * 0.15, y + self.height * 0.92),
                        r,
                        self.lit,
                    );
                    x += self.height * DIGIT_WIDTH * 0.35;
                }
                // Anything else is drawn as-is so a stray character still
                // shows up rather than silently vanishing.
                other => {
                    paint::text(
                        painter,
                        Pos2::new(x, y + self.height / 2.0),
                        Align2::LEFT_CENTER,
                        &other.to_string(),
                        super::font::body(self.height * 0.6),
                        self.lit,
                    );
                    x += self.height * DIGIT_WIDTH + self.height * DIGIT_GAP;
                }
            }
        }
    }

    fn paint_digit(&self, painter: &Painter, pos: Pos2, segments: Segments) {
        let h = self.height;
        let w = h * DIGIT_WIDTH;
        let t = (h * SEGMENT_THICKNESS).max(1.0);
        // Each half of the digit, above and below the middle bar.
        let half = (h - t) / 2.0;

        let seg = |rect: Rect, on: bool| {
            paint::fill(painter, rect, if on { self.lit } else { self.unlit });
        };

        let top = pos.y;
        let mid = pos.y + half;
        let bottom = pos.y + h - t;

        // a — top horizontal
        seg(
            Rect::from_min_size(Pos2::new(pos.x + t, top), Vec2::new(w - 2.0 * t, t)),
            segments[0],
        );
        // b — top right vertical
        seg(
            Rect::from_min_size(Pos2::new(pos.x + w - t, top + t), Vec2::new(t, half - t)),
            segments[1],
        );
        // c — bottom right vertical
        seg(
            Rect::from_min_size(Pos2::new(pos.x + w - t, mid + t), Vec2::new(t, half - t)),
            segments[2],
        );
        // d — bottom horizontal
        seg(
            Rect::from_min_size(Pos2::new(pos.x + t, bottom), Vec2::new(w - 2.0 * t, t)),
            segments[3],
        );
        // e — bottom left vertical
        seg(
            Rect::from_min_size(Pos2::new(pos.x, mid + t), Vec2::new(t, half - t)),
            segments[4],
        );
        // f — top left vertical
        seg(
            Rect::from_min_size(Pos2::new(pos.x, top + t), Vec2::new(t, half - t)),
            segments[5],
        );
        // g — middle horizontal
        seg(
            Rect::from_min_size(Pos2::new(pos.x + t, mid), Vec2::new(w - 2.0 * t, t)),
            segments[6],
        );
    }
}

/// Format a duration as `M:SS` for the seven-segment display.
///
/// Hours are folded into minutes — `65:00` rather than `1:05:00` — because a
/// six-digit readout looks better than one that grows and shrinks a digit
/// every hour.
pub fn format_clock(secs: u64) -> String {
    let minutes = secs / 60;
    let seconds = secs % 60;
    format!("{minutes}:{seconds:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::pixel::test_support::painter;

    #[test]
    fn digit_patterns_are_well_formed() {
        // 8 must light every segment, 1 exactly two.
        assert_eq!(DIGITS[8], [true; 7]);
        assert_eq!(DIGITS[1].iter().filter(|s| **s).count(), 2);
        // Every pattern has at least one lit segment.
        for (digit, pattern) in DIGITS.iter().enumerate() {
            assert!(
                pattern.iter().any(|s| *s),
                "digit {digit} has no lit segments"
            );
        }
    }

    #[test]
    fn measure_grows_with_content() {
        let short = SevenSeg::new("1", Color32::WHITE, Color32::GRAY).measure();
        let long = SevenSeg::new("12:34", Color32::WHITE, Color32::GRAY).measure();
        assert!(long > short);
    }

    #[test]
    fn measure_counts_the_colon_narrower_than_a_digit() {
        let digits = SevenSeg::new("11", Color32::WHITE, Color32::GRAY).measure();
        let colon = SevenSeg::new("::", Color32::WHITE, Color32::GRAY).measure();
        assert!(colon < digits);
    }

    #[test]
    fn paints_every_supported_character() {
        let p = painter();
        let seg = SevenSeg::new("0123456789:-. ", Color32::WHITE, Color32::GRAY);
        seg.paint(&p, Pos2::new(0.0, 0.0));
    }

    #[test]
    fn paints_unknown_characters_without_panicking() {
        let p = painter();
        SevenSeg::new("AB!?", Color32::WHITE, Color32::GRAY).paint(&p, Pos2::ZERO);
    }

    #[test]
    fn paints_at_various_heights() {
        let p = painter();
        for height in [8.0, 14.0, 20.0, 40.0, 64.0] {
            SevenSeg::new("12:34", Color32::WHITE, Color32::GRAY)
                .height(height)
                .paint(&p, Pos2::ZERO);
        }
    }

    #[test]
    fn height_snaps_to_whole_pixels() {
        let seg = SevenSeg::new("1", Color32::WHITE, Color32::GRAY).height(19.6);
        assert_eq!(seg.height, 20.0);
    }

    #[test]
    fn empty_text_draws_nothing() {
        let p = painter();
        let seg = SevenSeg::new("", Color32::WHITE, Color32::GRAY);
        assert_eq!(seg.measure(), 0.0);
        seg.paint(&p, Pos2::ZERO);
    }

    #[test]
    fn format_clock_pads_seconds() {
        assert_eq!(format_clock(0), "0:00");
        assert_eq!(format_clock(9), "0:09");
        assert_eq!(format_clock(60), "1:00");
        assert_eq!(format_clock(214), "3:34");
        assert_eq!(format_clock(3599), "59:59");
    }

    #[test]
    fn format_clock_folds_hours_into_minutes() {
        assert_eq!(format_clock(3600), "60:00");
        assert_eq!(format_clock(3900), "65:00");
    }
}
