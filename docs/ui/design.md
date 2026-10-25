# Design language

Status: **accepted** (task P7-18; the user approved the proposal and
its five choices on 2026-10-26). The palette, type and metrics live in
`ui-spec/themes/base.yaml`.

Larrygram has two layouts. Panel Modern has a command bar with tabs.
Panel Classic has a menu bar with docked toolbars. Each layout keeps
its own structure, but both use one design language: the same palette,
type, spacing, corner radii, borders and state colours. A user who
switches layout sees the same app arranged differently, not a
different app.

## Principles

- **Neutral surfaces.** The chrome is light grey, and content sits on
  white. Colour is kept for meaning.
- **One accent.** One calm blue marks the File button, the selected
  tab, the focus ring, links and the default button of a dialog. No
  other surface is filled with it.
- **State is shown the same way everywhere.** Hover is a neutral tint,
  a mouse press is a darker neutral, and a selected or latched item
  is an accent tint with an accent edge. A menu item, a toolbar
  button, a command bar button and a page tab all follow this rule.
- **Contrast first.** Text and icons meet WCAG 2.2 AA on every surface
  they appear on (the table below).
- **Quiet structure.** Lines are 1 px and light. A surface is set off
  by a change of fill, not by a heavy border.

## Palette

The palette has semantic names. The widget tokens in
`ui-spec/themes/*.yaml` (`menu.highlight.fill`, `status_bar.background`
and so on) each take one of these values; the mapping is below.

### Neutrals

| Name | Hex | Use |
|---|---|---|
| `surface` | `#f5f5f5` | The chrome: title bar, tab strip, command bar, menu bar, toolbar rows, status bar, page-tab strip, rulers, File view navigation |
| `surface.raised` | `#ffffff` | Content and popups: panes, menus, dialogs, the wizard, tooltips, fields, the File view page |
| `border` | `#d6d6d6` | Dividers, group separators, pane and popup edges |
| `border.strong` | `#8a8a8a` | The edges of fields, check boxes and combo boxes (they must be seen) |
| `text` | `#1f1f1f` | Body text and labels |
| `text.muted` | `#5f5f5f` | Group captions, shortcut keys in menus, status-bar fields, hints |
| `text.disabled` | `#a3a3a3` | Disabled labels (exempt from AA; see below) |
| `icon` | `#424242` | Neutral icons |

### Accent

| Name | Hex | Use |
|---|---|---|
| `accent` | `#1f5bc4` | File button, selected tab caption and underline, focus ring, links, default dialog button, active page tab |
| `accent.hover` | `#1a4ea8` | An accent-filled button under the pointer |
| `accent.pressed` | `#164290` | An accent-filled button held down |
| `accent.text` | `#ffffff` | Text and icons on an accent fill |

### States

| Name | Hex | Use |
|---|---|---|
| `hover` | `#ebebeb` | Any button, menu item, tab or list row under the pointer |
| `pressed` | `#dedede` | The same, while the mouse button is down |
| `selected` | `#e3ebf8` | A latched tool (Pointer Tool), an on toggle, the open menu title, the selected File view entry, the selected row of a list |
| `selected.border` | `#5b82cc` | The edge of a selected item |
| `focus` | `#1f5bc4` | The 2 px keyboard focus ring (the accent) |

### Drawing area

| Name | Hex | Use |
|---|---|---|
| `canvas` | `#e4e4e4` | The off-page area around the page |
| `page` | `#ffffff` | The page |
| `page.border` | `#b8b8b8` | The page edge |
| `page.shadow` | `#d0d0d0` | The drop shadow beside and below the page |
| `ruler` | `#f5f5f5` | The ruler background (the chrome surface) |
| `ruler.ticks` | `#5f5f5f` | Ruler ticks and numbers (the muted text) |

### Icon tints

Our own palette, darkened from the earlier set until every tint meets
3:1 on the chrome and on the selected tint. Blue is the accent, so a
blue icon matches the File button and the focus ring.

| Token | Hex |
|---|---|
| `icon.neutral` | `#424242` |
| `icon.blue` | `#1f5bc4` |
| `icon.green` | `#2f7d32` |
| `icon.orange` | `#b85600` |
| `icon.red` | `#c4314b` |
| `icon.yellow` | `#946c00` |
| `icon.purple` | `#6b4fbb` |

### Contextual tab sets (Panel Modern)

A contextual tab set (Picture Tools, Container Tools) keeps a hue so
that it stands out from the fixed tabs. The hues are muted to sit with
the accent.

| Family | Band | Fill | Text |
|---|---|---|---|
| green | `#2f7d32` | `#eaf4ea` | `#1e6b22` |
| orange | `#d86a00` | `#fdf1e6` | `#a64a00` |

## Contrast (WCAG 2.2 AA)

Normal text needs 4.5:1. Icons and the edges of controls need 3:1
(success criterion 1.4.11). Disabled controls are exempt (1.4.3), but
the disabled grey still reads as a label.

| Pair | Ratio | Needs | |
|---|---|---|---|
| `text` on `surface` | 15.12 | 4.5 | pass |
| `text` on `surface.raised` | 16.48 | 4.5 | pass |
| `text` on `hover` | 13.83 | 4.5 | pass |
| `text` on `pressed` | 12.25 | 4.5 | pass |
| `text` on `selected` | 13.74 | 4.5 | pass |
| `text.muted` on `surface` | 5.86 | 4.5 | pass |
| `text.muted` on `surface.raised` | 6.39 | 4.5 | pass |
| `text.muted` on `canvas` | 5.02 | 4.5 | pass |
| `accent` on `surface` | 5.75 | 4.5 | pass |
| `accent` on `surface.raised` | 6.27 | 4.5 | pass |
| `accent` on `selected` | 5.22 | 4.5 | pass |
| `accent.text` on `accent` | 6.27 | 4.5 | pass |
| `accent.text` on `accent.hover` | 7.78 | 4.5 | pass |
| `icon.neutral` on `selected` | 8.38 | 3 | pass |
| `text.muted` on `hover` | 5.36 | 4.5 | pass |
| `accent` on `hover` | 5.26 | 4.5 | pass |
| `icon.neutral` on `surface` | 9.22 | 3 | pass |
| `icon.neutral` on `hover` | 8.43 | 3 | pass |
| `icon.blue` on `selected` | 5.22 | 3 | pass |
| `icon.green` on `surface` | 4.70 | 3 | pass |
| `icon.orange` on `surface` | 4.41 | 3 | pass |
| `icon.orange` on `selected` | 4.01 | 3 | pass |
| `icon.red` on `surface` | 4.95 | 3 | pass |
| `icon.yellow` on `surface` | 4.37 | 3 | pass |
| `icon.yellow` on `selected` | 3.97 | 3 | pass |
| `icon.purple` on `surface` | 5.57 | 3 | pass |
| green tab-set text on its fill | 5.85 | 4.5 | pass |
| orange tab-set text on its fill | 5.25 | 4.5 | pass |
| `border.strong` (field edge) on `surface.raised` | 3.45 | 3 | pass |
| `focus` ring on `surface` | 5.75 | 3 | pass |
| `selected.border` on `surface` | 3.50 | 3 | pass |
| `selected.border` on `selected` | 3.18 | 3 | pass |
| `text.disabled` on `surface` | 2.31 | — | exempt |
| `text.disabled` on `surface.raised` | 2.52 | — | exempt |

`border` (1.5:1) is used only for dividers that separate surfaces of
the same function. Those dividers are not control edges, so 1.4.11
does not apply. The page edge on the off-page area (1.56:1) is in the
same position: the shadow and the fill change mark the page.

## Type

One family, Selawik (OFL, bundled). Both layouts use the same scale;
the role names are the font roles of `base.yaml`, and a widget asks for
one with `theme::font_at`.

| Role | Face | Size | Use |
|---|---|---|---|
| `caption` | Regular | 11 px | Group captions, contextual tab-set names, egui's small text |
| `ui` | Regular | 12 px | Labels, menus, buttons, tabs, fields |
| `strong` | Semibold | 12 px | Dialog section headings, the selected File view entry |
| `lead` | Regular | 14 px | The wizard and About body text, which grows with the window |
| `title` | Semibold | 15 px | Dialog titles (egui's heading style) |
| `display` | Light | 22 px | Wizard headings, File view page titles |

Tab captions are drawn in title case, as the data has them.

## Spacing, radii and borders

- **Spacing** is a 4 px grid: 2, 4, 8, 12, 16 and 24 px. Inner padding
  of a button is 4 px; between groups 8 px; dialog margins 16 px;
  between dialog sections 24 px.
- **Radii**: 0 for bars and docked panels (they meet the window
  edge); 4 px for buttons, fields, tabs, page tabs, list rows and menu
  highlights; 6 px for popups (menus, tooltips, dialogs, the wizard,
  floating toolbars).
- **Borders**: 1 px everywhere; the focus ring is 2 px, outside the
  control. Popups have a 1 px `border` edge and a soft shadow.

## The token mapping

The existing widget tokens keep their names. Each takes a palette
value:

| Widget token | Palette |
|---|---|
| `chrome.background`, `chrome.title_bar`, `status_bar.background`, `file_view.nav.background`, `ruler.background` | `surface` |
| `menu.background`, `menu.gutter`, `pane.background`, `pane.title.fill`, `tooltip.background`, `field.background`, `file_view.background` | `surface.raised` |
| `chrome.border`, `chrome.separator`, `group.separator`, `menu.border`, `menu.separator`, `pane.border`, `tooltip.border` | `border` |
| `chrome.text`, `tab.text`, `page_tab.text`, `status_bar.text`, `file_view.nav.text`, `pane.title.text` | `text` |
| `group.caption.text`, `ruler.ticks` | `text.muted` |
| `chrome.text_disabled` | `text.disabled` |
| `accent`, `tab.selected.text`, `page_tab.active.text` | `accent` |
| `accent.text` | `accent.text` |
| `button.hover.fill`, `button.hover.border`, `menu.highlight.fill`, `menu.highlight.border` | `hover` |
| `button.pressed.fill`, `menu.bar.open.fill`, `file_view.nav.selected` | `selected` |
| `button.pressed.border`, `menu.bar.open.border` | `selected.border` |
| `canvas.background`, `page.*` | the drawing-area values |

The design added these tokens: `button.active.fill` (`pressed`),
`focus`, `control.border` (`border.strong`), `chrome.text_muted`
(`text.muted`), `accent.hover`, `accent.pressed`, `button.fill` (a
button at rest: transparent), `button.raised.fill` (a standard
button: white), `shadow`, and the metrics `radius.control`,
`radius.popup`, `focus.width`, `tab.underline` and
`file_view.nav.indicator`. Tooltips draw with the menu tokens (egui
gives them the popup frame), so the `tooltip.*` tokens went.

## Where the layouts differ

Only structure differs. Colours never do.

| | Panel Modern | Panel Classic |
|---|---|---|
| Top of the window | File button, tab strip, command bar with groups | Menu bar, docked toolbar rows with grippers |
| Commands | Large, medium and small buttons in groups with captions | 16 px toolbar buttons and combo boxes |
| Menus | Drop-downs from split buttons and galleries | The menu bar's menus and their submenus; an icon column at the left |
| File commands | The File view | The File menu |
| Shared | Status bar, panes, Shapes window, page tabs, rulers, canvas, dialogs, wizard, About window, context menus |

Both layouts share one theme file for colours and type,
`ui-spec/themes/base.yaml`. A layout's own file holds only its
structure: the Panel Modern contextual tab sets, and the Panel Classic
title bar, Shapes window and page-tab heights. The loader refuses a
layout file that sets a colour or a font.

## The choices (approved 2026-10-26)

1. **File view navigation**: neutral (`surface`), the selected entry in
   the selected tint with a 3 px accent bar at its left.
2. **Status bar**: neutral (`surface`).
3. **Tab captions**: title case.
4. **Selected tab**: accent caption with a 2 px accent underline.
5. **Accent hue**: `#1f5bc4`, the brand blue of the Larrygram art
   (`#1e66e0`) deepened until it passes AA on every surface, so the
   wizard pictures and the chrome agree.

## How the widgets apply it

- `theme::apply` maps the tokens onto egui: buttons are flat at rest
  (`button.fill`), take `button.hover.*` under the pointer,
  `button.active.fill` while held, and the selected tint with an
  accent label when selected. The active (held or focused) state has a
  `focus.width` accent edge. Every widget state has the
  `radius.control` corners; popups and windows have `radius.popup`.
- `theme::raise_controls` gives the controls of a ui the standard
  frame (`button.raised.fill`, `control.border`); check boxes and
  radio buttons in that ui get the same edge. The wizard, the About
  window and the File view pages use it.
- `theme::accent_controls` makes a ui's buttons default buttons
  (`accent`, `accent.hover`, `accent.pressed`, `accent.text`): the
  File button, the wizard's Next and Start.
- A disabled icon is tinted `chrome.text_disabled`, like a disabled
  label (it was a 40% alpha).
- The test `crates/ui/tests/phase7/palette.rs` draws the surfaces of
  both layouts and fails when any shape outside the canvas paints a
  colour that is not a theme token.

egui 0.37's per-widget style providers would let check boxes carry an
edge everywhere while buttons stay flat, but they sit behind egui's
`experimental` feature. Until then a surface with check boxes calls
`raise_controls`.

## The pictures

The pictures are local (never in git), under
`/Volumes/Dara/larrygram/reference/ui-review/design/`, every surface of
both layouts at 1440 x 900, scales 1 and 2:

- `before/`: the two layouts before the design pass;
- `proposal/`: the approved mock-ups (the palette applied through the
  theme alone), with `contact-sheet.html`;
- `after/`: the result, with `contact-sheet.html` (before and after side
  by side).

They come from the P7-17 review sweep:

```sh
LARRYGRAM_UI_REVIEW_DIR=/Volumes/Dara/larrygram/reference/ui-review/design/after \
LARRYGRAM_UI_REVIEW_ONLY=1440x900 \
  cargo test -p larrygram-ui --test phase7 review -- --ignored
```

## Still to adopt the design

These surfaces are drawn by files outside task P7-18 (`shell.rs`,
`panes.rs`, `shapes_window.rs`, `canvas.rs`), so they keep some of
their own constants for now:

- the Options window body should call `theme::raise_controls`, so its
  check boxes and radio buttons get the `control.border` edge;
- the canvas grid, the selection, the handles, the snap guides and the
  marquee are constants in `canvas.rs`; they need tokens
  (`canvas.grid.minor`, `canvas.grid.major`, `canvas.selection`,
  `canvas.handle.fill`, `canvas.snap_guide`) added to `base.yaml` in the
  same change that reads them;
- the Shapes window title is Selawik Light 22 px from a constant; it
  should use the `strong` role;
- the status bar ("PAGE 1 OF 1") and the Shapes window links
  ("STENCILS", "SEARCH") are upper-cased in code; title case matches the
  tabs;
- the Shape Data fields in the panes have no `control.border` edge.
