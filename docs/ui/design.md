# Design language

Status: **proposal** (task P7-18, step 1). The user approves it before
any widget changes.

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

One family, Selawik (OFL, bundled), at four sizes. Both layouts use
the same scale.

| Role | Face | Size | Use |
|---|---|---|---|
| `caption` | Regular | 11 px | Group captions, status bar, ruler numbers |
| `body` | Regular | 12 px | Labels, menus, buttons, tabs, fields |
| `strong` | Semibold | 12 px | Pane titles, dialog section headings, the selected File view entry |
| `title` | Semibold | 15 px | Dialog and wizard titles, File view page titles |
| `display` | Light | 22 px | The wizard welcome heading and the About window name |

Tab captions are drawn in title case, as the data has them. The
upper-case transform goes (a choice for the user, below).

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

Step 2 adds the tokens the palette needs and the widgets do not have
yet: `pressed`, `focus`, `border.strong`, `text.muted`,
`accent.hover`, `accent.pressed`, and the radius and spacing metrics.

## Where the layouts differ

Only structure differs. Colours never do.

| | Panel Modern | Panel Classic |
|---|---|---|
| Top of the window | File button, tab strip, command bar with groups | Menu bar, docked toolbar rows with grippers |
| Commands | Large, medium and small buttons in groups with captions | 16 px toolbar buttons and combo boxes |
| Menus | Drop-downs from split buttons and galleries | The menu bar's menus and their submenus; an icon column at the left |
| File commands | The File view | The File menu |
| Shared | Status bar, panes, Shapes window, page tabs, rulers, canvas, dialogs, wizard, About window, context menus |

Both layouts share one theme file for colours and type
(`ui-spec/themes/base.yaml`, step 2). A layout's own file holds only
its metrics: bar heights, button sizes and toolbar widths.

## Choices for the user

1. **File view navigation.** Neutral (`surface`, with the selected
   entry in the selected tint and an accent bar at its left) as
   proposed, or an accent-filled rail as before.
2. **Status bar.** Neutral (`surface`, muted text) as proposed, or
   accent-filled.
3. **Tab captions.** Title case as proposed, or upper case as before.
4. **Selected tab.** Accent caption with a 2 px accent underline as
   proposed, or a raised white tab with a border.
5. **Accent hue.** `#1f5bc4` is the brand blue of the Larrygram art
   (`#1e66e0`), deepened until it passes AA on every surface, so the
   wizard pictures and the chrome agree. A quieter slate blue
   (`#3a5ba0`) is the alternative.

## The mock-ups

The pictures are local (never in git), under
`/Volumes/Dara/larrygram/reference/ui-review/design/`:

- `before/`: every surface of both layouts today, at 1440 x 900,
  scales 1 and 2;
- `proposal/`: the same surfaces with the palette in both theme files
  and the egui mapping of `theme.rs` (flat buttons at rest, 4 px and
  6 px radii, the accent for selection and links). No widget code
  changed;
- `proposal/contact-sheet.html`: before and proposal side by side.

What the mock-ups cannot show yet, because it needs widget changes in
step 2:

- the selected-tab underline, title-case tab captions and the accent
  bar on the selected File view entry;
- a standard frame (white fill, `border.strong` edge) for dialog
  buttons, the wizard's Back and Start and the File view tiles. The
  flat rest state leaves them as bare text in the mock-ups;
- the focus ring and the check box and radio edges in
  `border.strong`;
- the disabled-icon treatment (today a faint alpha; step 2 draws it
  in `text.disabled`);
- the type scale (the caption and title sizes);
- the canvas grid, selection, handles and the Shapes window title.
  These are in P7-19's files: step 2 lists the tokens they need.
