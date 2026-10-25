# The Panel Modern layout

How the Panel Modern layout looks and behaves. The data is
`ui-spec/layouts/panel-modern.yaml`, and the full outline is
[panel-modern-inventory.md](panel-modern-inventory.md). The colours and sizes
named below are tokens in `ui-spec/themes/panel-modern.yaml`.

## Window

From top to bottom:

1. **Title bar** (25 px, `chrome.background`):
   - on the left, the app icon and the Quick Access Toolbar (QAT);
   - in the centre, the document title in `chrome.text`;
   - on the right, a help button and the window buttons.
2. **Tab strip** (25 px):
   - a 59 px File button in `accent` with `accent.text`, the window's
     one accent-filled control;
   - then the tabs Home, Insert, Design, Data, Process, Review and
     View;
   - tab captions are drawn in title case;
   - the selected tab has its caption in `tab.selected.text` and a
     `tab.underline` (2 px) accent underline.
3. **Panel Modern** (92 px):
   - groups run left to right, separated by 1 px `group.separator`
     lines;
   - each group has a caption centred along its bottom edge
     (`group.caption.text`, 17 px row);
   - a launcher, when the group has one, sits in the caption's bottom
     right corner;
   - a ^ button at the far right collapses the command bar.
4. **Body**:
   - the Shapes window on the left (262 px): a large "Shapes" title in
     `pane.title.text`, the STENCILS and SEARCH headings, then the
     stencils;
   - the drawing window with rulers;
   - the page area on `canvas.background`, with a soft `page.shadow`
     around the page.
5. **Page-tab strip** (40 px):
   - under the horizontal scroll bar: the page tabs (the active one in
     `page_tab.active.text`);
   - an "All ▴" menu listing every page;
   - a ⊕ insert-page button.
6. **Status bar** (22 px, `status_bar.background`, white text):
   - on the left, "PAGE n OF m" and the document language;
   - on the right, a presentation-mode button, the zoom slider with −
     and + buttons, the zoom percentage, fit-page-to-window and
     switch-window buttons. The buttons draw flat on the bar;
   - the right side lays out first and the texts on the left get the
     room it leaves. On a bar under 760 px the switch-window button
     is left out and fit-page-to-window reads "Fit".

## Tabs

- A click selects a tab. The mouse wheel over the tab strip or command bar
  moves between tabs.
- A double-click on a tab, or Ctrl+F1, collapses the command bar to the tab
  strip.
  - While collapsed, a click on a tab shows the command bar as an overlay
    over the drawing. It closes again after a command runs or on a
    click elsewhere.
  - Another double-click pins it open again.
- **Contextual tabs** (ADR 0064) appear to the right of View, with the
  tab-set name in small text in front of the tab's label:

  | Tab set | Tab | Shown in | Colour |
  |---|---|---|---|
  | Picture Tools | Format | `picture_selected` | green |
  | Container Tools | Format | `container_selected` | orange |
  | ShapeSheet Tools | Design | `shapesheet_window` | green |

  - The tab gets the set's `fill`, with a 4 px `band` across the top.
    The tab-set name uses the set's `text` colour. (The name sits in
    the tab, not above it: the 25 px strip has no room for a caption
    row, and drawn over the label the two were unreadable.)
  - A contextual tab is not selected automatically.
  - If its context ends while it is selected, the command bar returns to the
    last core tab used.

## Narrow windows

The command bar fits its groups into the window's width, with the ^
button's corner kept free. Every group starts at the size the data
gives it. While the groups do not fit, they shrink one stage at a
time, from the rightmost group leftwards; only when every group has
reached a stage does the next stage start:

1. **Medium**: large controls become medium (icon beside the label)
   and the controls pack three to a column in their slot order.
2. **Small**: every control with an icon shows only its icon; combos,
   check boxes and controls with no icon stay medium.
3. **Collapsed**: the group is one large button with the group's
   caption and its first control's icon. It opens the whole group, at
   full size, as a popup that closes after a command runs, on a click
   outside it, or on Esc.

Once the groups fit, any room left goes back to the groups from the
left, one stage at a time. `rows` groups (Font, Paragraph) skip the
medium and small stages and only collapse.

## Groups and controls

A group is laid out as either a `stack` or `rows`.

**`stack`**:

- Columns run left to right.
- A **large** control is at least 42 px wide and 69 px high: a 32 px
  icon at the top, then a label of up to two lines below it. A dropdown
  or split adds ▾ after the label.
- Other controls stack three per column (23 px rows) or, with
  `rows: 2`, two per column centred vertically.
  - **Medium**: 16 px icon plus label, as wide as needed.
  - **Small**: 16 px icon only, 22 px wide.
  - **Checks**: a 13 px box plus label.

**`rows`** (Font, Paragraph):

- Two rows of small buttons and combos, each laid out left to right.
- A small gap separates clusters (`rows.gap`).

Controls in detail:

- **Split buttons**, for example Paste and Bring to Front:
  - the icon part runs `command`; the arrow part opens the menu;
  - large splits divide horizontally (icon above, label and ▾ below);
  - medium and small splits divide vertically, with a 12 px arrow.
- **Dropdowns** open their `items` as a menu under the control. A
  dropdown or split whose own command is not available still opens
  when an entry of its menu runs. In that menu:
  - entries with icons use the same 24 px gutter as Panel Classic menus;
  - `options` show as text rows;
  - a `list` shows as rows, or as a grid for galleries;
  - a submenu opens on hover or click. Hovering another row closes it,
    and so does moving the pointer off every menu for 400 ms (the
    Panel Classic rule); the menu itself stays open until a click
    outside or Esc.
- **Galleries** (Quick Styles, Themes, Variants, Backgrounds, Borders &
  Titles, Data Graphics, Change Shape, Container Styles) open a grid of
  thumbnails filled from their `list`. Phase 7 shows Larrygram's own
  entries, which may be none yet.
- **Combos**: Font (107 px) and Font Size (52 px). They behave as in
  Panel Classic.
- **Launchers** are a 16 x 17 px arrow in the caption's corner; they
  open the group's dialog (Font, Paragraph, Shape
  Format, Page Setup, Layout and Routing, Show, Visual Aids, Adjust).
  Their tooltip names the dialog.
- **Disabled controls** draw text and icons in `chrome.text_disabled`.
- **Hover** uses `button.hover.fill`. A pressed or latched control (the
  active Pointer Tool) uses `button.pressed.fill`.
- **Tooltips** show the label, then the shortcut in brackets, then one
  sentence of our own text taken from the command's description.
- **Hidden controls** (Left-to-Right, Right-to-Left and Text Direction
  in Paragraph, and Chart in Illustrations) are not drawn. They are kept
  for vertical and right-to-left text support.

## Quick Access Toolbar

- Contents: Save, Undo (a split with a history list; the label changes
  to "Undo <action>"), Redo (the label changes to "Can't Redo" or
  "Repeat <action>"), and ▾ for the customise menu.
- Customisation is out of scope. The ▾ menu shows the current items as
  checked and disabled.

## File view

- **Opening.** File (or Alt+F) replaces the whole window below the
  title bar with the File view:
  - on the left, a 123 px navigation column in
    `file_view.nav.background`, with a ← back button at the top;
  - nav entries are 38 px high, and the selected page uses
    `file_view.nav.selected`;
  - on the right, the page on `file_view.background`.
- **Entries.**

  | Entry | Kind | Behaviour |
  |---|---|---|
  | Info | page | Document properties, the file path and protection status |
  | New | page | Blank Drawing (runs `file.new`), then Larrygram's own template tiles; no tile when there are none |
  | Open | page | Recent files, then Browse… (native dialog or upload) |
  | Save | command | Runs `file.save` and closes the view |
  | Save As | page | Browse… (native dialog or download) |
  | Print | page | Print settings and a preview |
  | Export | page | Export formats: PNG, SVG, PDF when available |
  | Close | command | Runs `file.close` |
  | Options | command | Opens the Options dialog |

- **Closing.** Esc or ← closes the view and returns to the last tab.

## Context menus

Right-click behaviour is the same as in Panel Classic (see
[panel-classic.md](panel-classic.md)), with the Panel Modern menu set:

- Shape;
- Page (empty canvas);
- Text editing;
- Page tab and Page navigation;
- Hyperlink and Connection point;
- Full screen;
- ShapeSheet;
- Issues list;
- Embedded object;
- Master editing and Stencil editing.

A Panel Modern context menu is shorter than its Panel Classic counterpart. Shape
formatting lives on the command bar, and the menu keeps clipboard, grouping,
container, data, text and connector commands.

## Fluent-data layouts

The 2016 and 365 layouts render with these same widgets through the v1
adapter (ADR 0061). Their groups have no slot data, so controls flow in
file order as medium buttons. Large sizes apply only to the first
control of a group that has a menu.
