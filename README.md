# Sonus

Free, local-first music player in Rust. Streams audio from YouTube Music via
yt-dlp, with a retro pixel-art desktop GUI. No ads, no accounts, no cost.

## Features

- **YouTube Music search** — music only, no vlogs or random videos
- **Recommendation radio** — keeps playing after your queue ends
- **Native desktop GUI** — egui + eframe, pixel-art styling, no browser engine
- **8 built-in themes** — Winamp Green (default), Osaka Jade, Amber CRT, C64,
  Midnight, Tokyo Night, Solarized Dark, Gruvbox — plus custom TOML themes
- **10-band equalizer** — with presets, runs in the audio pipeline
- **Library** — SQLite-backed tracks, playlists, favourites, play history
- **Seek anywhere** — click or drag the progress bar
- **Cross-platform** — Linux, Windows, macOS
- **Low memory** — streams instead of hoarding; audio is never fully downloaded

## Requirements

Sonus shells out to two external tools. Both must be on `PATH`:

- **[yt-dlp](https://github.com/yt-dlp/yt-dlp)** — resolves and streams audio
- **[ffmpeg](https://ffmpeg.org/)** — decodes to raw PCM

```bash
# Arch
sudo pacman -S yt-dlp ffmpeg

# Debian / Ubuntu
sudo apt install yt-dlp ffmpeg

# Fedora
sudo dnf install yt-dlp ffmpeg

# macOS
brew install yt-dlp ffmpeg

# Windows
winget install yt-dlp.yt-dlp
winget install Gyan.FFmpeg
```

Run `sonus paths` to check both are found. Custom locations can be set with
`SONUS_YTDLP_PATH` and `SONUS_FFMPEG_PATH`.

## Build

```bash
cargo build --release
./target/release/sonus
```

## Usage

```bash
sonus                    # Launch the GUI (default)
sonus search "query"     # Search from the terminal and print results
sonus radio <video-id>   # Print YouTube's recommendations for a track
sonus themes             # List available themes
sonus paths              # Show data/cache/theme directories, check dependencies
```

### Keyboard shortcuts

| Key | Action |
|-----|--------|
| `Space` | Play / pause |
| `←` / `→` | Seek 5 seconds |
| `Shift` + `←` / `→` | Seek 30 seconds |
| `↑` / `↓` | Volume |
| `N` / `P` | Next / previous track |
| `/` | Focus the search box |
| `Ctrl` + `↑` / `↓` | Move through search results |
| `Enter` | Play the highlighted result |
| `L` | Like the current track |
| `S` / `R` | Shuffle / cycle repeat |
| `E` / `T` | Toggle equalizer / cycle theme |
| `1` – `4` | Switch view |
| `Esc` | Close panels |

### GUI layout

The window is compact and frameless, laid out like a piece of hardware:

- **Title bar** — app name, queue info, EQ / theme / help toggles, window controls
- **Display** — now-playing marquee, status, seven-segment clock, live spectrum
- **Transport** — prev, play/pause, stop, next, shuffle, repeat, like, volume
- **Seek bar** — click or drag to jump anywhere in the track
- **Tabs** — Search, Queue, Lists (playlists), Library (liked + recent)
- **Status bar** — notices, queue time remaining, keyboard hints

Right-click a row for a context menu (play, queue, like, remove, reorder).

## Custom Themes

Drop a `.toml` file into the themes directory:

- Linux: `~/.config/sonus/themes/`
- Windows: `%APPDATA%\sonus\themes\`
- macOS: `~/Library/Application Support/sonus/themes/`

Eight colours are required; the rest are derived automatically:

```toml
name = "My Custom Theme"
bg = "#1a1e1a"        # window background
panel = "#232b30"     # panel face
fg = "#d4e4d4"        # primary text
primary = "#50fa7b"   # accents, active track, sliders
secondary = "#6272a4" # dimmed text and labels
accent = "#ffb86c"    # highlights, peaks
border = "#44475a"    # 1px panel borders
highlight = "#44475a" # selected row background
```

Optional overrides, if the derived values don't suit:

```toml
panel_light = "#3a454c"  # bevel highlight (derived from panel)
panel_dark  = "#12181c"  # bevel shadow    (derived from panel)
fg_dim      = "#6272a4"  # dim text        (derived from secondary)
primary_dim = "#1a7a3d"  # dim accent      (derived from primary)
lcd_bg      = "#0c1210"  # display backdrop
lcd_fg      = "#2ee86b"  # lit segments
lcd_dim     = "#0f4a24"  # unlit segments
danger      = "#e5484d"  # errors
highlight_fg = "#d4e4d4" # text on selected rows
```

Run `sonus themes` to confirm it loaded. A malformed file is reported with its
filename rather than silently ignored.

## Project Layout

```
crates/
  sonus-core/     Shared types: Track, Queue, Playlist, Theme
  sonus-ytdlp/    yt-dlp subprocess: search, radio, metadata, streaming
  sonus-audio/    Playback engine (rodio), ring buffer, EQ, spectrum
  sonus-library/  SQLite library: tracks, playlists, history, favourites
  sonus-cache/    LRU file cache for audio data
  sonus-gui/      GUI: pixel-art widgets, views, theme manager
  sonus/          Binary entry point and CLI
```

### How playback works

The GUI owns the queue and decides what plays next. `AudioEngine` is not `Send`
— rodio's output stream holds platform-specific handles — so it lives on a
dedicated audio thread and the GUI talks to it over a channel.

Audio flows `yt-dlp → ffmpeg → ring buffer → rodio`. A pre-roll gate holds the
sink until enough audio has buffered, so the clock never runs ahead of the
sound while a stream is still being prepared.

## Development

```bash
cargo check              # Fast type-check
cargo clippy             # Lints
cargo test               # Run tests
cargo fmt                # Format code
cargo build --release    # Optimized build
```

## License

MIT
