//! The window layout and keyboard handling.

pub mod chrome;
pub mod overlays;
pub mod player;
pub mod setup;
pub mod views;

use egui::Key;

use crate::app::{SonusApp, View};

/// Draw one frame.
pub fn render(app: &mut SonusApp, ctx: &egui::Context) {
    // Track the display scale before anything is measured — every snap and
    // font size depends on it, and a window dragged to another monitor can
    // change it mid-session.
    crate::pixel::font::set_pixels_per_point(ctx.pixels_per_point());

    let palette = app.themes.palette().clone();
    palette.apply(ctx);

    let dt = ctx.input(|i| i.stable_dt).min(0.1);
    app.tick(ctx, dt);
    handle_keys(app, ctx);

    // ---- first-run dependency setup ---------------------------------------
    // When yt-dlp/ffmpeg are missing, block the whole window on a setup
    // overlay instead of showing a dead player with a red status message.
    if app.needs_dependency_setup() {
        setup::render(app, ctx, &palette);
        return;
    }

    // ---- chrome -----------------------------------------------------------
    egui::TopBottomPanel::top("titlebar")
        .exact_height(chrome::TITLEBAR_HEIGHT)
        .frame(egui::Frame::none())
        .show(ctx, |ui| chrome::titlebar(ui, app, &palette));

    egui::TopBottomPanel::bottom("statusbar")
        .exact_height(chrome::STATUS_HEIGHT)
        .frame(egui::Frame::none())
        .show(ctx, |ui| chrome::statusbar(ui, app, &palette));

    // ---- player -----------------------------------------------------------
    egui::TopBottomPanel::top("player")
        .exact_height(player::HEIGHT)
        .frame(egui::Frame::none().inner_margin(egui::Margin::symmetric(2.0, 2.0)))
        .show(ctx, |ui| player::render(ui, app, &palette));

    // ---- equalizer --------------------------------------------------------
    if app.show_eq {
        egui::TopBottomPanel::top("eq")
            .exact_height(132.0)
            .frame(egui::Frame::none().inner_margin(egui::Margin::symmetric(2.0, 0.0)))
            .show(ctx, |ui| overlays::equalizer(ui, app, &palette));
    }

    // ---- main view --------------------------------------------------------
    egui::CentralPanel::default()
        .frame(egui::Frame::none())
        .show(ctx, |ui| {
            egui::TopBottomPanel::top("tabs")
                .exact_height(22.0)
                .frame(egui::Frame::none())
                .show_inside(ui, |ui| views::tabbar(ui, app, &palette));

            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(palette.panel))
                .show_inside(ui, |ui| {
                    egui::Frame::none()
                        .inner_margin(egui::Margin::symmetric(2.0, 2.0))
                        .show(ui, |ui| views::render(ui, app, &palette));
                });
        });

    // ---- floating panels --------------------------------------------------
    if app.show_themes {
        egui::TopBottomPanel::bottom("theme_picker")
            .frame(egui::Frame::none().inner_margin(egui::Margin::symmetric(2.0, 0.0)))
            .show(ctx, |ui| overlays::theme_picker(ui, app, &palette));
    }

    if app.show_help {
        egui::TopBottomPanel::bottom("help")
            .frame(egui::Frame::none().inner_margin(egui::Margin::symmetric(2.0, 0.0)))
            .show(ctx, |ui| overlays::help(ui, app, &palette));
    }
}

/// Global keyboard shortcuts.
fn handle_keys(app: &mut SonusApp, ctx: &egui::Context) {
    // Don't steal keys while the user is typing in a text field.
    if ctx.wants_keyboard_input() {
        // Escape still closes panels — it's how you get out of a focused field.
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            app.show_themes = false;
            app.show_help = false;
            ctx.memory_mut(|m| m.surrender_focus(egui::Id::new("search_input")));
        }
        return;
    }

    let shift = ctx.input(|i| i.modifiers.shift);

    ctx.input(|input| {
        // Playback.
        if input.key_pressed(Key::Space) {
            app.toggle_play_pause();
        }

        // Seek.
        let seek_step = if shift { 30.0 } else { 5.0 };
        if input.key_pressed(Key::ArrowLeft) {
            let target = (app.position_secs - seek_step).max(0.0);
            app.seek_to(target);
        }
        if input.key_pressed(Key::ArrowRight) {
            let duration = app
                .current_track()
                .and_then(|t| t.duration_secs)
                .unwrap_or(u64::MAX) as f32;
            let target = (app.position_secs + seek_step).min(duration);
            app.seek_to(target);
        }
        if input.key_pressed(Key::ArrowUp) {
            app.set_volume(app.player.volume + 0.05);
        }
        if input.key_pressed(Key::ArrowDown) {
            app.set_volume(app.player.volume - 0.05);
        }

        // Track navigation.
        if input.key_pressed(Key::N) {
            app.next_track();
        }
        if input.key_pressed(Key::P) {
            app.previous_track();
        }

        // Focus search.
        if input.key_pressed(Key::Slash) {
            app.view = View::Search;
            app.focus_search = true;
        }

        // Play the highlighted search result.
        if input.key_pressed(Key::Enter) && app.view == View::Search {
            if let Some(index) = app.search_cursor {
                if app.search_results.get(index).is_some() {
                    let tracks: Vec<sonus_core::Track> = app
                        .search_results
                        .iter()
                        .map(sonus_core::Track::from)
                        .collect();
                    let query = app.search_input.trim().to_string();
                    app.play_list(tracks, index, sonus_core::QueueSource::Search(query));
                    app.view = View::Queue;
                }
            }
        }

        // Search result cursor.
        if app.view == View::Search && !app.search_results.is_empty() {
            let last = app.search_results.len() - 1;
            let mut cursor = app.search_cursor.unwrap_or(0);
            // Ctrl+arrows move the cursor; plain arrows seek.
            if input.modifiers.ctrl {
                if input.key_pressed(Key::ArrowDown) {
                    cursor = (cursor + 1).min(last);
                    app.search_cursor = Some(cursor);
                }
                if input.key_pressed(Key::ArrowUp) {
                    cursor = cursor.saturating_sub(1);
                    app.search_cursor = Some(cursor);
                }
            }
        }

        // Like.
        if input.key_pressed(Key::L) {
            if let Some(track) = app.current_track().cloned() {
                app.toggle_like(&track);
            }
        }

        // Shuffle and repeat.
        if input.key_pressed(Key::S) {
            app.toggle_shuffle();
        }
        if input.key_pressed(Key::R) {
            app.cycle_repeat();
        }

        // Panels.
        if input.key_pressed(Key::E) {
            app.show_eq = !app.show_eq;
        }
        if input.key_pressed(Key::T) {
            app.cycle_theme();
        }
        if input.key_pressed(Key::Escape) {
            app.show_themes = false;
            app.show_help = false;
        }
        if input.key_pressed(Key::F1) || input.key_pressed(Key::Questionmark) {
            app.show_help = !app.show_help;
        }

        // Views.
        if input.key_pressed(Key::Num1) {
            app.view = View::Search;
        }
        if input.key_pressed(Key::Num2) {
            app.view = View::Queue;
        }
        if input.key_pressed(Key::Num3) {
            app.view = View::Playlists;
        }
        if input.key_pressed(Key::Num4) {
            app.view = View::Library;
        }
    });
}

/// The `eframe::App` implementation.
impl eframe::App for SonusApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        render(self, ctx);
    }

    fn on_exit(&mut self) {
        // Persist the things worth remembering between sessions.
        let theme = self.themes.current_name().to_string();
        self.save_setting(sonus_library::setting_keys::THEME, &theme);

        let volume = self.player.volume;
        self.save_setting(sonus_library::setting_keys::VOLUME, &volume.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_heights_fit_a_compact_window() {
        // Title + player + tabs + status must leave usable room in a 620px
        // window; if this ever fails the list views become unusable.
        let chrome_height = chrome::TITLEBAR_HEIGHT + player::HEIGHT + 22.0 + chrome::STATUS_HEIGHT;
        assert!(
            chrome_height < 200.0,
            "chrome eats too much of the window: {chrome_height}px"
        );
    }

    #[test]
    fn eq_panel_leaves_room_for_the_list() {
        let with_eq =
            chrome::TITLEBAR_HEIGHT + player::HEIGHT + 132.0 + 22.0 + chrome::STATUS_HEIGHT;
        // Even with the EQ open there must be a usable list area at the
        // default window height of 620px.
        assert!(620.0 - with_eq > 150.0);
    }
}
