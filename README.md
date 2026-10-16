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

## Install

Every release publishes prebuilt packages to
[GitHub Releases](https://github.com/halowahyudi/sonus-music/releases).

### Windows

1. Download **sonus-<version>-setup.exe** and run it — no admin rights, no
   command line
2. On first launch Sonus offers to download yt-dlp and ffmpeg for you

Prefer no installer? Grab **sonus-windows-x86_64.zip** and run `sonus.exe`
from any folder.

### Debian / Ubuntu

```bash
sudo apt install ./sonus_<version>-1_amd64.deb
```

### Fedora / openSUSE

```bash
sudo dnf install ./sonus-<version>-1.x86_64.rpm
```

### Arch Linux

The AUR package is prepared (`packaging/aur/PKGBUILD`) but not published yet —
new AUR account registration is currently closed. Until it's live, build from
source:

```bash
git clone https://github.com/halowahyudi/sonus-music
cd sonus-music
cargo build --release --locked
install -Dm755 target/release/sonus ~/.local/bin/sonus
```

### macOS

Download **sonus-macos-arm64.tar.gz**, unpack `Sonus.app`, and drag it into
`/Applications`. It's unsigned, so the first launch needs right-click → Open.

### Any binary tarball

The Linux and macOS archives contain a single `sonus` binary:

```bash
tar -xzf sonus-linux-x86_64.tar.gz
install -Dm755 sonus ~/.local/bin/sonus
```

### From source

```bash
cargo install --path crates/sonus --locked
```

## Requirements

Sonus streams audio through two external tools: **yt-dlp** (resolves and
fetches the stream) and **ffmpeg** (decodes to raw PCM).

You do **not** have to install them — the GUI offers to download both into
your user data directory on first launch. If you prefer your package manager:

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

They're found in this order:

1. `SONUS_YTDLP_PATH` / `SONUS_FFMPEG_PATH` env overrides
2. A copy Sonus downloaded itself (`sonus deps install`)
3. Anything already on `PATH`

If a system copy is already installed, Sonus prefers it. To manage its own
downloaded copies instead:

```bash
sonus deps status     # where each tool lives, and whether it's up to date
sonus deps install    # download anything missing
sonus deps update     # force re-download of the latest versions
```

Run `sonus paths` to see all the directories Sonus uses.

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
sonus deps status        # Where yt-dlp/ffmpeg live, and whether updates exist
sonus deps install       # Download any missing tools
sonus deps update        # Force re-download of the latest tools
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

In **Search**, a click plays the track now and starts a radio tail from it.
Use the context menu (right-click) for finer control:

- **Play next** — insert right after the current track
- **Add to queue** — append without disrupting playback
- **Start radio** — reboot playback from this track with its recommendations

Radio tracks in the queue carry an `AUTO` badge — they're disposable filler
whose job is to keep the music going after your picks end. If the network
radio ever comes back empty or fails, Sonus falls back to your own listening
history (same artist, then most-played, then old favourites) so playback
keeps going even offline.

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
  sonus-deps/     Downloads/verifies/updates the yt-dlp & ffmpeg binaries
  sonus-ytdlp/    yt-dlp subprocess: search, radio, metadata, streaming
  sonus-audio/    Playback engine (rodio), ring buffer, EQ, spectrum
  sonus-library/  SQLite library: tracks, playlists, history, favourites
  sonus-cache/    LRU file cache for audio data
  sonus-gui/      GUI: pixel-art widgets, views, theme manager
  sonus/          Binary entry point and CLI

packaging/
  assets/         Icons + .desktop (single source for window, installers)
  windows/        Inno Setup script + portable build
  macos/          .app bundle Info.plist
  aur/            AUR PKGBUILD
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
