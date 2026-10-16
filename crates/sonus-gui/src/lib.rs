pub mod app;
pub mod audio;
pub mod pixel;
pub mod task;
pub mod theme;
pub mod ui;

pub use app::{Notice, SonusApp, View};
pub use audio::{AudioController, AudioStatus, Transport};
pub use theme::{themes_dir, ThemeManager};

/// The 64×64 icon embedded into the window/taskbar.
///
/// The PNG lives in `packaging/assets` — the same source the installers use —
/// so the window icon and the launcher icon can never drift apart.
const WINDOW_ICON: &[u8] = include_bytes!("../../../packaging/assets/png/sonus-64x64.png");

/// Decode the window icon into the format eframe expects.
///
/// Falls back to `None` (no icon) if the embedded PNG can't be decoded — a
/// broken icon shouldn't stop the player from starting.
pub fn window_icon() -> Option<std::sync::Arc<egui::IconData>> {
    let image = image::load_from_memory(WINDOW_ICON).ok()?.to_rgba8();
    let (width, height) = image.dimensions();
    Some(std::sync::Arc::new(egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_icon_decodes_to_a_square_rgba() {
        let icon = window_icon().expect("embedded icon must decode");
        assert_eq!(icon.width, 64);
        assert_eq!(icon.height, 64);
        assert_eq!(icon.rgba.len(), 64 * 64 * 4);
    }
}
