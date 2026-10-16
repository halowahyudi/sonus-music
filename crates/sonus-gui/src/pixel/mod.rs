//! The pixel-art look: fonts, palette, drawing primitives, widgets.
//!
//! Everything visual goes through this module. Keeping it in one place is what
//! lets a theme swap restyle the entire app without touching a single view —
//! views ask for roles ("panel face", "LCD text"), never for hex codes.

pub mod button;
pub mod font;
pub mod list;
pub mod paint;
pub mod palette;
pub mod seven_seg;
pub mod slider;
pub mod spectrum;

pub use button::{segmented, toggle, ButtonStyle, PixelButton};
pub use font::{body, display, size as font_size};
pub use list::{RowContent, RowState, ROW_HEIGHT};
pub use paint::{Bevel, UNIT};
pub use palette::Palette;
pub use seven_seg::{format_clock, SevenSeg};
pub use slider::{BandSlider, PixelSlider};
pub use spectrum::Spectrum;

/// Test scaffolding shared by the widget modules.
#[cfg(test)]
pub(crate) mod test_support {
    use egui::Ui;

    /// A context with the pixel fonts installed.
    ///
    /// Widget tests draw real text, and our custom families don't exist on a
    /// bare `Context` — without this every text measurement panics.
    pub fn context() -> egui::Context {
        let ctx = egui::Context::default();
        super::font::install(&ctx);
        super::font::install_crisp_tessellation(&ctx);
        ctx
    }

    /// A painter backed by a throwaway context.
    pub fn painter() -> egui::Painter {
        let ctx = context();
        let mut captured = None;
        let _ = ctx.run(Default::default(), |ctx| {
            let layer = egui::LayerId::new(egui::Order::Background, egui::Id::new("test"));
            captured = Some(ctx.layer_painter(layer));
        });
        captured.expect("a painter is available inside a frame")
    }

    /// Run a closure inside a real frame so widgets can allocate space.
    pub fn in_frame<R>(f: impl FnOnce(&mut Ui) -> R) -> R {
        let ctx = context();
        let mut f = Some(f);
        let mut result = None;
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                if let Some(f) = f.take() {
                    result = Some(f(ui));
                }
            });
        });
        result.expect("closure ran")
    }

    /// The default palette, for tests that don't care about theming.
    pub fn palette() -> super::Palette {
        super::Palette::from_theme(&sonus_core::default_theme())
    }
}
