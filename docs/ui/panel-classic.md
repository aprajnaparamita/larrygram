# The Panel Classic layout

How the Panel Classic layout looks and behaves. The data is
`ui-spec/layouts/panel-classic.yaml`, and the full outline is
[panel-classic-inventory.md](panel-classic-inventory.md). The colours and sizes
named below are tokens in `ui-spec/themes/panel-classic.yaml`.

## Window

From top to bottom:

1. **Title bar** (23 px): the document name and the app name.
2. **Menu bar** (24 px):
   - the menus File, Edit, View, Insert, Format, Tools, Data, Shape,
     Window, Help;
   - at the right end, a "Type a question for help" search box. It
     runs `help.search` with the typed text.
3. **Toolbar rows** (26 px each):
   - Standard on row 0 and Formatting on row 1;
   - each toolbar starts with a 7 px gripper;
   - toolbars sit end to end; one that does not fit wraps to a new
     row, and one wider than the window breaks between its buttons
     (never between a split button and its arrow).
4. **Body**:
   - the docked Shapes window on the left (190 px: title row with a
     close button, a search box, then the stencils);
   - the drawing window with rulers on the top and left (16 px);
   - the page area on `canvas.background`.
5. **Page-tab strip** (18 px) under the drawing:
   - first, previous, next and last page buttons;
   - the page tabs;
   - the horizontal scroll bar.
6. **Status bar** (20 px): panes for width, height and angle of the
   selection on the left, and "Page n/m" on the right.

The whole chrome (menu bar, toolbar rows and status bar) is one flat
`chrome.background` colour.

## Menus

- **Opening.** A click on a menu title opens it. While a menu is open,
  moving the pointer to another title opens that menu instead. Alt or
  F10 puts focus on the menu bar and shows the access-key underlines.
  Alt+letter opens that menu directly. Esc closes one level; a click
  outside closes all.
- **Look.** Menus are drawn on `menu.background` with a 1 px
  `menu.border`:
  - On the left is a 24 px gutter (`menu.gutter`) holding the 16 px
    icon. A checked item with no icon shows a check mark there; one
    with an icon draws a pressed frame around the icon.
  - Items are 23 px high. The label is followed by the shortcut,
    right-aligned. A submenu shows ▸ at the right edge.
  - `begin_group` draws a 1 px `menu.separator` line, inset past the
    gutter.
- **Hover.** The hovered item gets `menu.highlight.fill` with a 1 px
  `menu.highlight.border`. This also applies to disabled items, whose
  text stays `chrome.text_disabled`.
- **The open top-level title** gets `menu.bar.open.fill` and
  `menu.bar.open.border`, joined to the menu below it.
- **Submenus** open to the right after a short hover (about 400 ms), or
  at once with a click or the Right arrow key. They open on the left if
  there is no room on the right. Only one submenu chain is open at a
  time.
- **Closing submenus.** Hovering another row of the parent closes the
  open submenu. Once the pointer has moved off every open menu and
  stayed off for the same 400 ms, every submenu closes; the top-level
  menu stays open until a click outside or Esc. A pointer that rests
  outside the menus does not close a submenu the keyboard opened. The
  same rules apply to context menus and toolbar popups.
- **Keyboard.** Up and Down move between items, skipping separators and
  hidden items. Disabled items can get focus but cannot be activated.
  Enter or Space activates the item. Right opens a submenu; Left closes
  it, or moves to the previous top-level menu.
- **Hidden items** (`hidden: true`) are not drawn. They are kept for
  later features, such as document check-in or publishing to a fixed
  format.
- **Run-time lists** are inserted where the `list` item sits:
  - File > recent files, numbered 1–9 with the digit as the access key;
  - Window > open windows, with the active one checked;
  - Edit > Go To > pages.

## Menu bars per window mode

| Mode | Menu bar | Extra toolbars |
|---|---|---|
| A drawing window is active | `drawing` | none |
| A ShapeSheet window is active | `shapesheet` (adds the section and row editing items) | ShapeSheet Standard, ShapeSheet Formula Bar, ShapeSheet Formula Tracing |
| A stencil is open for editing | `stencil` (adds a Master menu) | Stencil Standard, Stencil Format |

The swap happens when the active window changes. The user's toolbar
choices for each mode are remembered separately.

## Toolbars

- **Buttons** are 23×22 px with a 16 px icon:
  - hover draws `button.hover.fill` and `button.hover.border`;
  - a latched tool or toggle (the active Pointer Tool, or Align Center
    while it applies) draws `button.pressed.fill` and
    `button.pressed.border`;
  - disabled buttons draw the icon in `chrome.text_disabled`.
- **Display.** `display: text` items show only their label (the colour
  palette entries); `icon_text` shows both (Theme). Every button has a
  tooltip with the label and, if there is one, the shortcut in
  brackets: "Cut (Ctrl+X)".
- **Split buttons** (Undo, Redo, Text Color, Line Color, Fill Color,
  Line Weight, Line Pattern, Line Ends, Pointer Tool, Connector Tool,
  Text Tool, New):
  - the main part runs `command`;
  - an 11 px arrow part opens the `items`;
  - `split_recent` (Connector Tool, Text Tool) puts the last chosen
    child into the main part.
- **Colour palettes** open a popup with headings ("Theme Colors",
  "Standard Colors", "Recent Colors"), then a grid of swatches
  (`list: theme_colors`, ...), then "More … Colors…".
- **Line palettes** (`palette`) show "No Line", a list of sample
  strokes, then "More Line …".
- **Combos**: Font (144 px), Font Size (86 px) and Zoom (65 px). Each
  is editable, applies on Enter or when an entry is picked, and reverts
  on Esc.
- **View > Toolbars** lists the 16 general toolbars with a check on
  the visible ones. Choosing one toggles it (commands
  `view.toolbar.<name>`). Customize… is out of scope and stays
  disabled.
- **Right-click** on the toolbar area opens the same list.
- **Floating toolbars** (`dock: floating`: Drawing, Ink, Picture) open
  as small tool windows near the top left of the drawing the first
  time they are shown, each under the one before. A window holds its
  buttons in rows of at most six. Dragging toolbars between docked and floating is
  out of scope for Phase 7.

## Context menus

- A right-click selects what it hits first (if it is not already
  selected), then opens the matching menu. P7-08 maps hit targets to
  menus. The rows marked *pending* are not routed yet: the shell
  routes the canvas (a shape, the page, text being edited), the Shapes
  window, the Shape Data and Size & Position panes, the page tabs and
  the page navigation buttons (P7-14); the pending targets need
  window state that does not exist yet (a full-screen view, master
  editing, embedded objects) or hit rects inside one more widget:

  | Target | Menu | State |
  |---|---|---|
  | A shape | `context.shape` | routed |
  | An empty part of the page | `context.page` | routed |
  | Text being edited | `context.text_edit` | routed |
  | A page tab | `context.page_tab` | routed |
  | The page navigation buttons | `context.page_tab_navigation` | routed (no menu in the classic data yet) |
  | The Shapes window | `context.shapes_window` | routed |
  | The Shape Data window | `context.shape_data_window` | routed |
  | The Size & Position window | `context.size_position_window` | routed |
  | A connection point (with the connection point tool) | `context.connection_point` | pending |
  | A comment mark | `context.comment` | pending |
  | A hyperlink in text | `context.hyperlink` | pending |
  | A docked window's title | `context.window_title` | pending |
  | Inside a ShapeSheet window | `context.shapesheet` | pending |
  | A master in an editable stencil | `context.stencil_edit` | pending |
  | The page of a master being edited | `context.master_edit` | pending |
  | An embedded object | `context.embedded_object` | pending |
  | The drawing in full-screen view | `context.full_screen` | pending |

- They look and behave like menus, and Shift+F10 opens the one for the
  current selection.
- Items with `hidden: true` stay hidden unless the target supports them.

## Shapes window

- The title row reads "Shapes", with a close button.
- Below it is "Search for Shapes:", and under that a text box, a
  dropdown arrow and a green go button. The window keeps its 190 px
  (`shapes_window.width`, at most 30% of a narrow window).
- Then come the open stencils, each as a title bar that expands or
  collapses.
- With no stencil open, the window shows a short getting-started note
  in our own words.
- Right-clicking a stencil title offers the `context.shapes_window`
  items: the view modes (Icons and Names, Names Under Icons, Icons
  Only, Names Only, Icons and Details), Close and Hide Window.

## Status bar

The status bar is a row of sunken segments. The left segments show
the width, height and angle of the selection when there is one. The
right-most segment shows "Page n/m". Segments are `chrome.background` with
a 1 px darker inset edge.
