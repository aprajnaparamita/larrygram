# 0067: Brand assets, their licence and image loading

- Status: accepted
- Date: 2026-10-23
- Task: P7-01

## Context

The welcome wizard and the About window need artwork: the app
picture, the two layout pictures, the app icon and the photograph of
Larry Jeffus. The artwork is not open-licensed; it is made for this
app and used with permission. AGENTS.md section 4 requires that every
committed asset has an `ATTRIBUTION.md` row in the same commit and
that no third-party product artwork enters the repository. The SVGs
and the PNG also have to load in egui on desktop and on wasm.

## Decision

### Files and licence

The assets live in `crates/ui/assets/brand/`:

| File | Use |
|---|---|
| `about_screen.svg` | The welcome wizard's page-1 picture (16:10) |
| `panel-layout.svg` | The Panel Modern button picture on page 2 |
| `classic-layout.svg` | The Panel Classic button picture on page 2 |
| `icon.svg` | The window icon (desktop) and the favicon (web) |
| `larry.png` | The About window picture |

- The four SVGs: © 2026 Janet Jeffus, made in VectorCraft, all rights
  reserved, used in Larrygram with permission.
- `larry.png`: used with Larry Jeffus's permission, all rights
  reserved.
- They are **not** covered by the MIT licence of the repository.
  `ATTRIBUTION.md` has one row per file with these terms, and the
  `README.md` licence note says so and tells forks to replace the
  files before shipping.
- No file is Microsoft artwork, and none of it is traced or redrawn
  from another product. The layout pictures show Larrygram's own
  layouts.

### App icon and favicon

- `icon.svg` becomes the desktop window icon (through the eframe/winit
  icon API, at the sizes the platform asks for) and the web favicon
  (a `<link rel="icon">` in the web app's page).
- Task P7-16 wires both; the asset stays a single SVG source.

### Loading the images

The proposed loader is the `egui_extras` image loaders, with the
`svg` feature (SVG through `resvg`) and the `image` feature (PNG
through the `image` crate). Licence check (task P7-01, `cargo deny`
over the closure in a scratch workspace):

| Crate | Licence |
|---|---|
| `egui_extras` 0.37 | MIT OR Apache-2.0 |
| `resvg` / `usvg` (0.45.1, the `egui_extras` pin) | MIT OR Apache-2.0 |
| `tiny-skia`, `tiny-skia-path` | MIT OR Apache-2.0 |
| `image`, `png`, `miniz_oxide`, `flate2`, `image-webp`, `zune-*` | MIT OR Apache-2.0 / Zlib / BSD |
| `fontdb`, `svgtypes`, `kurbo` | MIT OR Apache-2.0 |

The closure passes the repository's `deny.toml` licence allow-list
with no MPL crate, so the "only `larrygram-vsd-binary` is MPL" rule
(ADR 0003) holds. The master added the dependency at the boundary
(step R5): `egui_extras` with the `image` and `svg` features and no
default features (no file or HTTP loader), and `image` with only the
`png` decoder. `larrygram_ui::install_image_loaders` installs them, and
both apps call it before the first frame. The repository `cargo deny`
step checks the real closure. If `egui_extras` proves too heavy for wasm, the fallback is
our own thin SVG reader over `tiny-skia` (already in the tree through
`larrygram-render`); that is an implementation change, not a licence
one.

`currentColor` handling and the icon rasterisation of ADR 0062 cover
the icon set; the brand SVGs are drawn as-is.

## Consequences

- The artwork ships in every build. It is small (SVGs and one PNG).
- Forks and downstream distributors must replace the five files; the
  README says so in its licence note.
- A dark theme later may need dark variants of the pictures; that is a
  new asset decision with its own row.

## Alternatives considered

- **CC licensing the artwork.** Rejected: the artist keeps all rights
  and grants the project permission; the `ATTRIBUTION.md` rows record
  exactly that.
- **Drawing the pictures in egui at run time.** Rejected: they are
  artwork of their own, not widgets.
- **PNG only, rasterised at build time.** Rejected: SVGs stay sharp at
  every scale factor and at the zoom levels of HiDPI screens.

## Amendment 2026-11-03: the app icon is MIT

The artist, Janet Jeffus, releases `icon.svg` under the MIT licence of
the repository, so packages and distributions can ship the app icon
without a separate permission. It is still © 2026 Janet Jeffus. Forks
may keep it.

`about_screen.svg`, `panel-layout.svg`, `classic-layout.svg` and
`larry.png` keep the terms above: all rights reserved, not covered by
the MIT licence, replaced by any fork. The three goldens that render
them (`about.png`, `wizard-welcome.png`, `wizard-layout.png`) keep the
same restriction.
