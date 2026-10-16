//! Pixel fonts and text styles.
//!
//! Two bitmap fonts are compiled into the binary so the look is identical on
//! every platform and works offline:
//!
//! - **Press Start 2P** for display text — the LCD readout, headers, the clock.
//! - **Silkscreen** for body text — labels, list rows, buttons.
//!
//! Both are extended with egui's bundled fonts as fallbacks, so a glyph
//! neither bitmap font covers (♪, arrows, CJK) still renders instead of
//! showing a tofu box.

use std::sync::atomic::{AtomicU32, Ordering};

use egui::{FontData, FontDefinitions, FontFamily, FontId};

const PRESS_START: &[u8] = include_bytes!("../../assets/fonts/PressStart2P.ttf");
const SILKSCREEN: &[u8] = include_bytes!("../../assets/fonts/Silkscreen.ttf");

/// Font family for chunky display text.
pub const PIXEL: &str = "pixel";
/// Font family for readable body text.
pub const UI: &str = "ui";

/// The display scale, stored as thousandths so it fits in an atomic.
///
/// Bitmap fonts only look right when rasterized at whole physical pixels. On a
/// 1.25× display an 11pt glyph lands on 13.75px and the rasterizer smears it
/// across a partial pixel — which is exactly what turns crisp pixel art into
/// mush. We keep the scale here so [`display`] and [`body`] can round every
/// requested size to one that lands on an integer.
static PIXELS_PER_POINT_MILLI: AtomicU32 = AtomicU32::new(1000);

/// Record the current display scale. Call once per frame — it's a single
/// relaxed store, and sizes are only recomputed when the value changes.
pub fn set_pixels_per_point(ppp: f32) {
    let milli = (ppp.clamp(0.5, 8.0) * 1000.0).round() as u32;
    PIXELS_PER_POINT_MILLI.store(milli, Ordering::Relaxed);
}

/// The display scale last recorded by [`set_pixels_per_point`].
pub fn pixels_per_point() -> f32 {
    PIXELS_PER_POINT_MILLI.load(Ordering::Relaxed) as f32 / 1000.0
}

/// Round a font size so its *physical* size is a whole number of pixels.
///
/// This is the key to sharp bitmap text on fractional-DPI displays. At 1.25×
/// a requested 11.0 becomes 11.2 — which rasterizes as exactly 14 physical
/// pixels instead of 13.75.
pub fn snap_size(size: f32) -> f32 {
    let ppp = pixels_per_point();
    if ppp <= 0.0 {
        return size;
    }
    ((size * ppp).round() / ppp).max(1.0 / ppp)
}

/// Install the pixel fonts. Call once, before the first frame.
pub fn install(ctx: &egui::Context) {
    set_pixels_per_point(ctx.pixels_per_point());

    let mut fonts = FontDefinitions::default();

    fonts
        .font_data
        .insert("press_start".into(), FontData::from_static(PRESS_START));
    fonts
        .font_data
        .insert("silkscreen".into(), FontData::from_static(SILKSCREEN));

    let pixel = FontFamily::Name(PIXEL.into());
    let ui = FontFamily::Name(UI.into());

    // Display family: bitmap first, egui's fonts behind it for coverage.
    let mut pixel_stack = vec!["press_start".to_string()];
    pixel_stack.extend(default_stack(&fonts, &FontFamily::Proportional));
    fonts.families.insert(pixel, pixel_stack);

    // Body family: Silkscreen, then the defaults.
    let mut ui_stack = vec!["silkscreen".to_string()];
    ui_stack.extend(default_stack(&fonts, &FontFamily::Proportional));
    fonts.families.insert(ui, ui_stack);

    // Make Silkscreen the default proportional face too, so any widget we
    // didn't style by hand still fits the aesthetic.
    if let Some(proportional) = fonts.families.get_mut(&FontFamily::Proportional) {
        proportional.insert(0, "silkscreen".into());
    }
    if let Some(monospace) = fonts.families.get_mut(&FontFamily::Monospace) {
        monospace.insert(0, "silkscreen".into());
    }

    ctx.set_fonts(fonts);
}

fn default_stack(fonts: &FontDefinitions, family: &FontFamily) -> Vec<String> {
    fonts.families.get(family).cloned().unwrap_or_default()
}

/// Pixel-perfect rendering: no anti-aliasing on shape edges.
///
/// egui feathers every filled shape to smooth it, which is exactly wrong for
/// pixel art — it turns hard 1px bevels into grey mush. Disabling it is what
/// makes the whole UI read as 8-bit rather than "retro-ish".
pub fn install_crisp_tessellation(ctx: &egui::Context) {
    ctx.tessellation_options_mut(|options| {
        options.feathering = false;
        // Text is rendered from the font atlas, so this only affects the
        // shapes we draw by hand.
        options.round_text_to_pixels = true;
    });
}

// ---------------------------------------------------------------------------
// Text styles
// ---------------------------------------------------------------------------

/// Chunky display font — the LCD readout and section headers.
pub fn display(size: f32) -> FontId {
    FontId::new(snap_size(size), FontFamily::Name(PIXEL.into()))
}

/// Body font — labels, list rows, buttons.
pub fn body(size: f32) -> FontId {
    FontId::new(snap_size(size), FontFamily::Name(UI.into()))
}

/// Standard sizes. Kept as named constants so the UI stays on a scale rather
/// than accumulating one-off numbers.
pub mod size {
    /// Big LCD clock / time readout.
    pub const TIME: f32 = 15.0;
    /// Panel section headers.
    pub const HEADER: f32 = 8.0;
    /// The now-playing title on the LCD.
    pub const TITLE: f32 = 8.0;
    /// List rows and buttons.
    pub const BODY: f32 = 11.0;
    /// Secondary text, hints, durations.
    pub const SMALL: f32 = 9.0;
    /// The smallest labels — column headers, status bar.
    pub const TINY: f32 = 8.0;
}

/// Natural size of Press Start 2P. Multiples of this stay pixel-aligned.
pub const PIXEL_GRID: f32 = 8.0;

/// Round a size to a multiple of the pixel grid.
///
/// Bitmap fonts look right at integer multiples of their design size and
/// blurry between them — this keeps every display size on the grid.
pub fn snap_to_grid(size: f32) -> f32 {
    (size / PIXEL_GRID).round().max(1.0) * PIXEL_GRID
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    /// The display scale is process-global, so tests that change it must not
    /// run concurrently. Every test that touches it takes this lock.
    fn dpi_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Run `f` with a specific display scale, restoring 1.0 afterwards.
    fn with_scale<R>(ppp: f32, f: impl FnOnce() -> R) -> R {
        let _guard = dpi_lock();
        set_pixels_per_point(ppp);
        let result = f();
        set_pixels_per_point(1.0);
        result
    }

    #[test]
    fn snap_to_grid_rounds_to_multiples() {
        assert_eq!(snap_to_grid(8.0), 8.0);
        assert_eq!(snap_to_grid(16.0), 16.0);
        assert_eq!(snap_to_grid(11.0), 8.0);
        assert_eq!(snap_to_grid(13.0), 16.0);
    }

    #[test]
    fn snap_to_grid_never_returns_zero() {
        assert_eq!(snap_to_grid(0.0), 8.0);
        assert_eq!(snap_to_grid(-5.0), 8.0);
        assert_eq!(snap_to_grid(1.0), 8.0);
    }

    #[test]
    fn fonts_are_embedded() {
        assert!(PRESS_START.len() > 1000, "Press Start 2P should be bundled");
        assert!(SILKSCREEN.len() > 1000, "Silkscreen should be bundled");
    }

    #[test]
    fn snap_size_lands_on_whole_physical_pixels() {
        // The case that motivated this: 1.25x display, 11pt request.
        with_scale(1.25, || {
            let physical = snap_size(11.0) * 1.25;
            assert!(
                (physical - physical.round()).abs() < 1e-4,
                "11pt at 1.25x gave {physical} physical px"
            );
        });
    }

    #[test]
    fn snap_size_is_exact_at_integer_scales() {
        with_scale(1.0, || assert_eq!(snap_size(11.0), 11.0));
        with_scale(2.0, || assert_eq!(snap_size(11.0), 11.0));
    }

    #[test]
    fn snap_size_handles_every_common_scale() {
        for ppp in [1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0] {
            with_scale(ppp, || {
                for size in [8.0, 9.0, 11.0, 15.0, 16.0] {
                    let physical = snap_size(size) * ppp;
                    assert!(
                        (physical - physical.round()).abs() < 1e-3,
                        "{size}pt at {ppp}x gave {physical} physical px"
                    );
                }
            });
        }
    }

    #[test]
    fn snap_size_never_returns_zero() {
        with_scale(1.25, || {
            assert!(snap_size(0.0) > 0.0);
            assert!(snap_size(-5.0) > 0.0);
        });
    }

    #[test]
    fn pixels_per_point_round_trips() {
        with_scale(1.25, || {
            assert!((pixels_per_point() - 1.25).abs() < 1e-3);
        });
        with_scale(2.0, || {
            assert!((pixels_per_point() - 2.0).abs() < 1e-3);
        });
    }

    #[test]
    fn pixels_per_point_clamps_absurd_values() {
        // A zero scale would divide by zero in every snap call.
        with_scale(0.0, || assert!(pixels_per_point() >= 0.5));
        with_scale(1000.0, || assert!(pixels_per_point() <= 8.0));
    }

    #[test]
    fn install_does_not_panic() {
        let ctx = egui::Context::default();
        install(&ctx);
        install_crisp_tessellation(&ctx);
        // Font families only become queryable after a frame has run.
        let _ = ctx.run(Default::default(), |ctx| {
            ctx.fonts(|f| {
                assert!(f.families().contains(&FontFamily::Name(PIXEL.into())));
                assert!(f.families().contains(&FontFamily::Name(UI.into())));
            });
        });
    }

    #[test]
    fn crisp_tessellation_disables_feathering() {
        let ctx = egui::Context::default();
        install_crisp_tessellation(&ctx);
        ctx.tessellation_options(|options| {
            assert!(!options.feathering, "anti-aliasing breaks the pixel look");
            assert!(options.round_text_to_pixels);
        });
    }

    #[test]
    fn text_styles_use_the_right_families() {
        assert_eq!(display(8.0).family, FontFamily::Name(PIXEL.into()));
        assert_eq!(body(11.0).family, FontFamily::Name(UI.into()));
    }
}
