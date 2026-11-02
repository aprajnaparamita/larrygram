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

- **macOS**: right-click (or Control-click) `Larrygram.app` and choose
  **Open**, then **Open** in the dialog. If the dialog refuses, open
  **System Settings > Privacy & Security**, scroll down and click
  **Open Anyway**. After that the app opens normally; a newer build may
  ask once more. The `.dmg` also holds an `Applications` link: drag
  the app there and start it from your Applications folder.

- **Windows**: unpack the zip anywhere (for example
  `%LOCALAPPDATA%\Larrygram`) and run `larrygram.exe`. Windows
  Defender SmartScreen may say it protected you: click **More info**,
  then **Run anyway**.

- **Linux**: the package is not an installer; unpack it and follow the
  `INSTALL.txt` inside. The short version for one user:

  ```sh
  tar xzf Larrygram-<version>-linux-x64.tar.gz
  cd Larrygram-<version>-linux-x64
  cp larrygram larrygram-cli ~/.local/bin
  cp larrygram.desktop ~/.local/share/applications/
  cp larrygram.png ~/.local/share/icons/hicolor/256x256/apps/
  ```

  The two binaries keep their executable bit; `chmod +x larrygram
  larrygram-cli` restores it if your unpacker drops it. The desktop
  entry starts `larrygram` and registers the Visio drawing types, so
  double-clicking a `.vsdx` or `.vsd` file opens it in Larrygram.

## What the alpha can and cannot do

It opens `.vsdx` drawings and imports `.vsd` drawings (read-only:
saving writes `.vsdx`), and it edits and saves drawings. It is alpha
software: expect rough edges and save your work often.

## Report a bug

Open an issue in the repository's issue tracker. The repository has
bug-report templates; pick the one for defects. Whatever the template
asks, the report needs at least:

- what you did, what you expected and what happened instead;
- the drawing, if you can share it (a small file that shows the
  problem is best);
- your system (macOS version, Windows version or Linux
  distribution);
- the app version from **Help > About** inside Larrygram — that is
  the version in the package name and the exact build you ran.

If the app cannot open at all, the version is in the package name you
downloaded.
