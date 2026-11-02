# For testers: downloading and running the alpha builds

Larrygram alpha builds are unsigned, no-installer packages for macOS,
Windows and Linux (task R9). This page is for the people who test
them: how to download one, check it, open it despite the operating
system's warnings, and report a bug. Our own words throughout.

## Which build to download

Go to the repository's **Releases** page on GitHub and pick the newest
`v*` **pre-release**. It carries three packages; take the one for your
system:

| System | Package |
|---|---|
| macOS on Apple silicon | `Larrygram-<version>-macos-arm64.dmg` |
| 64-bit Windows | `Larrygram-<version>-windows-x64.zip` |
| 64-bit x86 Linux | `Larrygram-<version>-linux-x64.tar.gz` |

The first tag for testers is announced by the maintainer once the
pending repository history rewrite (task R6) is done; do not treat an
earlier tag as final, because that rewrite moves it.

## Check the download

Every pre-release carries a `SHA256SUMS` file with the checksum of
each package. Check yours against it before opening anything.

- **macOS** (Terminal, in the folder with the downloads):

  ```sh
  shasum -a 256 -c SHA256SUMS --ignore-missing
  ```

- **Linux** (a shell, in the folder with the downloads):

  ```sh
  sha256sum -c SHA256SUMS --ignore-missing
  ```

- **Windows** (PowerShell, in the folder with the downloads):

  ```powershell
  Get-FileHash .\Larrygram-<version>-windows-x64.zip -Algorithm SHA256
  ```

  The printed hash must match the line for your package in
  `SHA256SUMS`.

A mismatch means the download is broken or worse: delete it and
download again. Report it if it happens twice.

## Open an unsigned build

The builds carry no developer signature (task R9 decisions), so the
operating system warns on the first launch. That is expected.

- **macOS**: drag `Larrygram.app` from the `.dmg` onto its
  `Applications` link, then open it once from your Applications
  folder. macOS refuses and names the app. Click **Done** (not **Move
  to Trash**), open **System Settings > Privacy & Security**, scroll
  down to the line about Larrygram and click **Open Anyway**, then
  confirm. From then on the app opens normally; a newer build may ask
  once more.

  On macOS 14 (Sonoma) and earlier there is a shorter way:
  right-click (or Control-click) the app, choose **Open**, then
  **Open** in the dialog.

  If neither works, remove the download flag in Terminal:

  ```sh
  xattr -dr com.apple.quarantine /Applications/Larrygram.app
  ```

  This deletes the "downloaded from the internet" mark that makes
  macOS check the app, so it opens without the prompt.

- **Windows**: unpack the zip anywhere (for example
  `%LOCALAPPDATA%\Larrygram`) and run `larrygram.exe`. Windows
  Defender SmartScreen may say it protected you: click **More info**,
  then **Run anyway**.

- **Linux**: the package is not an installer; unpack it and follow the
  `INSTALL.txt` inside. The short version for one user:

  ```sh
  tar xzf Larrygram-<version>-linux-x64.tar.gz
  cd Larrygram-<version>-linux-x64
  mkdir -p ~/.local/bin ~/.local/share/applications ~/.local/share/icons/hicolor/256x256/apps
  cp larrygram larrygram-cli ~/.local/bin/
  cp larrygram.desktop ~/.local/share/applications/
  cp larrygram.png ~/.local/share/icons/hicolor/256x256/apps/
  command -v update-desktop-database >/dev/null && update-desktop-database ~/.local/share/applications
  ```

  The last line refreshes the menu's list of file types; it is skipped
  quietly where the tool is not installed. The two binaries keep their
  executable bit; `chmod +x larrygram larrygram-cli` restores it if
  your unpacker drops it.

  The desktop entry starts `larrygram` by name, so `~/.local/bin` must
  be on the `PATH` of your desktop session. Most distributions add it
  at login once the directory exists: log out and back in after the
  first install, and check with `echo $PATH`.

  Optional, to make Larrygram the default for `.vsdx` and `.vsd`
  files (otherwise it is only offered under "Open With"):

  ```sh
  xdg-mime default larrygram.desktop application/vnd.ms-visio.drawing.main+xml
  xdg-mime default larrygram.desktop application/vnd.visio
  ```

## What the alpha can and cannot do

It opens `.vsdx` drawings and imports `.vsd` drawings (read-only:
saving writes `.vsdx`), and it edits and saves drawings. It is alpha
software: expect rough edges and save your work often.

## Report a bug

Open an issue in the repository's issue tracker. The report needs at
least:

- what you did, what you expected and what happened instead;
- the drawing, if you can share it (a small file that shows the
  problem is best);
- your system (macOS version, Windows version or Linux
  distribution);
- the app version from **Help > About** inside Larrygram — that is
  the version in the package name and the exact build you ran.

If the app cannot open at all, the version is in the package name you
downloaded.
