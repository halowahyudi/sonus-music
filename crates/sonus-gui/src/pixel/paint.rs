//! Low-level pixel-art drawing primitives.
//!
//! Everything the UI draws goes through here. The rules that make it read as
//! pixel art rather than "dark theme with square corners":
//!
//! 1. **No anti-aliasing.** Edge feathering is disabled globally (see
//!    `pixel::font`), so every fill has a hard edge.
//! 2. **Integer coordinates.** Shapes snap to whole pixels; a 1px line at
//!    y=10.5 becomes a 2px blurry smear.
//! 3. **Bevels, not shadows.** Depth comes from a light edge on the top/left
//!    and a dark edge on the bottom/right — the classic 3D plastic look.
//! 4. **A fixed grid.** Sizes are multiples of `UNIT` so nothing lands on a
//!    half-pixel.

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, Vec2};

use super::palette::Palette;

/// Base spacing unit. Every measurement in the UI is a multiple of this.
pub const UNIT: f32 = 2.0;

/// Border thickness — always exactly 1 physical pixel.
pub const BORDER: f32 = 1.0;

/// Snap a coordinate to the physical pixel grid.
///
/// Points are logical units; on a 1.25× display a "whole" logical pixel is
/// still 1.25 physical ones, so rounding to integers in logical space isn't
/// enough to keep 1px lines crisp. Rounding in physical space and converting
/// back is what actually lands shapes on the grid.
#[inline]
pub fn snap(v: f32) -> f32 {
    let ppp = super::font::pixels_per_point();
    if ppp <= 0.0 {
        return v.round();
    }
    (v * ppp).round() / ppp
}

/// Snap a length so it covers a whole number of physical pixels.
#[inline]
pub fn snap_len(v: f32) -> f32 {
    let ppp = super::font::pixels_per_point();
    if ppp <= 0.0 {
        return v.round();
    }
    (v * ppp).round().max(1.0) / ppp
}

/// Snap a point to the pixel grid.
#[inline]
pub fn snap_pos(p: Pos2) -> Pos2 {
    Pos2::new(snap(p.x), snap(p.y))
}

/// Snap a rect to the pixel grid.
#[inline]
pub fn snap_rect(r: Rect) -> Rect {
    Rect::from_min_max(snap_pos(r.min), snap_pos(r.max))
}

/// How a surface should look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bevel {
    /// Raised plastic — light top/left, dark bottom/right. Buttons at rest.
    Raised,
    /// Sunken — dark top/left, light bottom/right. Inputs, LCD screens.
    Sunken,
    /// Flat with a 1px outline. Panels, list backgrounds.
    Flat,
    /// No border at all.
    None,
}

/// Draw a filled rectangle with a hard 1px outline.
pub fn fill(painter: &Painter, rect: Rect, color: Color32) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    painter.rect_filled(snap_rect(rect), 0.0, color);
}

/// Draw a 1px outline.
pub fn outline(painter: &Painter, rect: Rect, color: Color32) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    painter.rect_stroke(snap_rect(rect), 0.0, Stroke::new(BORDER, color));
}

/// Draw a 1px horizontal line.
pub fn hline(painter: &Painter, x0: f32, x1: f32, y: f32, color: Color32) {
    painter.line_segment(
        [Pos2::new(snap(x0), snap(y)), Pos2::new(snap(x1), snap(y))],
        Stroke::new(BORDER, color),
    );
}

/// Draw a 1px vertical line.
pub fn vline(painter: &Painter, x: f32, y0: f32, y1: f32, color: Color32) {
    painter.line_segment(
        [Pos2::new(snap(x), snap(y0)), Pos2::new(snap(x), snap(y1))],
        Stroke::new(BORDER, color),
    );
}

/// Draw a bevelled surface.
///
/// The bevel is drawn *inside* `rect` so adjacent panels never overlap by a
/// pixel — overlapping edges is what produces seams at high DPI.
pub fn surface(
    painter: &Painter,
    rect: Rect,
    fill_color: Color32,
    bevel: Bevel,
    palette: &Palette,
) {
    let rect = snap_rect(rect);
    if rect.width() < 2.0 || rect.height() < 2.0 {
        return;
    }

    fill(painter, rect, fill_color);

    let light = palette.panel_light;
    let dark = palette.panel_dark;

    match bevel {
        Bevel::Raised => {
            // Top and left catch the light.
            hline(painter, rect.left(), rect.right() - 1.0, rect.top(), light);
            vline(painter, rect.left(), rect.top(), rect.bottom() - 1.0, light);
            // Bottom and right fall into shadow.
            hline(
                painter,
                rect.left() + 1.0,
                rect.right(),
                rect.bottom() - 1.0,
                dark,
            );
            vline(
                painter,
                rect.right() - 1.0,
                rect.top() + 1.0,
                rect.bottom(),
                dark,
            );
        }
        Bevel::Sunken => {
            hline(painter, rect.left(), rect.right() - 1.0, rect.top(), dark);
            vline(painter, rect.left(), rect.top(), rect.bottom() - 1.0, dark);
            hline(
                painter,
                rect.left() + 1.0,
                rect.right(),
                rect.bottom() - 1.0,
                light,
            );
            vline(
                painter,
                rect.right() - 1.0,
                rect.top() + 1.0,
                rect.bottom(),
                light,
            );
        }
        Bevel::Flat => {
            outline(painter, rect, palette.border);
        }
        Bevel::None => {}
    }
}

/// Draw a chunky panel with a title bar.
///
/// Returns the content rect below the title bar. The title is centered with
/// the double-bar ornaments that hardware players used to separate the label
/// from the rest of the chrome.
pub fn titled_panel(
    painter: &Painter,
    rect: Rect,
    title: &str,
    palette: &Palette,
    font: FontId,
) -> Rect {
    let rect = snap_rect(rect);
    let title_h = 16.0;

    // Body first, then the title bar over it.
    surface(painter, rect, palette.panel, Bevel::Flat, palette);

    let bar = Rect::from_min_size(rect.min, Vec2::new(rect.width(), title_h));
    fill(painter, bar, palette.panel_dark);
    hline(
        painter,
        bar.left(),
        bar.right(),
        bar.bottom() - 1.0,
        palette.border,
    );

    let center = Pos2::new(snap(bar.center().x), snap(bar.center().y));

    // Measure the title so the ornaments sit just outside it.
    let galley = painter.layout_no_wrap(title.to_string(), font.clone(), palette.fg);
    let half_w = galley.size().x / 2.0;

    painter.galley(
        Pos2::new(
            snap(center.x - half_w),
            snap(center.y - galley.size().y / 2.0),
        ),
        galley,
        palette.fg,
    );

    // Two vertical ticks on each side — the classic separator.
    let ornament = palette.primary_dim;
    for i in 0..2 {
        let offset = 8.0 + i as f32 * 4.0;
        let (x_left, x_right) = (center.x - half_w - offset, center.x + half_w + offset);
        vline(painter, x_left, center.y - 3.0, center.y + 3.0, ornament);
        vline(painter, x_right, center.y - 3.0, center.y + 3.0, ornament);
    }

    Rect::from_min_max(Pos2::new(rect.left(), bar.bottom()), rect.max)
}

/// Draw a rectangular LED indicator.
pub fn led(painter: &Painter, rect: Rect, on: bool, palette: &Palette, color: Color32) {
    let rect = snap_rect(rect);
    let (fill_color, edge) = if on {
        (color, palette.lighten(color, 0.4))
    } else {
        (palette.dim(color, 0.85), palette.dim(color, 0.7))
    };

    fill(painter, rect, fill_color);
    // A lit LED gets a bright top edge so it reads as emissive.
    if on {
        hline(painter, rect.left(), rect.right(), rect.top(), edge);
    }
}

/// Draw a segmented level meter — the blocky bar used for volume and spectrum.
///
/// `segments` blocks are drawn across `rect`; the first `lit` of them are
/// filled. Blocks light from `low_color` through `high_color` as they climb,
/// which is what makes a meter readable at a glance.
pub fn meter(
    painter: &Painter,
    rect: Rect,
    segments: usize,
    lit: usize,
    palette: &Palette,
    low_color: Color32,
    high_color: Color32,
) {
    if segments == 0 {
        return;
    }
    let rect = snap_rect(rect);
    let seg_w = rect.width() / segments as f32;

    for i in 0..segments {
        let x = rect.left() + i as f32 * seg_w;
        // Leave a 1px gap so the blocks stay distinct.
        let seg = Rect::from_min_size(
            Pos2::new(snap(x), rect.top()),
            Vec2::new(snap(seg_w) - 1.0, rect.height()),
        );

        if i < lit {
            // Top of the meter goes to the accent color.
            let t = i as f32 / segments.max(1) as f32;
            let color = blend(low_color, high_color, t);
            fill(painter, seg, color);
        } else {
            fill(painter, seg, palette.dim(low_color, 0.85));
        }
    }
}

/// Linear blend between two colors.
pub fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let lerp = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(lerp(a.r(), b.r()), lerp(a.g(), b.g()), lerp(a.b(), b.b()))
}

/// Draw text with the painter, snapped to the pixel grid.
///
/// Snapping matters more than it sounds: a glyph drawn at y=10.4 lands on a
/// half-pixel and the bitmap font turns to mush.
pub fn text(
    painter: &Painter,
    pos: Pos2,
    align: Align2,
    content: &str,
    font: FontId,
    color: Color32,
) {
    painter.text(snap_pos(pos), align, content, font, color);
}

/// Draw text clipped to `rect`, ellipsized with ".." when it doesn't fit.
///
/// Uses `..` rather than the typographic `…` because the bitmap font has no
/// ellipsis glyph.
pub fn text_clipped(
    painter: &Painter,
    rect: Rect,
    pos: Pos2,
    align: Align2,
    content: &str,
    font: FontId,
    color: Color32,
) {
    let clipped = painter.with_clip_rect(snap_rect(rect));
    let fitted = ellipsize(painter, content, &font, rect.width());
    clipped.text(snap_pos(pos), align, &fitted, font, color);
}

/// Truncate `content` until it fits in `max_width`, appending "..".
pub fn ellipsize(painter: &Painter, content: &str, font: &FontId, max_width: f32) -> String {
    if max_width <= 0.0 {
        return String::new();
    }

    let width = |s: &str| {
        painter
            .layout_no_wrap(s.to_string(), font.clone(), Color32::WHITE)
            .size()
            .x
    };

    if width(content) <= max_width {
        return content.to_string();
    }

    // Walk back one character at a time — strings here are short, and this
    // avoids slicing mid-codepoint the way byte indexing would.
    let chars: Vec<char> = content.chars().collect();
    for take in (1..chars.len()).rev() {
        let candidate: String = chars[..take].iter().collect::<String>() + "..";
        if width(&candidate) <= max_width {
            return candidate;
        }
    }
    "..".to_string()
}

/// Scroll long text horizontally, bouncing between the two ends.
///
/// `elapsed` is seconds since the text became the active one; the returned
/// string is what should be drawn. Short text is returned unchanged.
pub fn marquee(content: &str, max_width: f32, char_width: f32, elapsed: f32) -> String {
    let text_width = content.chars().count() as f32 * char_width;
    if text_width <= max_width || char_width <= 0.0 {
        return content.to_string();
    }

    let overflow = text_width - max_width;
    // Pixels per second — slow enough to read, fast enough not to wait.
    const SPEED: f32 = 24.0;
    const PAUSE: f32 = 1.5;

    let travel = overflow / SPEED;
    let cycle = travel * 2.0 + PAUSE * 2.0;
    let t = elapsed % cycle;

    let offset = if t < PAUSE {
        0.0
    } else if t < PAUSE + travel {
        (t - PAUSE) * SPEED
    } else if t < PAUSE * 2.0 + travel {
        overflow
    } else {
        overflow - (t - PAUSE * 2.0 - travel) * SPEED
    };

    let skip = (offset / char_width).floor() as usize;
    content.chars().skip(skip).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::pixel::test_support::{painter, palette};
    use std::sync::{Mutex, MutexGuard, OnceLock};

    /// The display scale is process-global; tests that change it must not run
    /// concurrently with each other.
    fn dpi_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn with_scale<R>(ppp: f32, f: impl FnOnce() -> R) -> R {
        let _guard = dpi_lock();
        super::super::font::set_pixels_per_point(ppp);
        let result = f();
        super::super::font::set_pixels_per_point(1.0);
        result
    }

    #[test]
    fn snap_rounds_to_whole_pixels() {
        with_scale(1.0, || {
            assert_eq!(snap(10.0), 10.0);
            assert_eq!(snap(10.4), 10.0);
            assert_eq!(snap(10.6), 11.0);
            assert_eq!(snap(-0.6), -1.0);
        });
    }

    #[test]
    fn snap_lands_on_physical_pixels_at_fractional_scale() {
        // The whole point of snapping in physical space: at 1.25x a logical
        // 10.0 is fine but 10.4 would sit on a partial physical pixel.
        with_scale(1.25, || {
            for value in [10.0, 10.4, 10.6, 13.9, 27.3] {
                let physical = snap(value) * 1.25;
                assert!(
                    (physical - physical.round()).abs() < 1e-4,
                    "snap({value}) landed on {physical} physical px"
                );
            }
        });
    }

    #[test]
    fn snap_len_covers_at_least_one_physical_pixel() {
        with_scale(2.0, || {
            assert!(snap_len(0.1) * 2.0 >= 1.0, "a hairline must stay visible");
        });
    }

    #[test]
    fn snap_rect_keeps_positive_area() {
        with_scale(1.0, || {
            let r = snap_rect(Rect::from_min_size(
                Pos2::new(10.3, 20.7),
                Vec2::new(50.4, 30.2),
            ));
            assert_eq!(r.min, Pos2::new(10.0, 21.0));
            assert!(r.width() > 0.0 && r.height() > 0.0);
        });
    }

    #[test]
    fn ellipsize_leaves_short_text_alone() {
        let p = painter();
        let font = super::super::font::body(11.0);
        assert_eq!(ellipsize(&p, "Short", &font, 500.0), "Short");
    }

    #[test]
    fn ellipsize_truncates_long_text() {
        let p = painter();
        let font = super::super::font::body(11.0);
        let long = "This is a very long track title that will not fit";
        let result = ellipsize(&p, long, &font, 60.0);
        assert!(result.ends_with(".."), "got {result:?}");
        assert!(result.len() < long.len());
    }

    #[test]
    fn ellipsize_handles_zero_width() {
        let p = painter();
        let font = super::super::font::body(11.0);
        assert_eq!(ellipsize(&p, "Anything", &font, 0.0), "");
        assert_eq!(ellipsize(&p, "Anything", &font, -10.0), "");
    }

    #[test]
    fn ellipsize_is_safe_with_multibyte_text() {
        let p = painter();
        let font = super::super::font::body(11.0);
        // Must not panic on a char boundary.
        let result = ellipsize(&p, "日本語のとても長いタイトルです", &font, 40.0);
        assert!(!result.is_empty());
    }

    #[test]
    fn marquee_leaves_short_text_alone() {
        assert_eq!(marquee("Short", 500.0, 6.0, 99.0), "Short");
    }

    #[test]
    fn marquee_scrolls_long_text_over_time() {
        let text = "A very long title that definitely does not fit";
        let at_start = marquee(text, 60.0, 6.0, 2.0);
        let later = marquee(text, 60.0, 6.0, 4.0);
        assert_ne!(at_start, later, "text should scroll");
        assert!(at_start.chars().count() <= text.chars().count());
    }

    #[test]
    fn marquee_never_exceeds_the_source_length() {
        let text = "Another long title for the marquee test";
        for i in 0..100 {
            let result = marquee(text, 50.0, 6.0, i as f32 * 0.3);
            assert!(result.chars().count() <= text.chars().count());
        }
    }

    #[test]
    fn marquee_with_zero_char_width_is_a_noop() {
        assert_eq!(marquee("Anything", 10.0, 0.0, 5.0), "Anything");
    }

    #[test]
    fn blend_endpoints() {
        let a = Color32::from_rgb(0, 0, 0);
        let b = Color32::from_rgb(255, 255, 255);
        assert_eq!(blend(a, b, 0.0), a);
        assert_eq!(blend(a, b, 1.0), b);
        assert_eq!(blend(a, b, 2.0), b);
    }

    #[test]
    fn drawing_helpers_do_not_panic_on_degenerate_rects() {
        let p = painter();
        let pal = palette();
        let zero = Rect::from_min_size(Pos2::ZERO, Vec2::ZERO);

        fill(&p, zero, pal.bg);
        outline(&p, zero, pal.border);
        surface(&p, zero, pal.panel, Bevel::Raised, &pal);
        surface(&p, zero, pal.panel, Bevel::Sunken, &pal);
        led(&p, zero, true, &pal, pal.primary);
        meter(&p, zero, 10, 5, &pal, pal.primary, pal.accent);
        text(
            &p,
            Pos2::ZERO,
            Align2::LEFT_TOP,
            "x",
            super::super::font::body(11.0),
            pal.fg,
        );
    }

    #[test]
    fn every_bevel_variant_draws() {
        let p = painter();
        let pal = palette();
        let rect = Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::new(100.0, 40.0));
        for bevel in [Bevel::Raised, Bevel::Sunken, Bevel::Flat, Bevel::None] {
            surface(&p, rect, pal.panel, bevel, &pal);
        }
    }

    #[test]
    fn titled_panel_returns_the_content_area() {
        let p = painter();
        let pal = palette();
        let rect = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(200.0, 100.0));
        let content = titled_panel(&p, rect, "PLAYER", &pal, super::super::font::body(11.0));
        assert_eq!(content.left(), rect.left());
        assert_eq!(content.top(), 16.0);
        assert_eq!(content.bottom(), rect.bottom());
    }

    #[test]
    fn meter_with_zero_segments_is_a_noop() {
        let p = painter();
        let pal = palette();
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(100.0, 10.0));
        meter(&p, rect, 0, 0, &pal, pal.primary, pal.accent);
    }

    #[test]
    fn meter_clamps_lit_count() {
        let p = painter();
        let pal = palette();
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(100.0, 10.0));
        // More lit segments than exist must not panic.
        meter(&p, rect, 5, 99, &pal, pal.primary, pal.accent);
    }
}
