# Building the macOS `.dmg`

How to build, sign and test the macOS app bundle and disk image
(task R7). The build is local: there is no hosted CI, and this is the
machine the artifacts are built on.

## What gets built

`cargo xtask dist mac` builds a release `Larrygram.app` and a
`Larrygram-<version>.dmg` under `/Volumes/Dara/larrygram/dist/` (the
external drive, never the repository):

- `Contents/MacOS/larrygram` — the release binary of `apps/larrygram`,
  for `aarch64-apple-darwin` (Apple silicon only; no universal binary
  for now);
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
- `/Volumes/Dara` mounted: the output directory lives there.

## Build

```sh
cargo xtask dist mac
```

The command builds the release binary, assembles the bundle, signs it
ad-hoc and packs the `.dmg`. It prints the two artifact paths at the
end. Rebuild after a version bump to get a new `Larrygram-<version>.dmg`;
the `.app` directory is replaced in place.

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

## Testers' check list

Do this on the mounted `.dmg`, before copying the app anywhere:

- [ ] Mount `Larrygram-<version>.dmg` (double-click).
- [ ] Open `Larrygram.app` from the mounted image (right-click, Open,
      the first time; see "Signing" above).
- [ ] Open a `.vsdx` drawing by double-clicking it in Finder: the app
      comes to the front with the drawing on the page.
- [ ] Open a `.vsd` drawing by double-clicking it: the drawing opens,
      and the status line may list import warnings (the binary format
      is read-only import).
- [ ] Make an edit, save, close the window and reopen the file: the
      edit is still there.
- [ ] Quit with Cmd+Q: the app exits cleanly, no dialog, no crash.
- [ ] (optional) Drag the app to `Applications` from the image and
      repeat the open steps from there.

Report any step that fails with the file used and what you saw.
