//! Theme colors, resolved for rendering.
//!
//! `sonus_core::Theme` stores hex strings so it stays free of any UI
//! dependency. This module is where those become `egui::Color32`, plus the
//! handful of derived shades the pixel widgets need (bevels, glows, row
//! striping) that aren't worth storing in the theme file.

use egui::{Color32, Rgba, Stroke};
use sonus_core::Theme;

/// A theme resolved into render-ready colors.
#[derive(Debug, Clone, PartialEq)]
pub struct Palette {
    pub bg: Color32,
    pub panel: Color32,
    pub panel_light: Color32,
    pub panel_dark: Color32,

    pub fg: Color32,
    pub fg_dim: Color32,

    pub primary: Color32,
    pub primary_dim: Color32,
    pub accent: Color32,
    pub danger: Color32,

    pub lcd_bg: Color32,
    pub lcd_fg: Color32,
    pub lcd_dim: Color32,

    pub border: Color32,
    pub highlight: Color32,
    pub highlight_fg: Color32,

    /// Subtle tint for alternating list rows.
    pub row_alt: Color32,
    /// Background of a hovered row.
    pub row_hover: Color32,
    /// Disabled text.
    pub disabled: Color32,
}

impl Palette {
    pub fn from_theme(theme: &Theme) -> Self {
        let bg = hex(&theme.bg);
        let panel = hex(&theme.panel);
        let border = hex(&theme.border);

        Self {
            bg,
            panel,
            panel_light: hex(&theme.panel_light),
            panel_dark: hex(&theme.panel_dark),

            fg: hex(&theme.fg),
            fg_dim: hex(&theme.fg_dim),

            primary: hex(&theme.primary),
            primary_dim: hex(&theme.primary_dim),
            accent: hex(&theme.accent),
            danger: hex(&theme.danger),

            lcd_bg: hex(&theme.lcd_bg),
            lcd_fg: hex(&theme.lcd_fg),
            lcd_dim: hex(&theme.lcd_dim),

            border,
            highlight: hex(&theme.highlight),
            highlight_fg: hex(&theme.highlight_fg),

            // Zebra striping: a whisper of the panel color over the background.
            row_alt: mix(bg, panel, 0.35),
            row_hover: mix(bg, hex(&theme.fg), 0.10),
            disabled: mix(hex(&theme.fg_dim), bg, 0.45),
        }
    }

    /// Apply the palette to egui's own widget styling.
    ///
    /// Our hand-drawn widgets ignore this, but scrollbars, text edits and
    /// tooltips come from egui and would otherwise use the default dark theme.
    pub fn apply(&self, ctx: &egui::Context) {
        let mut visuals = egui::Visuals::dark();

        visuals.panel_fill = self.bg;
        visuals.window_fill = self.panel;
        visuals.extreme_bg_color = self.lcd_bg;
        visuals.faint_bg_color = self.row_alt;
        visuals.window_stroke = Stroke::new(1.0_f32, self.border);
        visuals.window_shadow = egui::epaint::Shadow::NONE;
        visuals.popup_shadow = egui::epaint::Shadow::NONE;

        visuals.widgets.noninteractive.bg_fill = self.panel;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, self.border);
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, self.fg_dim);

        visuals.widgets.inactive.bg_fill = self.panel;
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, self.fg);

        visuals.widgets.hovered.bg_fill = self.highlight;
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, self.primary);

        visuals.widgets.active.bg_fill = self.highlight;
        visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, self.primary);

        visuals.selection.bg_fill = self.primary_dim;
        visuals.selection.stroke = Stroke::new(1.0_f32, self.primary);
        visuals.hyperlink_color = self.primary;

        // Hard corners everywhere — rounded rects are the fastest way to break
        // the pixel-art illusion.
        let square = egui::Rounding::ZERO;
        visuals.window_rounding = square;
        visuals.menu_rounding = square;
        for widget in [
            &mut visuals.widgets.noninteractive,
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            widget.rounding = square;
        }

        ctx.set_visuals(visuals);

        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(4.0, 4.0);
        style.spacing.scroll.bar_width = 8.0;
        style.spacing.scroll.floating = false;
        ctx.set_style(style);
    }

    /// A dimmed version of `color`, for unlit indicators.
    pub fn dim(&self, color: Color32, amount: f32) -> Color32 {
        mix(color, self.bg, amount)
    }

    /// Brighten a color — used for the lit face of a button.
    pub fn lighten(&self, color: Color32, amount: f32) -> Color32 {
        mix(color, Color32::WHITE, amount)
    }

    /// Darken a color — used for pressed states.
    pub fn darken(&self, color: Color32, amount: f32) -> Color32 {
        mix(color, Color32::BLACK, amount)
    }
}

fn hex(s: &str) -> Color32 {
    let (r, g, b) = Theme::parse_hex(s);
    Color32::from_rgb(r, g, b)
}

/// Linear blend, `t = 0.0` → `a`, `t = 1.0` → `b`.
fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let [ar, ag, ab, aa] = a.to_array();
    let [br, bg, bb, ba] = b.to_array();
    let lerp = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(lerp(ar, br), lerp(ag, bg), lerp(ab, bb), lerp(aa, ba))
}

/// Fade a color toward transparent — for glows and shadows.
pub fn with_alpha(color: Color32, alpha: f32) -> Color32 {
    let [r, g, b, _] = color.to_array();
    Color32::from_rgba_unmultiplied(r, g, b, (alpha.clamp(0.0, 1.0) * 255.0) as u8)
}

/// Blend toward white, matching `Theme::shade`'s behavior.
pub fn shade_toward(color: Color32, target: Color32, amount: f32) -> Color32 {
    mix(color, target, amount)
}

/// egui lerp helper for the odd place we need `Rgba`.
pub fn rgba_mix(a: Color32, b: Color32, t: f32) -> Color32 {
    Color32::from(egui::lerp(Rgba::from(a)..=Rgba::from(b), t))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sonus_core::builtin_themes;

    fn palette() -> Palette {
        Palette::from_theme(&sonus_core::default_theme())
    }

    #[test]
    fn builds_from_the_default_theme() {
        let p = palette();
        assert_ne!(p.bg, Color32::TRANSPARENT);
        assert_ne!(p.primary, p.bg);
    }

    #[test]
    fn every_builtin_theme_produces_a_usable_palette() {
        for theme in builtin_themes() {
            let p = Palette::from_theme(&theme);
            // Fully opaque roles — a transparent panel would show through.
            for (label, color) in [
                ("bg", p.bg),
                ("panel", p.panel),
                ("fg", p.fg),
                ("primary", p.primary),
                ("border", p.border),
                ("lcd_bg", p.lcd_bg),
                ("lcd_fg", p.lcd_fg),
            ] {
                assert_eq!(
                    color.a(),
                    255,
                    "theme '{}' role '{label}' is not opaque",
                    theme.name
                );
            }
        }
    }

    #[test]
    fn foreground_contrasts_with_background() {
        for theme in builtin_themes() {
            let p = Palette::from_theme(&theme);
            assert!(
                contrast(p.bg, p.fg) > 3.0,
                "theme '{}' has unreadable fg on bg",
                theme.name
            );
            assert!(
                contrast(p.lcd_bg, p.lcd_fg) > 3.0,
                "theme '{}' has unreadable LCD text",
                theme.name
            );
        }
    }

    #[test]
    fn mix_endpoints_are_exact() {
        let a = Color32::from_rgb(0, 0, 0);
        let b = Color32::from_rgb(255, 255, 255);
        assert_eq!(mix(a, b, 0.0), a);
        assert_eq!(mix(a, b, 1.0), b);
    }

    #[test]
    fn mix_clamps_out_of_range_t() {
        let a = Color32::from_rgb(0, 0, 0);
        let b = Color32::from_rgb(255, 255, 255);
        assert_eq!(mix(a, b, -1.0), a);
        assert_eq!(mix(a, b, 2.0), b);
    }

    #[test]
    fn lighten_and_darken_move_toward_their_targets() {
        let p = palette();
        let c = Color32::from_rgb(128, 128, 128);
        assert!(p.lighten(c, 0.5).r() > c.r());
        assert!(p.darken(c, 0.5).r() < c.r());
    }

    #[test]
    fn with_alpha_sets_the_alpha_channel() {
        let c = Color32::from_rgb(255, 0, 0);
        assert_eq!(with_alpha(c, 0.0).a(), 0);
        assert_eq!(with_alpha(c, 1.0).a(), 255);
        assert_eq!(with_alpha(c, 2.0).a(), 255);
    }

    #[test]
    fn row_alternation_is_subtle_but_distinct() {
        let p = palette();
        assert_ne!(p.row_alt, p.bg, "alternating rows must be visible");
        // But not so different that it looks like a different panel.
        let diff = (p.row_alt.r() as i32 - p.bg.r() as i32).abs()
            + (p.row_alt.g() as i32 - p.bg.g() as i32).abs()
            + (p.row_alt.b() as i32 - p.bg.b() as i32).abs();
        assert!(diff < 60, "row striping is too strong: {diff}");
    }

    #[test]
    fn apply_sets_square_corners() {
        let ctx = egui::Context::default();
        palette().apply(&ctx);
        let visuals = ctx.style().visuals.clone();
        assert_eq!(visuals.window_rounding, egui::Rounding::ZERO);
        assert_eq!(visuals.widgets.inactive.rounding, egui::Rounding::ZERO);
        assert_eq!(visuals.panel_fill, palette().bg);
    }

    /// WCAG relative luminance contrast ratio.
    fn contrast(a: Color32, b: Color32) -> f32 {
        fn luminance(c: Color32) -> f32 {
            let channel = |v: u8| {
                let v = v as f32 / 255.0;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
        }
        let (l1, l2) = (luminance(a), luminance(b));
        let (hi, lo) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
        (hi + 0.05) / (lo + 0.05)
    }
}
