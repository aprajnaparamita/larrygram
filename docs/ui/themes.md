# Themes

ADR 0063 records the decision, and `docs/ui/design.md` the design
language the values carry. There are three files:

- `ui-spec/themes/base.yaml`: every colour, every font role and the
  shared metrics;
- `ui-spec/themes/panel-modern.yaml` and
  `ui-spec/themes/panel-classic.yaml`: only what a layout's structure
  adds (the contextual tab sets, a few heights and widths).

The loader derives each layout theme from the base: the base's fonts
and colours, the base's metrics with the layout's over them, and the
layout's id, name and contextual tab sets. A layout file that sets a
colour or a font is a load error, so both layouts always share one
palette and one type scale. Panel (2016 data) and Panel (365 data) use
the Panel Modern theme.

## Structure

```yaml
schema_version: 1
id: base                                    # or panel-modern, panel-classic
name: Larrygram
fonts:   {role: {family, size_px}}          # base only
colors:  {token: '#rrggbb' | '#rrggbbaa'}   # base only
metrics: {token: px | {width, height, ...}}
contextual_tab_sets: {set: colour family}   # Panel Modern
```

## Mapping to egui

`crates/ui/src/theme.rs` `visuals`:

| Token | egui |
|---|---|
| `chrome.background` | `panel_fill`, `faint_bg_color`, `code_bg_color`, the non-interactive widget fill |
| `chrome.text` | `override_text_color` and every widget state's `fg_stroke` |
| `chrome.text_muted` | `weak_text_color` (hints) |
| `chrome.border` | the non-interactive `bg_stroke` (separators) |
| `button.fill` | the inactive `weak_bg_fill`: buttons are flat at rest |
| `field.background` | `extreme_bg_color` (text edits) |
| `button.active.fill` | also the inactive `bg_fill`: the box of a check box or radio button at rest |
| `button.hover.fill`, `button.hover.border` | `widgets.hovered` |
| `button.active.fill`, `focus`, `focus.width` | `widgets.active` (held down or focused): the fill and the accent edge |
| `button.pressed.fill`, `accent` | `selection`: a selected button's fill and label |
| `button.pressed.border` | the `widgets.open` edge (an open combo or menu button) |
| `menu.background`, `menu.border` | `window_fill` and `window_stroke` (menus, popups, tooltips, windows) |
| `shadow` | the window and popup shadow colour |
| `accent` | `hyperlink_color` and the text cursor |
| `icon.orange`, `icon.red` | `warn_fg_color`, `error_fg_color` |
| `radius.control`, `radius.popup` | every widget state's corners; window and menu corners |
| `canvas.*`, `page.*`, `ruler.*` | read by the canvas (`crates/ui/src/canvas.rs`): the off-page area, the page, the rulers, the grid, the selection and handles, the snap guides |
| `splitter.hover` | the line on a pane border under the pointer (`crates/ui/src/splitter.rs`) |
| everything else | read by the widget that draws it (`theme::color_at`, `theme::metric_at`) |

`theme::raise_controls` (`button.raised.fill`, `control.border`) and
`theme::accent_controls` (`accent`, `accent.hover`, `accent.pressed`,
`accent.text`) restyle the buttons of one ui: framed standard buttons
for dialogs, accent default buttons.

A missing token falls back to its value in the embedded base theme and
warns once; a name the base lacks falls back to grey.

## Fonts

- Every role is Selawik, bundled in `crates/text/fonts` under the OFL.
  The 12 px `ui` size matches the 9 pt UI text of desktop apps at
  96 DPI.
- The roles are the type scale of `docs/ui/design.md`: `caption`,
  `ui`, `strong`, `lead`, `title` and `display`. Each becomes a named
  egui text style; `ui` is also the body and button style, `caption`
  the small style and `title` the heading style. A widget asks for a
  role with `theme::font_at` (or `theme::scaled_font` for text that
  grows with its window).
- Tab captions are drawn in title case, as the data has them.

## Tests

- `crates/ui-spec/tests/theme_tokens.rs`: every token a widget reads is
  in both themes, every theme token is read by a widget, and both
  layouts share the base palette and fonts.
- `crates/ui/tests/phase7/palette.rs`: no widget paints a colour that
  is not a theme token.
