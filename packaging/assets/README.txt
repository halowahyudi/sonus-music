Sonus icon assets
=================

- png/sonus.png: 1024x1024 master PNG
- png/sonus-<size>x<size>.png: Linux icon sizes (16, 22, 24, 32, 48, 64, 96, 128, 256, 512)
- sonus.ico: Windows multi-size icon (used by the Inno Setup installer)
- sonus.desktop: Linux desktop entry with StartupWMClass=sonus

These are consumed by the packaging scripts in ../ (Inno Setup for Windows,
cargo-deb/cargo-generate-rpm and the AUR PKGBUILD for Linux). The ico also
becomes the eframe window icon at build time via `sonus-gui`.

Linux install example (manual):
  mkdir -p ~/.local/share/icons/hicolor/512x512/apps
  cp png/sonus-512x512.png ~/.local/share/icons/hicolor/512x512/apps/sonus.png
  cp sonus.desktop ~/.local/share/applications/
  update-desktop-database ~/.local/share/applications

If your app's executable name differs from `sonus`, edit Exec= in sonus.desktop.
