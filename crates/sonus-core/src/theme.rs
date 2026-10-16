use serde::{Deserialize, Serialize};

/// A color theme.
///
/// Colors are hex strings — the GUI layer converts them to its own color type,
/// so `sonus-core` stays free of any rendering dependency.
///
/// The palette is split into *roles* rather than raw shades. That's what lets a
/// theme be swapped without touching a single line of drawing code: the pixel
/// widgets ask for "panel face" and "bevel shadow", never for a specific hex.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
    pub name: String,

    // ---- surfaces -------------------------------------------------------
    /// Window background — shows through the gaps between panels.
    pub bg: String,
    /// Panel face — the "raised" plastic look.
    pub panel: String,
    /// Bevel highlight, drawn on the top/left edge of a raised panel.
    pub panel_light: String,
    /// Bevel shadow, drawn on the bottom/right edge of a raised panel.
    pub panel_dark: String,

    // ---- text -----------------------------------------------------------
    /// Primary text.
    pub fg: String,
    /// Dimmed text — labels, inactive rows, hints.
    pub fg_dim: String,

    // ---- accents --------------------------------------------------------
    /// Main accent: active track, slider fill, focused widget.
    pub primary: String,
    /// Dimmed accent — unlit LEDs, inactive bars.
    pub primary_dim: String,
    /// Secondary accent — peaks, badges, non-critical emphasis.
    pub accent: String,
    /// Errors and destructive actions.
    pub danger: String,

    // ---- display --------------------------------------------------------
    /// Backdrop of the LCD / VFD style readouts.
    pub lcd_bg: String,
    /// Bright element on the display (lit segments, big time).
    pub lcd_fg: String,
    /// Unlit element on the display — the "ghost" segments.
    pub lcd_dim: String,

    // ---- chrome ---------------------------------------------------------
    /// Hard 1px border around panels.
    pub border: String,
    /// Background of a selected row.
    pub highlight: String,
    /// Text on a selected row.
    pub highlight_fg: String,
}

impl Theme {
    /// Parse `#rgb`, `#rrggbb`, or the same without the `#`.
    /// Falls back to a neutral dark grey rather than panicking — a malformed
    /// theme file should degrade, not crash the player.
    pub fn parse_hex(hex: &str) -> (u8, u8, u8) {
        let hex = hex.trim().trim_start_matches('#');
        match hex.len() {
            6 => (
                u8::from_str_radix(&hex[0..2], 16).unwrap_or(0x1a),
                u8::from_str_radix(&hex[2..4], 16).unwrap_or(0x1a),
                u8::from_str_radix(&hex[4..6], 16).unwrap_or(0x1a),
            ),
            3 => (
                u8::from_str_radix(&hex[0..1], 16).unwrap_or(1) * 17,
                u8::from_str_radix(&hex[1..2], 16).unwrap_or(1) * 17,
                u8::from_str_radix(&hex[2..3], 16).unwrap_or(1) * 17,
            ),
            _ => (0x1a, 0x1a, 0x1a),
        }
    }

    /// Blend two hex colors. `t = 0.0` gives `a`, `t = 1.0` gives `b`.
    pub fn mix(a: &str, b: &str, t: f32) -> (u8, u8, u8) {
        let (ar, ag, ab) = Self::parse_hex(a);
        let (br, bg, bb) = Self::parse_hex(b);
        let t = t.clamp(0.0, 1.0);
        let lerp = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
        (lerp(ar, br), lerp(ag, bg), lerp(ab, bb))
    }

    /// Lighten (`amount > 0`) or darken (`amount < 0`) a hex color.
    pub fn shade(hex: &str, amount: f32) -> (u8, u8, u8) {
        let target = if amount >= 0.0 { "#ffffff" } else { "#000000" };
        Self::mix(hex, target, amount.abs())
    }

    /// Format an rgb triple back into `#rrggbb`.
    pub fn to_hex((r, g, b): (u8, u8, u8)) -> String {
        format!("#{r:02x}{g:02x}{b:02x}")
    }

    /// All roles as `(label, value)` — handy for a theme preview or a CLI dump.
    pub fn roles(&self) -> [(&'static str, &str); 16] {
        [
            ("bg", &self.bg),
            ("panel", &self.panel),
            ("panel_light", &self.panel_light),
            ("panel_dark", &self.panel_dark),
            ("fg", &self.fg),
            ("fg_dim", &self.fg_dim),
            ("primary", &self.primary),
            ("primary_dim", &self.primary_dim),
            ("accent", &self.accent),
            ("danger", &self.danger),
            ("lcd_bg", &self.lcd_bg),
            ("lcd_fg", &self.lcd_fg),
            ("lcd_dim", &self.lcd_dim),
            ("border", &self.border),
            ("highlight", &self.highlight),
            ("highlight_fg", &self.highlight_fg),
        ]
    }
}

/// Build a theme from the eight core colors, deriving the rest.
///
/// Keeps the built-in table readable — only the colors that actually define a
/// theme's identity are spelled out, the bevels and display shades fall out of
/// simple rules that hold for every retro palette.
#[allow(clippy::too_many_arguments)]
fn theme(
    name: &str,
    bg: &str,
    panel: &str,
    fg: &str,
    primary: &str,
    secondary: &str,
    accent: &str,
    border: &str,
    highlight: &str,
) -> Theme {
    Theme {
        name: name.into(),
        bg: bg.into(),
        panel: panel.into(),
        // Bevels: lift the face toward white, push the shadow toward black.
        panel_light: Theme::to_hex(Theme::shade(panel, 0.16)),
        panel_dark: Theme::to_hex(Theme::shade(panel, -0.30)),
        fg: fg.into(),
        fg_dim: secondary.into(),
        primary: primary.into(),
        primary_dim: Theme::to_hex(Theme::mix(primary, bg, 0.55)),
        accent: accent.into(),
        danger: "#e5484d".into(),
        // Displays always read as a backlit screen: near-black glass with the
        // accent glowing on top of it.
        lcd_bg: Theme::to_hex(Theme::mix(bg, "#000000", 0.55)),
        lcd_fg: primary.into(),
        lcd_dim: Theme::to_hex(Theme::mix(primary, bg, 0.72)),
        border: border.into(),
        highlight: highlight.into(),
        highlight_fg: fg.into(),
    }
}

/// Every built-in theme, in cycle order. The first entry is the default.
pub fn builtin_themes() -> Vec<Theme> {
    vec![
        theme(
            "Winamp Green",
            "#1b2227",
            "#232b30",
            "#c8d3d8",
            "#2ee86b",
            "#5c6b73",
            "#ffb86c",
            "#3a454c",
            "#2c363c",
        ),
        theme(
            "Osaka Jade",
            "#0b120e",
            "#141d17",
            "#b6d4c2",
            "#4ade80",
            "#4a6b58",
            "#ffd479",
            "#26382d",
            "#1d3126",
        ),
        theme(
            "Amber CRT",
            "#171008",
            "#211709",
            "#e8c99a",
            "#ffb000",
            "#8a6a3d",
            "#ff7a45",
            "#3d2c14",
            "#2c1f0e",
        ),
        theme(
            "C64", "#2b2b6b", "#4040a0", "#c8c8ff", "#a0a0ff", "#7878c8", "#ffd479", "#6b6bb5",
            "#5252ad",
        ),
        theme(
            "Midnight", "#0d1117", "#161b22", "#c9d1d9", "#58a6ff", "#8b949e", "#79c0ff",
            "#30363d", "#1f2937",
        ),
        theme(
            "Tokyo Night",
            "#16161e",
            "#1f2335",
            "#a9b1d6",
            "#7aa2f7",
            "#565f89",
            "#bb9af7",
            "#3b4261",
            "#292e42",
        ),
        theme(
            "Solarized Dark",
            "#00252e",
            "#073642",
            "#93a1a1",
            "#2aa198",
            "#586e75",
            "#cb4b16",
            "#0d4351",
            "#0a4a58",
        ),
        theme(
            "Gruvbox", "#1d2021", "#282828", "#ebdbb2", "#b8bb26", "#928374", "#fabd2f", "#3c3836",
            "#32302f",
        ),
    ]
}

/// Look up a built-in theme by name, case-insensitively.
pub fn builtin_by_name(name: &str) -> Option<Theme> {
    builtin_themes()
        .into_iter()
        .find(|t| t.name.eq_ignore_ascii_case(name))
}

/// The theme used when nothing else is configured.
pub fn default_theme() -> Theme {
    builtin_themes()
        .into_iter()
        .next()
        .expect("builtin_themes is never empty")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_6_digits() {
        assert_eq!(Theme::parse_hex("#ff8800"), (0xff, 0x88, 0x00));
        assert_eq!(Theme::parse_hex("2ee86b"), (0x2e, 0xe8, 0x6b));
    }

    #[test]
    fn parse_hex_3_digits() {
        assert_eq!(Theme::parse_hex("#f80"), (0xff, 0x88, 0x00));
    }

    #[test]
    fn parse_hex_invalid_falls_back() {
        assert_eq!(Theme::parse_hex("not-a-color"), (0x1a, 0x1a, 0x1a));
        assert_eq!(Theme::parse_hex(""), (0x1a, 0x1a, 0x1a));
    }

    #[test]
    fn parse_hex_tolerates_whitespace() {
        assert_eq!(Theme::parse_hex("  #ffffff \n"), (0xff, 0xff, 0xff));
    }

    #[test]
    fn mix_endpoints() {
        assert_eq!(Theme::mix("#000000", "#ffffff", 0.0), (0, 0, 0));
        assert_eq!(Theme::mix("#000000", "#ffffff", 1.0), (255, 255, 255));
        assert_eq!(Theme::mix("#000000", "#ffffff", 0.5), (128, 128, 128));
    }

    #[test]
    fn shade_moves_toward_target() {
        assert_eq!(Theme::shade("#000000", 1.0), (255, 255, 255));
        assert_eq!(Theme::shade("#ffffff", -1.0), (0, 0, 0));
    }

    #[test]
    fn to_hex_round_trips() {
        assert_eq!(Theme::to_hex((0x12, 0x34, 0x56)), "#123456");
        assert_eq!(Theme::parse_hex(&Theme::to_hex((1, 2, 3))), (1, 2, 3));
    }

    #[test]
    fn winamp_green_is_default() {
        assert_eq!(default_theme().name, "Winamp Green");
        assert!(builtin_by_name("winamp green").is_some());
    }

    #[test]
    fn osaka_jade_is_the_second_theme() {
        // It was the default before Winamp Green took over; keeping it next in
        // the cycle means one press of "cycle theme" still lands on it.
        assert_eq!(builtin_themes()[1].name, "Osaka Jade");
    }

    #[test]
    fn all_builtins_have_every_role_populated() {
        for t in builtin_themes() {
            for (label, value) in t.roles() {
                assert!(
                    !value.is_empty(),
                    "theme '{}' has empty role '{label}'",
                    t.name
                );
                assert!(
                    value.starts_with('#') && value.len() == 7,
                    "theme '{}' role '{label}' is not #rrggbb: {value}",
                    t.name
                );
            }
        }
    }

    #[test]
    fn builtin_names_are_unique() {
        let themes = builtin_themes();
        for (i, a) in themes.iter().enumerate() {
            for b in themes.iter().skip(i + 1) {
                assert_ne!(a.name, b.name);
            }
        }
    }

    #[test]
    fn at_least_eight_themes() {
        assert!(builtin_themes().len() >= 8);
    }
}
