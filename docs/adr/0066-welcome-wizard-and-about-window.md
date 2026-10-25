# 0066: The welcome wizard and the About window

- Status: accepted
- Date: 2026-10-23
- Task: P7-01

## Context

Larrygram is renamed and re-launched as a product, not just a
collection of layouts. A first launch should introduce the app, offer
the two layouts (Panel Modern and Panel Classic) and point at the
story behind the name. The app also needs an About window with the
dedication, reachable from every layout, and a way to re-open the
wizard later.

The user's brief (the Phase 7 plan, `docs/plans/phase-7.md`, section
2) fixes the wording of the About window; this record turns it into
an interface.

## Decision

### The welcome wizard

- It opens on the first launch, before any document, and again from
  **Help > Welcome**. It is a modal two-page window.
- **Page 1, Welcome.**
  - Left half: `about_screen.svg` (ADR 0067), scaled to fit and
    keeping its 16:10 ratio.
  - Right half, with generous padding, top to bottom:
    - the title **Larrygram v1.0 alpha**;
    - the short description: "Larrygram is a free, open-source
      diagramming app. Draw flowcharts and diagrams with shapes and
      connectors, and open, edit and save `.vsdx` drawings, on the
      desktop or in your browser.";
    - a **Why Larrygram?** button that opens the About window
      ([`about::show`]).
  - Bottom row: **Cancel** and **Next**.
- **Page 2, Choose your layout.**
  - Two large buttons side by side, each showing its picture with the
    name under it:
    - **Panel Modern** (`panel-layout.svg`), selected by default;
    - **Panel Classic** (`classic-layout.svg`).
  - Clicking a button selects it.
  - Bottom row: **Back** and **Start**. Start saves the choice as the
    default layout, applies it at once and closes the wizard.
- Cancel on either page closes the wizard and keeps the current
  layout (Panel Modern on a first launch, where it is the default).
  A wizard re-opened from Help > Welcome must not reset a Panel
  Classic user to Panel Modern (amended at P7-14, ruling on P7-11
  F4: the code behaviour was right, this line was wrong). The
  wizard is marked as seen in both cases, so it does not open again
  on the next launch.

### The About window

- It opens from **Why Larrygram?** and from **Help > About Larrygram**
  in every layout. Panel Classic carries the entry in each Help menu
  (`help.about`, next to `help.welcome`); Panel Modern takes both
  entries from the title-bar help button's menu (task P7-09).
- Left half: `larry.png` (ADR 0067), scaled to fit.
- Right half, with generous padding, this text (the brief's wording,
  verbatim):

  > This app is dedicated to Larry Jeffus.
  >
  > Larry is an engineer with a deeply inspiring story. He began life
  > with severe dyslexia, and although he could not read, he went on
  > to write more than 40 books and to teach hundreds of thousands of
  > students around the world.
  >
  > **[ Watch Larry's story ]** (centred; opens the video in the
  > browser)
  >
  > He loves to make all his engineering designs with diagramming
  > applications. That is why his niece, Janet Jeffus, decided to make
  > this app and dedicate it to his inspiring story.

- Bottom: a **Close** button.
- The video link is `https://youtu.be/wpgiRvcGY_U` (the `?si=...`
  share-tracking id is dropped). **Watch Larry's story** opens it
  through egui's open-URL output, which works on desktop and on web.

### The settings

Two fields, with serde defaults so an old store still loads:

- `welcome_seen: bool` (default `false`): the wizard opens on launch
  only while it is `false`, and both Cancel and Start set it;
- the default layout (`LayoutKind`, default `PanelModern`): the
  wizard's Start writes it and the shell applies it at once.

The persisted `Settings` type lives in `crates/commands` (ADR 0029),
which Phase 6 owns during P7-01. The fields therefore start as
`crates/ui/src/settings.rs::WelcomeSettings` (a stub type), and task
P7-09 moves them onto `larrygram_commands::Settings` and deletes the
stub.

### Interfaces (stubs written by P7-01, implemented by P7-15/P7-16)

- `crates/ui/src/wizard.rs`: `Page`, `State`, `Outcome` and
  `wizard::show(ui, &mut State) -> Result<Option<Outcome>, UiError>`.
  Next and Back move inside `State`; the caller applies
  `Outcome::Cancel` or `Outcome::Start { default_layout }`.
- `crates/ui/src/about.rs`: `about::VIDEO_URL`,
  `about::Outcome { Open, Close, WatchVideo }` and
  `about::show(ui) -> Result<Outcome, UiError>`. The caller opens
  `VIDEO_URL` through egui's open-URL output.
- `crates/ui/src/settings.rs::WelcomeSettings`.
- Commands: `help.about` and `help.welcome`, Phase 7 proposals in
  `ui-spec/commands-phase7.yaml` until P7-02 registers them.

## Consequences

- The wizard and the About window are plain egui widgets with golden
  images in the Phase 7 test pass (ADR 0065).
- The About text and the wizard text are ours (from the brief); no
  third-party long-form text is copied.
- A future "show the wizard on next start" option (P7-09) is one
  checkbox that clears `welcome_seen`.

## Alternatives considered

- **A first-run page in the File view.** Rejected: the wizard must
  appear before any document exists.
- **Opening the video with a platform API.** Rejected: egui's open-URL
  output already covers desktop and web with one call.
- **Putting the About text in a data file.** Rejected: it is UI copy
  of our own, and code review is the right place for it.
