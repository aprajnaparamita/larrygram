# Building the macOS `.dmg`

How to build, sign and test the macOS app bundle and disk image
(tasks R7 and R9). The build runs on macOS on Apple silicon; there is
no cross-building. The same command builds the package on the
`macos-14` CI runner (see "CI" below).

## What gets built

`cargo xtask dist mac` builds a release `Larrygram.app` and a
`Larrygram-<version>-macos-arm64.dmg` under `<target dir>/dist/` (the
cargo target directory's `dist/`, never the repository):

- `Contents/MacOS/larrygram` — the release binary of `apps/larrygram`,
  for `aarch64-apple-darwin` (Apple silicon only; no universal binary
  for now);
- `Contents/MacOS/larrygram-cli` — the command line tool; every
  package carries both binaries (task R9 decisions);
- `Contents/Info.plist` — rendered from
  `apps/larrygram/macos/Info.plist`, with the bundle id
  `org.maitrigroup.Larrygram`, the version from `Cargo.toml`, the
  minimum macOS 11.0, high-resolution support and the `.vsdx`, `.vsd`,
  `.vss`, `.vst` document types;
- `Contents/Resources/Larrygram.icns` — rendered from
  `crates/ui/assets/brand/icon.svg` at the standard icon sizes by our
  own rasteriser (the same resvg the app uses for its window icon) and
  packed with the system `iconutil`;
- `Contents/Resources/` — the licence files the app ships with:
  `LICENSE`, `NOTICE`, `ATTRIBUTION.md` and the OFL texts of the
  bundled fonts (`fonts/Carlito-OFL.txt`, `fonts/Selawik-OFL.txt`);
- the `.dmg` — the app and an `Applications` link, packed with the
  system `hdiutil` (compressed UDZO).

## Prerequisites

- macOS on Apple silicon. The build fails on anything else.
- The Xcode Command Line Tools (`iconutil`, `hdiutil`, `codesign`):
  `xcode-select --install`.
- The Rust toolchain the workspace pins.
- Enough free disk space for the release build and the image: the
  output goes to `<target dir>/dist/` (task R9 goal 1), which on this
  machine's worktrees is the external drive.

## Build

```sh
cargo xtask dist mac
```

The command builds the release binaries, assembles the bundle, signs
it ad-hoc and packs the `.dmg`. It prints the two artifact paths at
the end. Rebuild after a version bump to get a new
`Larrygram-<version>-macos-arm64.dmg`; the `.app` directory is
replaced in place.

## Signing

The bundle is signed **ad-hoc** (`codesign --sign -`): no Developer ID,
no notarisation (the user's decision, 2026-10-25). macOS runs such an
app on this Mac and on the testers' Macs, but Gatekeeper has no
Developer ID to check, so **the first launch needs an explicit OK**:

- Right-click (or Control-click) the app, then **Open**, then **Open**
  in the dialog. Or:
- Try to open it once, then go to **System Settings > Privacy &
  Security**, scroll down and click **Open Anyway**.

After that the app opens normally. Replacing the app with a new build
may ask again.

## Opening documents (task R8)

Three routes open a document in the running app; all of them end in the
same `file.open` command as the File > Open menu item, and a path that
does not open is the same status-line error as a failed dialog pick:

- `Larrygram.app/Contents/MacOS/larrygram <path>...` (or any other way
  of starting the binary with paths as arguments): each path opens at
  startup, in order, and the last one is the active document.
- `open -a Larrygram.app <path>...` in Terminal: the same Apple event a
  **Finder double-click** sends, at launch or while the app runs.
- Dropping files on the Larrygram **Dock icon**: the same event with
  several URLs.

Opening a second document never replaces the first one and never asks
about unsaved changes: every file becomes its own document session and
the last opened is active, exactly as `file.open` from the menu does.

The check the maintainer runs after each `cargo xtask dist mac` build
that touches opening:

```sh
D=<target dir>/dist/Larrygram.app
open "$D"                                    # start the app
open -a "$D" /path/to/drawing.vsdx           # Finder-style open
"$D/Contents/MacOS/larrygram" /path/to/drawing.vsdx   # command line
open -a "$D" /path/to/one.vsdx /path/to/two.vsd       # several files
```

Each one must show the drawing on the page and the file's name in the
title strip, with no dialog and no crash. The Dock-icon drop is the
same event as `open -a`; a real Finder double-click additionally needs
the app to be the LaunchServices handler for the extension, which the
testers' check list covers on the mounted `.dmg`.

## Testers' check list

See also [testers.md](testers.md) for the download, the checksum
check and the first-launch note. Do this on the mounted `.dmg`,
before copying the app anywhere:

- [ ] Mount `Larrygram-<version>-macos-arm64.dmg` (double-click).
- [ ] Open `Larrygram.app` from the mounted image (right-click, Open,
      the first time; see "Signing" above).
- [ ] Open a `.vsdx` drawing by double-clicking it in Finder: the app
      comes to the front with the drawing on the page. (If another app
      owns the `.vsdx` extension on your Mac, choose **Open With >
      Larrygram** once and **Always**; a double-click opens the
      extension's default app, whatever that is.)
- [ ] Open a `.vsd` drawing by double-clicking it: the drawing opens,
      and the status line may list import warnings (the binary format
      is read-only import).
- [ ] Open a drawing with `open -a Larrygram.app <file>` from Terminal,
      and one by dropping it on the Larrygram icon in the Dock: both
      open the same way (task R8).
- [ ] Make an edit, save, close the window and reopen the file: the
      edit is still there.
- [ ] Quit with Cmd+Q: the app exits cleanly, no dialog, no crash.
- [ ] (optional) Drag the app to `Applications` from the image and
      repeat the open steps from there.

Report any step that fails with the file used and what you saw.

## CI

Two workflows build and check the macOS app (task R9 goals 4 and 5):

- `.github/workflows/ci.yml` runs the quality gate on `macos-latest`
  for every push to `main` and every pull request (and on request,
  `workflow_dispatch`). It is the same `cargo xtask gate` as local.
- `.github/workflows/release.yml` runs `cargo xtask dist mac` on the
  `macos-14` runner (Apple silicon, the only supported target) for
  every pushed `v*` tag and uploads the `.dmg` to the GitHub
  pre-release, with the Windows and Linux packages and `SHA256SUMS`.

The CI build is unsigned like the local one: ad-hoc, no Developer ID,
no notarisation. Nothing on GitHub is pushed or tagged by the build;
the maintainer pushes the tag by hand.
