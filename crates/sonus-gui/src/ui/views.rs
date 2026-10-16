//! The four main views: search, queue, playlists and library.

use egui::{Align2, Pos2, Rect, Sense, Vec2};

use crate::app::{SonusApp, View};
use crate::pixel::{
    body, font_size, list, paint, ButtonStyle, Palette, PixelButton, RowContent, RowState,
};
use sonus_core::{QueueSource, Track};

/// Draw whichever view is active.
pub fn render(ui: &mut egui::Ui, app: &mut SonusApp, palette: &Palette) {
    match app.view {
        View::Search => search(ui, app, palette),
        View::Queue => queue(ui, app, palette),
        View::Playlists => playlists(ui, app, palette),
        View::Library => library(ui, app, palette),
    }
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

fn search(ui: &mut egui::Ui, app: &mut SonusApp, palette: &Palette) {
    let width = ui.available_width();

    // ---- search box -------------------------------------------------------
    let (box_rect, _) = ui.allocate_exact_size(Vec2::new(width, 26.0), Sense::hover());
    let box_rect = paint::snap_rect(box_rect);
    paint::surface(
        ui.painter(),
        box_rect,
        palette.panel,
        paint::Bevel::Flat,
        palette,
    );

    let input_rect = Rect::from_min_max(
        Pos2::new(box_rect.left() + 4.0, box_rect.top() + 4.0),
        Pos2::new(box_rect.right() - 52.0, box_rect.bottom() - 4.0),
    );

    let response = ui.put(
        input_rect,
        egui::TextEdit::singleline(&mut app.search_input)
            .hint_text("Search YouTube Music")
            .desired_width(input_rect.width())
            .frame(false)
            .text_color(palette.fg)
            .font(body(font_size::BODY))
            .id(egui::Id::new("search_input")),
    );

    if app.focus_search {
        response.request_focus();
        app.focus_search = false;
    }

    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        app.start_search();
        app.focus_search = true;
    }

    let go_rect = Rect::from_min_size(
        Pos2::new(box_rect.right() - 46.0, box_rect.top() + 4.0),
        Vec2::new(42.0, box_rect.height() - 8.0),
    );
    if PixelButton::new("GO")
        .size(go_rect.size())
        .style(if app.search_pending.is_running() {
            ButtonStyle::Disabled
        } else {
            ButtonStyle::Active
        })
        .show_at(ui, go_rect, palette)
        .clicked()
    {
        app.start_search();
    }

    ui.add_space(2.0);

    // ---- results ----------------------------------------------------------
    if app.search_pending.is_running() {
        let elapsed = app.search_pending.elapsed().as_secs_f32();
        let dots = ".".repeat((elapsed * 3.0) as usize % 4);
        list::empty_state(
            ui,
            &format!("Searching{dots}"),
            "yt-dlp is asking YouTube Music",
            palette,
        );
        return;
    }

    if app.search_results.is_empty() {
        list::empty_state(
            ui,
            "Search for a song",
            "Press Enter or click GO. Results are music only.",
            palette,
        );
        return;
    }

    list::header(
        ui,
        ui.available_width(),
        "RESULTS",
        Some(app.search_results.len()),
        palette,
    );

    // A thin hint while artists are still being fetched in the background —
    // the list is already usable, so this is information, not a blocker.
    if app.enrich_pending.is_running() {
        let (hint_rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 12.0), Sense::hover());
        let dots = ".".repeat((app.elapsed * 3.0) as usize % 4);
        paint::text(
            ui.painter(),
            Pos2::new(hint_rect.left() + 6.0, hint_rect.center().y),
            Align2::LEFT_CENTER,
            &format!("fetching artists{dots}"),
            body(font_size::TINY),
            palette.fg_dim,
        );
    }

    let playing_id = app
        .current_track()
        .map(|t| t.youtube_id.clone())
        .unwrap_or_default();

    let results: Vec<_> = app
        .search_results
        .iter()
        .map(|r| {
            (
                r.youtube_id.clone(),
                r.title.clone(),
                r.artist.clone(),
                r.duration_secs,
            )
        })
        .collect();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let row_width = ui.available_width();
            for (index, (id, title, artist, duration)) in results.iter().enumerate() {
                let is_playing = *id == playing_id;
                let state = if is_playing {
                    RowState::Playing
                } else if index % 2 == 1 {
                    RowState::Alternate
                } else {
                    RowState::Normal
                };

                let duration_label = duration.map(|d| format!("{}:{:02}", d / 60, d % 60));

                let content = RowContent {
                    index: Some(index),
                    title,
                    subtitle: artist.as_deref(),
                    duration: duration_label.as_deref(),
                    badge: None,
                };

                let response = list::row(ui, row_width, &content, state, palette, app.is_liked(id));

                // A click plays the track right away with a fresh radio tail —
                // search no longer dumps every result into the queue.
                if response.clicked() {
                    play_search_now(app, index);
                }
                if response.double_clicked() {
                    play_search_now(app, index);
                }

                // Everything else lives in the context menu.
                response.context_menu(|ui| {
                    if ui.button("Play now").clicked() {
                        play_search_now(app, index);
                        ui.close_menu();
                    }
                    if ui.button("Play next").clicked() {
                        play_next_search_result(app, index);
                        ui.close_menu();
                    }
                    if ui.button("Add to queue").clicked() {
                        enqueue_search_result(app, index);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Start radio").clicked() {
                        // Restart playback from this track and let its
                        // recommendations replace the current autoplay tail.
                        play_search_now(app, index);
                        ui.close_menu();
                    }
                    if ui
                        .button(if app.is_liked(id) { "Unlike" } else { "Like" })
                        .clicked()
                    {
                        if let Some(result) = app.search_results.get(index) {
                            let track: Track = result.into();
                            app.toggle_like(&track);
                        }
                        ui.close_menu();
                    }
                });
            }
            ui.add_space(6.0);
        });
}

fn play_search_now(app: &mut SonusApp, index: usize) {
    if let Some(result) = app.search_results.get(index) {
        let track: Track = result.into();
        app.play_now(track);
    }
}

fn play_next_search_result(app: &mut SonusApp, index: usize) {
    if let Some(result) = app.search_results.get(index) {
        let track: Track = result.into();
        app.play_next(track);
    }
}

fn enqueue_search_result(app: &mut SonusApp, index: usize) {
    if let Some(result) = app.search_results.get(index) {
        let track: Track = result.into();
        app.enqueue(track);
    }
}

// ---------------------------------------------------------------------------
// Queue
// ---------------------------------------------------------------------------

fn queue(ui: &mut egui::Ui, app: &mut SonusApp, palette: &Palette) {
    let width = ui.available_width();

    // ---- header -----------------------------------------------------------
    let (header_rect, _) = ui.allocate_exact_size(Vec2::new(width, 24.0), Sense::hover());
    let header_rect = paint::snap_rect(header_rect);
    paint::fill(ui.painter(), header_rect, palette.panel);

    let source = app.player.queue.source().label();
    paint::text(
        ui.painter(),
        Pos2::new(header_rect.left() + 6.0, header_rect.center().y),
        Align2::LEFT_CENTER,
        &source,
        body(font_size::SMALL),
        palette.primary,
    );

    let count_text = format!(
        "{} tracks  {}",
        app.player.queue.len(),
        app.player.queue.total_duration_label()
    );
    paint::text(
        ui.painter(),
        Pos2::new(header_rect.right() - 6.0, header_rect.center().y),
        Align2::RIGHT_CENTER,
        &count_text,
        body(font_size::TINY),
        palette.fg_dim,
    );

    // Radio status badge.
    if app.player.radio_pending.is_running() {
        paint::text(
            ui.painter(),
            Pos2::new(header_rect.center().x, header_rect.center().y),
            Align2::CENTER_CENTER,
            "FINDING MORE...",
            body(font_size::TINY),
            palette.accent,
        );
    } else if app.player.radio_enabled {
        paint::text(
            ui.painter(),
            Pos2::new(header_rect.center().x, header_rect.center().y),
            Align2::CENTER_CENTER,
            "RADIO ON",
            body(font_size::TINY),
            palette.primary_dim,
        );
    }

    ui.add_space(2.0);

    if app.player.queue.is_empty() {
        list::empty_state(
            ui,
            "The queue is empty",
            "Play something and it'll show up here",
            palette,
        );
        return;
    }

    // ---- rows -------------------------------------------------------------
    let cursor = app.player.queue.current_index();
    let items: Vec<(usize, sonus_core::QueueItem)> = app
        .player
        .queue
        .items()
        .iter()
        .cloned()
        .enumerate()
        .collect();

    // Actions are collected and applied after the loop — mutating the queue
    // while iterating it would invalidate the indices.
    let mut jump_to: Option<usize> = None;
    let mut remove: Option<usize> = None;
    let mut move_up: Option<usize> = None;
    let mut move_down: Option<usize> = None;
    let mut like: Option<Track> = None;
    let mut play_next: Option<Track> = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let row_width = ui.available_width();
            for (index, item) in &items {
                let track = &item.track;
                let is_current = Some(*index) == cursor;
                let state = if is_current {
                    RowState::Playing
                } else if *index % 2 == 1 {
                    RowState::Alternate
                } else {
                    RowState::Normal
                };

                let duration = track.duration_label();
                let content = RowContent {
                    index: Some(*index),
                    title: &track.title,
                    subtitle: track.artist.as_deref(),
                    duration: Some(&duration),
                    badge: if item.is_autoplay { Some("AUTO") } else { None },
                };

                let response = list::row(
                    ui,
                    row_width,
                    &content,
                    state,
                    palette,
                    app.is_liked(&track.youtube_id),
                );

                if response.clicked() {
                    jump_to = Some(*index);
                }
                if response.secondary_clicked() {
                    remove = Some(*index);
                }

                // A small context menu keeps the row clean.
                response.context_menu(|ui| {
                    if ui.button("Play").clicked() {
                        jump_to = Some(*index);
                        ui.close_menu();
                    }
                    if ui.button("Play next").clicked() {
                        play_next = Some(track.clone());
                        ui.close_menu();
                    }
                    if ui
                        .button(if app.is_liked(&track.youtube_id) {
                            "Unlike"
                        } else {
                            "Like"
                        })
                        .clicked()
                    {
                        like = Some(track.clone());
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Move up").clicked() {
                        move_up = Some(*index);
                        ui.close_menu();
                    }
                    if ui.button("Move down").clicked() {
                        move_down = Some(*index);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Remove").clicked() {
                        remove = Some(*index);
                        ui.close_menu();
                    }
                });
            }
            ui.add_space(6.0);
        });

    // ---- apply ------------------------------------------------------------
    if let Some(index) = jump_to {
        if app.player.queue.jump_to(index).is_some() {
            let track = app.player.queue.current().cloned();
            if let Some(track) = track {
                app.audio.play(&track.youtube_id);
                app.position_secs = 0.0;
                app.spectrum.reset();
            }
        }
    }
    if let Some(index) = move_up {
        if index > 0 {
            app.player.queue.reorder(index, index - 1);
        }
    }
    if let Some(index) = move_down {
        app.player.queue.reorder(index, index + 1);
    }
    if let Some(track) = play_next {
        app.play_next(track);
    }
    if let Some(track) = like {
        app.toggle_like(&track);
    }
    if let Some(index) = remove {
        let was_current = Some(index) == cursor;
        app.player.queue.remove(index);
        if was_current {
            // The next track slid into the cursor position — play it.
            if let Some(track) = app.player.queue.current().cloned() {
                app.audio.play(&track.youtube_id);
                app.position_secs = 0.0;
            } else {
                app.audio.stop();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Playlists
// ---------------------------------------------------------------------------

fn playlists(ui: &mut egui::Ui, app: &mut SonusApp, palette: &Palette) {
    // If a playlist is open, show its contents instead of the list.
    if app.open_playlist.is_some() {
        open_playlist_view(ui, app, palette);
        return;
    }

    let width = ui.available_width();

    // ---- create bar -------------------------------------------------------
    let (bar_rect, _) = ui.allocate_exact_size(Vec2::new(width, 26.0), Sense::hover());
    let bar_rect = paint::snap_rect(bar_rect);
    paint::surface(
        ui.painter(),
        bar_rect,
        palette.panel,
        paint::Bevel::Flat,
        palette,
    );

    let input_rect = Rect::from_min_max(
        Pos2::new(bar_rect.left() + 4.0, bar_rect.top() + 4.0),
        Pos2::new(bar_rect.right() - 52.0, bar_rect.bottom() - 4.0),
    );
    ui.put(
        input_rect,
        egui::TextEdit::singleline(&mut app.new_playlist_name)
            .hint_text("New playlist name")
            .desired_width(input_rect.width())
            .frame(false)
            .text_color(palette.fg)
            .font(body(font_size::BODY))
            .id(egui::Id::new("new_playlist")),
    );

    let create_rect = Rect::from_min_size(
        Pos2::new(bar_rect.right() - 46.0, bar_rect.top() + 4.0),
        Vec2::new(42.0, bar_rect.height() - 8.0),
    );
    if PixelButton::new("NEW")
        .size(create_rect.size())
        .style(ButtonStyle::Active)
        .show_at(ui, create_rect, palette)
        .clicked()
    {
        let name = app.new_playlist_name.clone();
        app.create_playlist(&name);
        app.new_playlist_name.clear();
    }

    ui.add_space(2.0);

    // Save the current queue as a playlist.
    if !app.player.queue.is_empty() {
        let save_rect = Rect::from_min_size(
            Pos2::new(ui.cursor().left(), ui.cursor().top()),
            Vec2::new(ui.available_width(), 20.0),
        );
        let label = format!("SAVE QUEUE AS PLAYLIST ({})", app.player.queue.len());
        if PixelButton::new(&label)
            .size(save_rect.size())
            .font_size(font_size::TINY)
            .show_at(ui, save_rect, palette)
            .clicked()
        {
            let name = if app.new_playlist_name.trim().is_empty() {
                format!("Queue {}", app.playlists.len() + 1)
            } else {
                app.new_playlist_name.clone()
            };
            app.save_queue_as_playlist(&name);
            app.new_playlist_name.clear();
        }
        ui.allocate_space(Vec2::new(ui.available_width(), 22.0));
    }

    if app.playlists.is_empty() {
        list::empty_state(ui, "No playlists yet", "Create one above", palette);
        return;
    }

    list::header(
        ui,
        ui.available_width(),
        "PLAYLISTS",
        Some(app.playlists.len()),
        palette,
    );

    let playlists: Vec<(String, String, usize)> = app.playlists.clone();
    let mut open: Option<String> = None;
    let mut play: Option<String> = None;
    let mut delete: Option<String> = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let row_width = ui.available_width();
            for (index, (id, name, count)) in playlists.iter().enumerate() {
                let count_label = format!("{count}");
                let content = RowContent {
                    index: None,
                    title: name,
                    subtitle: None,
                    duration: Some(&count_label),
                    badge: None,
                };

                let response = list::row(
                    ui,
                    row_width,
                    &content,
                    if index % 2 == 1 {
                        RowState::Alternate
                    } else {
                        RowState::Normal
                    },
                    palette,
                    false,
                );

                if response.clicked() {
                    open = Some(id.clone());
                }
                if response.double_clicked() {
                    play = Some(id.clone());
                }

                response.context_menu(|ui| {
                    if ui.button("Play").clicked() {
                        play = Some(id.clone());
                        ui.close_menu();
                    }
                    if ui.button("Open").clicked() {
                        open = Some(id.clone());
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Delete").clicked() {
                        delete = Some(id.clone());
                        ui.close_menu();
                    }
                });
            }
            ui.add_space(6.0);
        });

    if let Some(id) = open {
        app.open_playlist(&id);
    }
    if let Some(id) = play {
        app.play_playlist(&id);
        app.view = View::Queue;
    }
    if let Some(id) = delete {
        app.delete_playlist(&id);
    }
}

fn open_playlist_view(ui: &mut egui::Ui, app: &mut SonusApp, palette: &Palette) {
    let Some(playlist) = app.open_playlist.clone() else {
        return;
    };

    let width = ui.available_width();
    let (header_rect, _) = ui.allocate_exact_size(Vec2::new(width, 24.0), Sense::hover());
    let header_rect = paint::snap_rect(header_rect);
    paint::fill(ui.painter(), header_rect, palette.panel);

    // Back button.
    let back_rect = Rect::from_min_size(
        Pos2::new(header_rect.left() + 2.0, header_rect.top() + 3.0),
        Vec2::new(28.0, 18.0),
    );
    if PixelButton::new("<")
        .size(back_rect.size())
        .font_size(font_size::SMALL)
        .show_at(ui, back_rect, palette)
        .clicked()
    {
        app.open_playlist = None;
        return;
    }

    paint::text(
        ui.painter(),
        Pos2::new(header_rect.left() + 36.0, header_rect.center().y),
        Align2::LEFT_CENTER,
        &playlist.name,
        body(font_size::SMALL),
        palette.primary,
    );

    let info = format!(
        "{} tracks  {}",
        playlist.len(),
        playlist.total_duration_label()
    );
    paint::text(
        ui.painter(),
        Pos2::new(header_rect.right() - 56.0, header_rect.center().y),
        Align2::RIGHT_CENTER,
        &info,
        body(font_size::TINY),
        palette.fg_dim,
    );

    let play_rect = Rect::from_min_size(
        Pos2::new(header_rect.right() - 50.0, header_rect.top() + 3.0),
        Vec2::new(48.0, 18.0),
    );
    if PixelButton::new("PLAY")
        .size(play_rect.size())
        .font_size(font_size::TINY)
        .style(ButtonStyle::Active)
        .show_at(ui, play_rect, palette)
        .clicked()
    {
        let id = playlist.id.clone();
        app.play_playlist(&id);
        app.view = View::Queue;
    }

    ui.add_space(2.0);

    if playlist.is_empty() {
        list::empty_state(
            ui,
            "This playlist is empty",
            "Add tracks from the queue",
            palette,
        );
        return;
    }

    let mut jump_to: Option<usize> = None;
    let mut remove: Option<String> = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let row_width = ui.available_width();
            for (index, track) in playlist.tracks.iter().enumerate() {
                let duration = track.duration_label();
                let content = RowContent {
                    index: Some(index),
                    title: &track.title,
                    subtitle: track.artist.as_deref(),
                    duration: Some(&duration),
                    badge: None,
                };

                let response = list::row(
                    ui,
                    row_width,
                    &content,
                    if index % 2 == 1 {
                        RowState::Alternate
                    } else {
                        RowState::Normal
                    },
                    palette,
                    app.is_liked(&track.youtube_id),
                );

                if response.clicked() {
                    jump_to = Some(index);
                }

                let id = track.youtube_id.clone();
                response.context_menu(|ui| {
                    if ui.button("Play from here").clicked() {
                        jump_to = Some(index);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Remove from playlist").clicked() {
                        remove = Some(id.clone());
                        ui.close_menu();
                    }
                });
            }
            ui.add_space(6.0);
        });

    if let Some(index) = jump_to {
        let name = playlist.name.clone();
        app.play_list(playlist.tracks.clone(), index, QueueSource::Playlist(name));
        app.view = View::Queue;
    }

    if let Some(youtube_id) = remove {
        if let Some(db) = app.db.as_ref() {
            let _ = sonus_library::Playlists::remove_track(db, &playlist.id, &youtube_id);
        }
        if let Some(open) = &mut app.open_playlist {
            open.remove(&youtube_id);
        }
        app.refresh_library();
    }
}

// ---------------------------------------------------------------------------
// Library
// ---------------------------------------------------------------------------

fn library(ui: &mut egui::Ui, app: &mut SonusApp, palette: &Palette) {
    // Two sections: liked tracks and recent history.
    let liked: Vec<Track> = app
        .recent
        .iter()
        .filter(|t| app.is_liked(&t.youtube_id))
        .cloned()
        .collect();

    let mut jump_liked: Option<usize> = None;
    let mut jump_recent: Option<usize> = None;
    let mut unlike: Option<Track> = None;
    let mut enqueue: Option<Track> = None;

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let row_width = ui.available_width();

            list::header(ui, row_width, "LIKED", Some(app.liked_ids.len()), palette);
            if liked.is_empty() {
                list::empty_state(
                    ui,
                    "Nothing liked yet",
                    "Press * while a track plays",
                    palette,
                );
            } else {
                for (index, track) in liked.iter().enumerate() {
                    let duration = track.duration_label();
                    let content = RowContent {
                        index: Some(index),
                        title: &track.title,
                        subtitle: track.artist.as_deref(),
                        duration: Some(&duration),
                        badge: None,
                    };
                    let response = list::row(
                        ui,
                        row_width,
                        &content,
                        if index % 2 == 1 {
                            RowState::Alternate
                        } else {
                            RowState::Normal
                        },
                        palette,
                        true,
                    );
                    if response.clicked() {
                        jump_liked = Some(index);
                    }
                    let track = track.clone();
                    response.context_menu(|ui| {
                        if ui.button("Play from here").clicked() {
                            jump_liked = Some(index);
                            ui.close_menu();
                        }
                        if ui.button("Unlike").clicked() {
                            unlike = Some(track.clone());
                            ui.close_menu();
                        }
                    });
                }
            }

            ui.add_space(4.0);
            list::header(ui, row_width, "RECENT", Some(app.recent.len()), palette);
            if app.recent.is_empty() {
                list::empty_state(ui, "Nothing played yet", "", palette);
            } else {
                for (index, track) in app.recent.iter().enumerate() {
                    let duration = track.duration_label();
                    let content = RowContent {
                        index: Some(index),
                        title: &track.title,
                        subtitle: track.artist.as_deref(),
                        duration: Some(&duration),
                        badge: None,
                    };
                    let response = list::row(
                        ui,
                        row_width,
                        &content,
                        if index % 2 == 1 {
                            RowState::Alternate
                        } else {
                            RowState::Normal
                        },
                        palette,
                        app.is_liked(&track.youtube_id),
                    );
                    if response.clicked() {
                        jump_recent = Some(index);
                    }
                    let track = track.clone();
                    response.context_menu(|ui| {
                        if ui.button("Play from here").clicked() {
                            jump_recent = Some(index);
                            ui.close_menu();
                        }
                        if ui.button("Add to queue").clicked() {
                            enqueue = Some(track.clone());
                            ui.close_menu();
                        }
                    });
                }
            }
            ui.add_space(6.0);
        });

    if let Some(index) = jump_liked {
        app.play_list(liked, index, QueueSource::Manual);
        app.view = View::Queue;
    }
    if let Some(index) = jump_recent {
        let recent = app.recent.clone();
        app.play_list(recent, index, QueueSource::Manual);
        app.view = View::Queue;
    }
    if let Some(track) = unlike {
        app.toggle_like(&track);
    }
    if let Some(track) = enqueue {
        app.enqueue(track);
    }
}

/// Draw the view-switching tab bar.
pub fn tabbar(ui: &mut egui::Ui, app: &mut SonusApp, palette: &Palette) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 22.0), Sense::hover());
    let rect = paint::snap_rect(rect);

    paint::fill(ui.painter(), rect, palette.panel_dark);

    let tab_width = (width / View::ALL.len() as f32).floor();
    let mut switch_to = None;

    for (index, view) in View::ALL.iter().enumerate() {
        let tab_rect = Rect::from_min_size(
            Pos2::new(rect.left() + index as f32 * tab_width, rect.top()),
            Vec2::new(tab_width, rect.height()),
        );

        let active = app.view == *view;
        let response = ui.interact(tab_rect, ui.id().with("tab").with(index), Sense::click());

        if active {
            paint::fill(ui.painter(), tab_rect, palette.panel);
            // Active tab joins the panel below it — no bottom border.
            paint::hline(
                ui.painter(),
                tab_rect.left(),
                tab_rect.right(),
                tab_rect.top(),
                palette.primary,
            );
        } else if response.hovered() {
            paint::fill(ui.painter(), tab_rect, palette.row_hover);
        }

        let label = format!("{} {}", view.icon(), view.label());
        paint::text(
            ui.painter(),
            tab_rect.center(),
            Align2::CENTER_CENTER,
            &label,
            body(font_size::TINY),
            if active {
                palette.primary
            } else {
                palette.fg_dim
            },
        );

        // Divider between tabs.
        if index > 0 {
            paint::vline(
                ui.painter(),
                tab_rect.left(),
                tab_rect.top() + 4.0,
                tab_rect.bottom() - 4.0,
                palette.border,
            );
        }

        if response.clicked() {
            switch_to = Some(*view);
        }
    }

    if let Some(view) = switch_to {
        app.view = view;
        app.open_playlist = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_bar_divides_evenly() {
        // Every view must be reachable — a zero-width tab would be invisible.
        let width = 440.0;
        let tab_width = (width / View::ALL.len() as f32).floor();
        assert!(tab_width > 60.0, "tabs too narrow to read: {tab_width}");
    }
}
