# Building the Linux package

How to build the Linux alpha package (task R9). The build runs on a
64-bit x86 Linux machine; there is no cross-building. The CI build is
the same command on the `ubuntu-22.04` runner
(`.github/workflows/release.yml`).

## What gets built

`cargo xtask dist linux` builds the release binaries and writes
`Larrygram-<version>-linux-x64.tar.gz` under `<target dir>/dist/` (the
cargo target directory, never the repository). Inside the archive,
under `Larrygram-<version>-linux-x64/`:

- `larrygram`, `larrygram-cli` — the desktop app and the command line
  tool;
- `larrygram.desktop` — the desktop entry
  (`apps/larrygram/linux/larrygram.desktop`), which registers the
  Visio drawing types
  (`application/vnd.ms-visio.drawing.main+xml`,
  `application/vnd.visio`);
- `larrygram.png` — the 256 px icon, rasterised from
  `crates/ui/assets/brand/icon.svg` at package time (task R9 goal 3);
- `INSTALL.txt` — where to copy the desktop entry and the icon;
- `LICENSE`, `NOTICE`, `ATTRIBUTION.md` and the bundled font
  licences (`fonts/Carlito-OFL.txt`, `fonts/Selawik-OFL.txt`).

The archive is written by the pure-Rust `tar` and `flate2` crates: no
`tar` or `7z` binaries are involved (task R9 goal 1). There is no
installer (no `.deb`, AppImage or Flatpak) and no signing (task R9
decisions).

## Prerequisites

- 64-bit x86 Linux (x86-64). The command refuses any other host.
- The Rust toolchain the workspace pins (`rust-toolchain.toml`).

## Build

```sh
cargo xtask dist linux
```

Then unpack the archive and follow its `INSTALL.txt`: the short
version for one user is

```sh
tar xzf Larrygram-<version>-linux-x64.tar.gz
cd Larrygram-<version>-linux-x64
mkdir -p ~/.local/bin ~/.local/share/applications ~/.local/share/icons/hicolor/256x256/apps
cp larrygram larrygram-cli ~/.local/bin/
cp larrygram.desktop ~/.local/share/applications/
cp larrygram.png ~/.local/share/icons/hicolor/256x256/apps/
command -v update-desktop-database >/dev/null && update-desktop-database ~/.local/share/applications
```

The last line refreshes the desktop's MIME cache and is skipped
quietly where the tool is missing. The desktop entry starts
`larrygram` from your `PATH`, so `~/.local/bin` must be on the `PATH`
of the desktop session. Optional, to make Larrygram the default for
`.vsdx` and `.vsd`:

```sh
xdg-mime default larrygram.desktop application/vnd.ms-visio.drawing.main+xml
xdg-mime default larrygram.desktop application/vnd.visio
```

Opening a drawing from the file manager or the command line passes
the path to the app (task R8).

## Compatibility

The binaries are built on the Ubuntu 22.04 runner, so they need
glibc 2.35 or newer. That is the oldest Ubuntu GitHub still offers
(task R9 decision): older glibc, wider reach.

## CI

`.github/workflows/release.yml` runs the same command on the
`ubuntu-22.04` runner for every `v*` tag and uploads the archive to
the GitHub pre-release. The regular CI
(`.github/workflows/ci.yml`) runs the quality gate on `ubuntu-latest`
with Mesa's software Vulkan installed, so the egui_kittest wgpu tests
have an adapter (task R9 goal 4).
