# 0063: Theme tokens per layout

- Status: accepted (2026-10-23, task P7-01); amended below
- Date: 2026-10-23
- Task: P7-01

## Context

The app uses egui's default look. The Panel Classic and Panel Modern layouts each
have a distinct palette and set of metrics: chrome colours, menu
highlight, pressed tool state, contextual tab-set colours, group and
toolbar sizes. The data has to live somewhere the widgets can read
without hard-coding numbers.

## Decision

- `ui-spec/themes/<id>.yaml`, where `<id>` is the layout's `theme`
  field. Each file has:
  - `fonts`: role to family, size and transform;
  - `colors`: token to sRGB hex;
  - `metrics`: token to logical pixels, or a small object;
  - for Panel Modern, `contextual_tab_sets`: tab set to colour family.
- Tokens are dotted names, for example `menu.highlight.fill`,
  `button.pressed.fill`, `group.caption.text` or `stack.row.height`.
- A widget asks for a token by name. A missing token falls back to an
  egui default and logs one warning, so a theme file can never crash
  the app.
- `crates/ui/src/theme.rs` maps the tokens it understands onto egui
  `Style` and `Visuals`: panel fills, widget fills and strokes, text
  colours and spacing. The widgets read the rest (command bar and toolbar
  metrics) directly.
- The UI font is Selawik, already bundled in `crates/text/fonts`
  (sources 12). Panel Modern tab captions are drawn in upper case, set by
  `fonts.tab.transform`.
- The Fluent-data layouts (2016, 365) use the Panel Modern theme.
- Tokens marked `# provisional` in the YAML are design choices still to
  confirm. They can change without an ADR.

## Consequences

- Colours and sizes can be tuned without touching widget code.
- Golden PNG tests (ADR 0065) pin the look. A deliberate theme change
  regenerates them.
- A dark theme later is one more file plus a setting.

## Alternatives considered

- **Hard-code the palette in the widgets.** Rejected: two layouts would
  duplicate every colour, and tuning needs a rebuild.
- **egui's `Visuals` only.** Rejected: `Visuals` has no notion of
  command bar metrics or contextual tab colours.

## Amendments (task P7-01)

- The files are `ui-spec/themes/panel-modern.yaml` and
  `ui-spec/themes/panel-classic.yaml` (ids `panel-modern` and
  `panel-classic`). Panel (2016 data) and Panel (365 data) use the
  Panel Modern theme.
- The command-bar metrics are `command_bar.height` and
  `command_bar.padding_top`.
- The seam is `crates/ui/src/theme.rs`; `theme::apply` is an
  interface stub until P7-05 implements it.

## Amendments (task P7-18, the design pass)

- One design language for both layouts (`docs/ui/design.md`, approved
  by the user on 2026-10-26). `ui-spec/themes/base.yaml` holds every
  colour and font role and the shared metrics; `panel-modern.yaml` and
  `panel-classic.yaml` hold only what their structure adds. The loader
  derives each layout theme from the base and refuses a layout file
  that sets a colour or a font.
- The palette is neutral (a light grey chrome, white content) with one
  accent, `#1f5bc4`. Every text and icon pair meets WCAG 2.2 AA; the
  ratios are in the design document.
- The font roles are a type scale (`caption`, `ui`, `strong`, `lead`,
  `title`, `display`) the widgets ask for by name. Panel Modern tab
  captions are no longer upper-cased; the `transform` field is unused.
- A missing token falls back to its base-theme value, not to an egui
  default.
- Disabled icons are tinted `chrome.text_disabled` instead of drawn at
  40% alpha (ADR 0062 said 40% alpha).
- A test fails when a widget paints a colour outside the theme tokens
  (`crates/ui/tests/phase7/palette.rs`).
