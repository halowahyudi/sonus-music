//! Theme loading and cycling.
//!
//! Built-in themes come from `sonus_core`. Custom themes are `.toml` files
//! dropped in the user's config directory — the same eight-color format the
//! built-ins are defined with, so a custom theme can start as a copy of one.

use std::path::PathBuf;

use serde::Deserialize;
use sonus_core::{builtin_themes, Theme};

use crate::pixel::Palette;

/// Where custom themes live.
///
/// Linux: `~/.config/sonus/themes`
/// Windows: `%APPDATA%\sonus\themes`
/// macOS: `~/Library/Application Support/sonus/themes`
pub fn themes_dir() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("sonus");
    path.push("themes");
    path
}

/// Owns the theme list and which one is active.
pub struct ThemeManager {
    themes: Vec<Theme>,
    current: usize,
    palette: Palette,
    /// Problems hit while loading custom themes, surfaced in the UI.
    warnings: Vec<String>,
}

impl ThemeManager {
    /// Load built-ins plus any custom themes found on disk.
    pub fn load() -> Self {
        let mut themes = builtin_themes();
        let mut warnings = Vec::new();

        let dir = themes_dir();
        if dir.is_dir() {
            match std::fs::read_dir(&dir) {
                Ok(entries) => {
                    // Sort by filename so the theme order is stable between
                    // runs — directory order isn't guaranteed.
                    let mut paths: Vec<PathBuf> = entries
                        .flatten()
                        .map(|entry| entry.path())
                        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("toml"))
                        .collect();
                    paths.sort();

                    for path in paths {
                        match load_theme_file(&path) {
                            Ok(theme) => {
                                // A custom theme may deliberately override a
                                // built-in, so replace rather than skip.
                                match themes
                                    .iter()
                                    .position(|t| t.name.eq_ignore_ascii_case(&theme.name))
                                {
                                    Some(index) => themes[index] = theme,
                                    None => themes.push(theme),
                                }
                            }
                            Err(e) => {
                                let name = path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_else(|| path.display().to_string());
                                warnings.push(format!("{name}: {e}"));
                            }
                        }
                    }
                }
                Err(e) => warnings.push(format!("{}: {e}", dir.display())),
            }
        }

        let palette = Palette::from_theme(&themes[0]);
        Self {
            themes,
            current: 0,
            palette,
            warnings,
        }
    }

    /// Built-ins only — used by tests and the CLI.
    pub fn builtin_only() -> Self {
        let themes = builtin_themes();
        let palette = Palette::from_theme(&themes[0]);
        Self {
            themes,
            current: 0,
            palette,
            warnings: Vec::new(),
        }
    }

    pub fn current(&self) -> &Theme {
        &self.themes[self.current]
    }

    pub fn current_index(&self) -> usize {
        self.current
    }

    /// The active palette.
    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    pub fn themes(&self) -> &[Theme] {
        &self.themes
    }

    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    pub fn len(&self) -> usize {
        self.themes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.themes.is_empty()
    }

    /// Switch to a theme by index. Returns true if it changed.
    pub fn select(&mut self, index: usize) -> bool {
        if index >= self.themes.len() || index == self.current {
            return false;
        }
        self.current = index;
        self.palette = Palette::from_theme(&self.themes[index]);
        true
    }

    /// Switch by name, case-insensitively.
    pub fn select_by_name(&mut self, name: &str) -> bool {
        match self
            .themes
            .iter()
            .position(|t| t.name.eq_ignore_ascii_case(name))
        {
            Some(index) => self.select(index),
            None => false,
        }
    }

    /// Advance to the next theme, wrapping around.
    pub fn cycle(&mut self) -> &Theme {
        self.current = (self.current + 1) % self.themes.len();
        self.palette = Palette::from_theme(&self.themes[self.current]);
        &self.themes[self.current]
    }

    /// Name of the active theme.
    pub fn current_name(&self) -> &str {
        &self.themes[self.current].name
    }
}

/// The on-disk theme format.
///
/// Every field is required except `name`: a theme missing a color would render
/// with an invisible element somewhere, and failing loudly with a filename is
/// more useful than a silently broken palette.
#[derive(Deserialize)]
struct ThemeFile {
    name: Option<String>,
    bg: String,
    panel: String,
    fg: String,
    primary: String,
    secondary: String,
    accent: String,
    border: String,
    highlight: String,
    /// Optional. Derived from `panel` when absent.
    panel_light: Option<String>,
    panel_dark: Option<String>,
    /// Optional. Derived from `secondary` when absent.
    fg_dim: Option<String>,
    /// Optional. Derived from `primary` when absent.
    primary_dim: Option<String>,
    lcd_bg: Option<String>,
    lcd_fg: Option<String>,
    lcd_dim: Option<String>,
    danger: Option<String>,
    highlight_fg: Option<String>,
}

impl ThemeFile {
    fn into_theme(self, fallback_name: &str) -> Theme {
        let shade = |hex: &str, amount: f32| Theme::to_hex(Theme::shade(hex, amount));
        let mix = |a: &str, b: &str, t: f32| Theme::to_hex(Theme::mix(a, b, t));

        Theme {
            name: self.name.unwrap_or_else(|| fallback_name.to_string()),
            panel_light: self.panel_light.unwrap_or_else(|| shade(&self.panel, 0.16)),
            panel_dark: self.panel_dark.unwrap_or_else(|| shade(&self.panel, -0.30)),
            fg_dim: self.fg_dim.unwrap_or_else(|| self.secondary.clone()),
            primary_dim: self
                .primary_dim
                .unwrap_or_else(|| mix(&self.primary, &self.bg, 0.55)),
            danger: self.danger.unwrap_or_else(|| "#e5484d".to_string()),
            lcd_bg: self
                .lcd_bg
                .unwrap_or_else(|| mix(&self.bg, "#000000", 0.55)),
            lcd_fg: self.lcd_fg.unwrap_or_else(|| self.primary.clone()),
            lcd_dim: self
                .lcd_dim
                .unwrap_or_else(|| mix(&self.primary, &self.bg, 0.72)),
            highlight_fg: self.highlight_fg.unwrap_or_else(|| self.fg.clone()),
            bg: self.bg,
            panel: self.panel,
            fg: self.fg,
            primary: self.primary,
            accent: self.accent,
            border: self.border,
            highlight: self.highlight,
        }
    }
}

/// Read and parse one theme file.
fn load_theme_file(path: &std::path::Path) -> Result<Theme, String> {
    let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let parsed: ThemeFile = toml::from_str(&content).map_err(|e| {
        // toml's error is multi-line and points at the offending line, which
        // is exactly what you want when hand-editing a theme.
        format!("invalid TOML: {}", e.message())
    })?;

    let fallback = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Custom".to_string());

    Ok(parsed.into_theme(&fallback))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_only_starts_on_winamp_green() {
        let manager = ThemeManager::builtin_only();
        assert_eq!(manager.current_name(), "Winamp Green");
        assert_eq!(manager.current_index(), 0);
    }

    #[test]
    fn builtin_only_has_every_builtin() {
        let manager = ThemeManager::builtin_only();
        assert_eq!(manager.len(), builtin_themes().len());
        assert!(manager.warnings().is_empty());
    }

    #[test]
    fn select_changes_the_theme_and_palette() {
        let mut manager = ThemeManager::builtin_only();
        let before = manager.palette().primary;
        assert!(manager.select(1));
        assert_eq!(manager.current_index(), 1);
        assert_ne!(manager.palette().primary, before);
    }

    #[test]
    fn select_same_index_is_a_noop() {
        let mut manager = ThemeManager::builtin_only();
        assert!(!manager.select(0));
    }

    #[test]
    fn select_out_of_range_is_a_noop() {
        let mut manager = ThemeManager::builtin_only();
        assert!(!manager.select(999));
        assert_eq!(manager.current_index(), 0);
    }

    #[test]
    fn select_by_name_is_case_insensitive() {
        let mut manager = ThemeManager::builtin_only();
        assert!(manager.select_by_name("gruvbox"));
        assert_eq!(manager.current_name(), "Gruvbox");
    }

    #[test]
    fn select_by_unknown_name_is_false() {
        let mut manager = ThemeManager::builtin_only();
        assert!(!manager.select_by_name("Nonexistent"));
        assert_eq!(manager.current_index(), 0);
    }

    #[test]
    fn cycle_visits_every_theme_and_wraps() {
        let mut manager = ThemeManager::builtin_only();
        let count = manager.len();
        let start = manager.current_name().to_string();

        for _ in 0..count {
            manager.cycle();
        }
        assert_eq!(manager.current_name(), start, "cycle should wrap around");
    }

    #[test]
    fn cycle_updates_the_palette() {
        let mut manager = ThemeManager::builtin_only();
        let before = manager.palette().clone();
        manager.cycle();
        assert_ne!(*manager.palette(), before);
    }

    #[test]
    fn palette_matches_the_active_theme() {
        let mut manager = ThemeManager::builtin_only();
        manager.select_by_name("Gruvbox");
        let expected = Palette::from_theme(manager.current());
        assert_eq!(*manager.palette(), expected);
    }

    #[test]
    fn parses_a_minimal_theme_file() {
        let toml = r##"
            name = "My Theme"
            bg = "#101010"
            panel = "#202020"
            fg = "#e0e0e0"
            primary = "#00ff00"
            secondary = "#808080"
            accent = "#ffaa00"
            border = "#404040"
            highlight = "#303030"
        "##;
        let parsed: ThemeFile = toml::from_str(toml).unwrap();
        let theme = parsed.into_theme("fallback");

        assert_eq!(theme.name, "My Theme");
        assert_eq!(theme.bg, "#101010");
        // Derived fields must be filled in and well-formed.
        for (label, value) in theme.roles() {
            assert!(
                value.starts_with('#') && value.len() == 7,
                "derived role '{label}' is malformed: {value}"
            );
        }
    }

    #[test]
    fn theme_file_without_a_name_uses_the_filename() {
        let toml = r##"
            bg = "#101010"
            panel = "#202020"
            fg = "#e0e0e0"
            primary = "#00ff00"
            secondary = "#808080"
            accent = "#ffaa00"
            border = "#404040"
            highlight = "#303030"
        "##;
        let parsed: ThemeFile = toml::from_str(toml).unwrap();
        assert_eq!(parsed.into_theme("from-filename").name, "from-filename");
    }

    #[test]
    fn theme_file_accepts_explicit_derived_colors() {
        let toml = r##"
            name = "Explicit"
            bg = "#101010"
            panel = "#202020"
            fg = "#e0e0e0"
            primary = "#00ff00"
            secondary = "#808080"
            accent = "#ffaa00"
            border = "#404040"
            highlight = "#303030"
            panel_light = "#abcdef"
            lcd_bg = "#000000"
            danger = "#ff0000"
        "##;
        let parsed: ThemeFile = toml::from_str(toml).unwrap();
        let theme = parsed.into_theme("x");
        assert_eq!(theme.panel_light, "#abcdef");
        assert_eq!(theme.lcd_bg, "#000000");
        assert_eq!(theme.danger, "#ff0000");
    }

    #[test]
    fn theme_file_missing_a_required_color_fails() {
        // `primary` is missing — better to reject than render something broken.
        let toml = r##"
            bg = "#101010"
            panel = "#202020"
            fg = "#e0e0e0"
            secondary = "#808080"
            accent = "#ffaa00"
            border = "#404040"
            highlight = "#303030"
        "##;
        assert!(toml::from_str::<ThemeFile>(toml).is_err());
    }

    #[test]
    fn loading_a_missing_file_is_an_error() {
        let path = std::path::Path::new("/nonexistent/theme.toml");
        assert!(load_theme_file(path).is_err());
    }

    #[test]
    fn loading_a_valid_file_works() {
        let dir = std::env::temp_dir().join("sonus_theme_load_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("custom.toml");
        std::fs::write(
            &path,
            r##"
            name = "Custom"
            bg = "#101010"
            panel = "#202020"
            fg = "#e0e0e0"
            primary = "#00ff00"
            secondary = "#808080"
            accent = "#ffaa00"
            border = "#404040"
            highlight = "#303030"
        "##,
        )
        .unwrap();

        let theme = load_theme_file(&path).unwrap();
        assert_eq!(theme.name, "Custom");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn loading_invalid_toml_reports_the_reason() {
        let dir = std::env::temp_dir().join("sonus_theme_invalid_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("broken.toml");
        std::fs::write(&path, "this is not = = valid toml").unwrap();

        let err = load_theme_file(&path).unwrap_err();
        assert!(err.contains("TOML"), "unhelpful error: {err}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn themes_dir_is_under_the_config_directory() {
        let dir = themes_dir();
        assert!(dir.ends_with("sonus/themes") || dir.ends_with("sonus\\themes"));
    }
}
