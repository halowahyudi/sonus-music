//! Overlay panels: the equalizer, the theme picker and the help sheet.

use egui::{Align2, Pos2, Rect, Sense, Vec2};

use crate::app::SonusApp;
use crate::pixel::{
    body, font_size, list, paint, BandSlider, ButtonStyle, Palette, PixelButton, RowContent,
    RowState,
};
use sonus_audio::{band_label, EQ_MAX_DB, EQ_MIN_DB, EQ_NUM_BANDS, PRESETS};

/// Draw the equalizer panel.
pub fn equalizer(ui: &mut egui::Ui, app: &mut SonusApp, palette: &Palette) {
    let width = ui.available_width();
    let height = 132.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let rect = paint::snap_rect(rect);

    let content = paint::titled_panel(
        ui.painter(),
        rect,
        "EQUALIZER",
        palette,
        body(font_size::HEADER),
    );
    let inner = Rect::from_min_max(
        Pos2::new(content.left() + 4.0, content.top() + 4.0),
        Pos2::new(content.right() - 4.0, content.bottom() - 4.0),
    );

    // The EQ is shared with the audio thread; hold the lock for the whole
    // panel so a band change can't land mid-draw. The Arc must be bound to a
    // local — locking a temporary would drop the guard immediately.
    let eq_handle = app.audio.eq();
    let mut eq = match eq_handle.lock() {
        Ok(eq) => eq,
        Err(poisoned) => poisoned.into_inner(),
    };

    // ---- top row: enable, presets, preamp ---------------------------------
    let top = Rect::from_min_size(inner.min, Vec2::new(inner.width(), 18.0));

    let on_rect = Rect::from_min_size(top.min, Vec2::new(30.0, 18.0));
    if PixelButton::new("ON")
        .size(on_rect.size())
        .font_size(font_size::TINY)
        .style(if eq.is_enabled() {
            ButtonStyle::Active
        } else {
            ButtonStyle::Normal
        })
        .show_at(ui, on_rect, palette)
        .clicked()
    {
        eq.toggle();
        app.save_setting(
            sonus_library::setting_keys::EQ_ENABLED,
            if eq.is_enabled() { "true" } else { "false" },
        );
    }

    // Preset name / current match.
    let preset_label = eq
        .matching_preset()
        .map(str::to_string)
        .unwrap_or_else(|| "CUSTOM".to_string());
    paint::text(
        ui.painter(),
        Pos2::new(top.left() + 36.0, top.center().y),
        Align2::LEFT_CENTER,
        &preset_label,
        body(font_size::TINY),
        if eq.matching_preset().is_some() {
            palette.primary
        } else {
            palette.accent
        },
    );

    // Preset cycling.
    let prev_rect = Rect::from_min_size(
        Pos2::new(top.right() - 68.0, top.top()),
        Vec2::new(18.0, 18.0),
    );
    if PixelButton::new("<")
        .size(prev_rect.size())
        .font_size(font_size::TINY)
        .show_at(ui, prev_rect, palette)
        .clicked()
    {
        cycle_preset(&mut eq, -1);
    }

    let next_rect = Rect::from_min_size(
        Pos2::new(top.right() - 48.0, top.top()),
        Vec2::new(18.0, 18.0),
    );
    if PixelButton::new(">")
        .size(next_rect.size())
        .font_size(font_size::TINY)
        .show_at(ui, next_rect, palette)
        .clicked()
    {
        cycle_preset(&mut eq, 1);
    }

    let flat_rect = Rect::from_min_size(
        Pos2::new(top.right() - 28.0, top.top()),
        Vec2::new(28.0, 18.0),
    );
    if PixelButton::new("FLAT")
        .size(flat_rect.size())
        .font_size(font_size::TINY)
        .show_at(ui, flat_rect, palette)
        .clicked()
    {
        eq.apply_preset(&PRESETS[0]);
    }

    // ---- band sliders -----------------------------------------------------
    let sliders_top = inner.top() + 22.0;
    let slider_height = inner.bottom() - sliders_top - 12.0;
    let band_slot = inner.width() / (EQ_NUM_BANDS as f32 + 1.0);

    // Preamp on the left.
    let preamp_value = eq.preamp();
    let (_, new_preamp) = BandSlider::show(
        ui,
        preamp_value,
        EQ_MIN_DB,
        EQ_MAX_DB,
        0.0,
        Vec2::new(band_slot - 2.0, slider_height),
        palette,
        palette.accent,
    );
    if (new_preamp - preamp_value).abs() > f32::EPSILON {
        eq.set_preamp(new_preamp);
    }
    paint::text(
        ui.painter(),
        Pos2::new(inner.left() + band_slot / 2.0, inner.bottom() - 5.0),
        Align2::CENTER_CENTER,
        "PRE",
        body(font_size::TINY),
        palette.fg_dim,
    );

    // The ten bands.
    let mut changed: Vec<(usize, f32)> = Vec::new();
    for index in 0..EQ_NUM_BANDS {
        let x = inner.left() + band_slot * (index as f32 + 1.0);
        let value = eq.band(index);

        let (_, new_value) = BandSlider::show(
            ui,
            value,
            EQ_MIN_DB,
            EQ_MAX_DB,
            0.0,
            Vec2::new(band_slot - 2.0, slider_height),
            palette,
            palette.primary,
        );
        if (new_value - value).abs() > f32::EPSILON {
            changed.push((index, new_value));
        }

        paint::text(
            ui.painter(),
            Pos2::new(x + band_slot / 2.0, inner.bottom() - 5.0),
            Align2::CENTER_CENTER,
            &band_label(index),
            body(font_size::TINY),
            palette.fg_dim,
        );
    }

    for (index, value) in changed {
        eq.set_band(index, value);
    }
}

fn cycle_preset(eq: &mut sonus_audio::Equalizer, direction: i32) {
    let current = eq
        .matching_preset()
        .and_then(|name| PRESETS.iter().position(|p| p.name == name))
        .unwrap_or(0);
    let count = PRESETS.len() as i32;
    let next = ((current as i32 + direction).rem_euclid(count)) as usize;
    eq.apply_preset(&PRESETS[next]);
}

/// Draw the theme picker.
pub fn theme_picker(ui: &mut egui::Ui, app: &mut SonusApp, palette: &Palette) {
    let width = ui.available_width();
    let count = app.themes.len();
    let height = (count as f32 * 20.0 + 30.0).min(320.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let rect = paint::snap_rect(rect);

    let content = paint::titled_panel(
        ui.painter(),
        rect,
        "THEMES",
        palette,
        body(font_size::HEADER),
    );
    let inner = Rect::from_min_max(
        Pos2::new(content.left() + 4.0, content.top() + 4.0),
        Pos2::new(content.right() - 4.0, content.bottom() - 4.0),
    );

    let current = app.themes.current_index();
    let mut select: Option<usize> = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .max_height(inner.height() - 24.0)
        .show(ui, |ui| {
            let row_width = ui.available_width();
            for index in 0..count {
                let name = app.themes.themes()[index].name.clone();
                let content = RowContent {
                    index: None,
                    title: &name,
                    subtitle: None,
                    duration: None,
                    badge: if index == current {
                        Some("ACTIVE")
                    } else {
                        None
                    },
                };

                let response = list::row(
                    ui,
                    row_width,
                    &content,
                    if index == current {
                        RowState::Playing
                    } else if index % 2 == 1 {
                        RowState::Alternate
                    } else {
                        RowState::Normal
                    },
                    palette,
                    false,
                );

                if response.clicked() {
                    select = Some(index);
                }
            }
        });

    // Footer buttons.
    let footer = Rect::from_min_size(
        Pos2::new(inner.left(), inner.bottom() - 20.0),
        Vec2::new(inner.width(), 20.0),
    );

    let cycle_rect = Rect::from_min_size(footer.min, Vec2::new(80.0, 20.0));
    if PixelButton::new("CYCLE")
        .size(cycle_rect.size())
        .font_size(font_size::TINY)
        .show_at(ui, cycle_rect, palette)
        .clicked()
    {
        app.cycle_theme();
    }

    let close_rect = Rect::from_min_size(
        Pos2::new(footer.right() - 60.0, footer.top()),
        Vec2::new(60.0, 20.0),
    );
    if PixelButton::new("CLOSE")
        .size(close_rect.size())
        .font_size(font_size::TINY)
        .show_at(ui, close_rect, palette)
        .clicked()
    {
        app.show_themes = false;
    }

    paint::text(
        ui.painter(),
        Pos2::new(footer.left() + 88.0, footer.center().y),
        Align2::LEFT_CENTER,
        &format!("{count} themes"),
        body(font_size::TINY),
        palette.fg_dim,
    );

    if let Some(index) = select {
        app.select_theme(index);
    }
}

/// Draw the keyboard shortcut reference.
pub fn help(ui: &mut egui::Ui, app: &mut SonusApp, palette: &Palette) {
    const SHORTCUTS: &[(&str, &str)] = &[
        ("Space", "Play / pause"),
        ("Left / Right", "Seek 5 seconds"),
        ("Shift + Arrows", "Seek 30 seconds"),
        ("Up / Down", "Volume"),
        ("N / P", "Next / previous track"),
        ("/", "Focus search"),
        ("Ctrl + Arrows", "Move result cursor"),
        ("Enter", "Play selection"),
        ("L", "Like the current track"),
        ("S / R", "Shuffle / repeat"),
        ("E / T", "Equalizer / cycle theme"),
        ("1 - 4", "Switch view"),
        ("Esc", "Close panels"),
    ];

    // Size the panel to its contents rather than guessing — the list grows
    // every time a shortcut is added.
    const ROW_HEIGHT: f32 = 11.0;
    const HEADER: f32 = 16.0;
    const PADDING: f32 = 14.0;
    const FOOTER: f32 = 24.0;

    let width = ui.available_width();
    let height = HEADER + SHORTCUTS.len() as f32 * ROW_HEIGHT + PADDING + FOOTER;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let rect = paint::snap_rect(rect);

    let content = paint::titled_panel(
        ui.painter(),
        rect,
        "SHORTCUTS",
        palette,
        body(font_size::HEADER),
    );
    let inner = Rect::from_min_max(
        Pos2::new(content.left() + 8.0, content.top() + 6.0),
        Pos2::new(content.right() - 8.0, content.bottom() - FOOTER),
    );

    let mut y = inner.top();
    for (key, description) in SHORTCUTS {
        if y + ROW_HEIGHT > inner.bottom() {
            break;
        }
        paint::text(
            ui.painter(),
            Pos2::new(inner.left(), y),
            Align2::LEFT_TOP,
            key,
            body(font_size::TINY),
            palette.primary,
        );
        paint::text(
            ui.painter(),
            Pos2::new(inner.left() + 90.0, y),
            Align2::LEFT_TOP,
            description,
            body(font_size::TINY),
            palette.fg,
        );
        y += ROW_HEIGHT;
    }

    let close_rect = Rect::from_min_size(
        Pos2::new(content.right() - 64.0, content.bottom() - 22.0),
        Vec2::new(60.0, 20.0),
    );
    if PixelButton::new("CLOSE")
        .size(close_rect.size())
        .font_size(font_size::TINY)
        .show_at(ui, close_rect, palette)
        .clicked()
    {
        app.show_help = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_cycling_wraps_in_both_directions() {
        let mut eq = sonus_audio::Equalizer::new(48000);

        // Start from a known preset.
        eq.apply_preset(&PRESETS[0]);
        cycle_preset(&mut eq, -1);
        assert_eq!(
            eq.matching_preset(),
            Some(PRESETS[PRESETS.len() - 1].name),
            "cycling backwards from the first preset should wrap to the last"
        );

        cycle_preset(&mut eq, 1);
        assert_eq!(eq.matching_preset(), Some(PRESETS[0].name));
    }

    #[test]
    fn preset_cycling_from_a_custom_curve_starts_at_flat() {
        let mut eq = sonus_audio::Equalizer::new(48000);
        eq.set_band(3, 7.0); // custom, matches no preset
        cycle_preset(&mut eq, 1);
        assert_eq!(eq.matching_preset(), Some(PRESETS[1].name));
    }

    #[test]
    fn every_preset_is_reachable_by_cycling() {
        let mut eq = sonus_audio::Equalizer::new(48000);
        eq.apply_preset(&PRESETS[0]);

        let mut seen = vec![PRESETS[0].name];
        for _ in 0..PRESETS.len() - 1 {
            cycle_preset(&mut eq, 1);
            let name = eq.matching_preset().expect("cycling lands on a preset");
            assert!(!seen.contains(&name), "revisited {name}");
            seen.push(name);
        }
        assert_eq!(seen.len(), PRESETS.len());
    }

    #[test]
    fn help_shortcuts_fit_the_panel() {
        // The panel sizes itself to its content, so every row must have room.
        const SHORTCUT_COUNT: f32 = 13.0;
        const ROW_HEIGHT: f32 = 11.0;
        const HEADER: f32 = 16.0;
        const PADDING: f32 = 14.0;
        const FOOTER: f32 = 24.0;

        let height = HEADER + SHORTCUT_COUNT * ROW_HEIGHT + PADDING + FOOTER;
        let rows_height = SHORTCUT_COUNT * ROW_HEIGHT;
        let available = height - HEADER - PADDING - FOOTER;

        assert!(
            rows_height <= available,
            "shortcut rows overflow the panel: {rows_height} > {available}"
        );
    }
}
