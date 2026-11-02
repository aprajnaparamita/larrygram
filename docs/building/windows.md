# Building the Windows package

How to build the Windows alpha package (task R9). The build runs on a
Windows machine; there is no cross-building. The CI build is the same
command on the `windows-latest` runner
(`.github/workflows/release.yml`).

## What gets built

`cargo xtask dist windows` builds the release binaries and writes
`Larrygram-<version>-windows-x64.zip` under `<target dir>/dist/` (the
cargo target directory, never the repository):

- `Larrygram-<version>-windows-x64/larrygram.exe` — the desktop app;
- `Larrygram-<version>-windows-x64/larrygram-cli.exe` — the command
  line tool (its own console window);
- `Larrygram-<version>-windows-x64/LICENSE`, `NOTICE`,
  `ATTRIBUTION.md` and the bundled font licences
  (`fonts/Carlito-OFL.txt`, `fonts/Selawik-OFL.txt`).

The archive is written by the pure-Rust `zip` crate: no PowerShell,
`tar` or `7z` binaries are involved (task R9 goal 1). There is no
installer (no MSI) and no signing (task R9 decisions); the package is
a plain zip you unpack anywhere, for example
`%LOCALAPPDATA%\Larrygram`.

## Prerequisites

- 64-bit Windows (x86-64). The command refuses any other host.
- The Rust toolchain the workspace pins (`rust-toolchain.toml`), with
  the MSVC target (rustup installs it on first use).
- A working MSVC toolchain for `x86_64-pc-windows-msvc` (the rustup
  default on Windows).

## Build

```powershell
cargo xtask dist windows
```

Unpack the zip and run `larrygram.exe`. The app opens without a
console window (`windows_subsystem = "windows"`, task R9 goal 2);
`larrygram-cli.exe` keeps its console. Both the window and the
taskbar icon are rendered from `crates/ui/assets/brand/icon.svg` at
startup (ADR 0067), so there is no embedded `.ico` resource.

## Signing

The binaries are **unsigned** (task R9 decisions). Windows Defender
SmartScreen may warn on the first launch of a downloaded build; see
[testers.md](testers.md) for how to open it anyway.

## CI

`.github/workflows/release.yml` runs the same command on the
`windows-latest` runner for every `v*` tag and uploads the zip to the
GitHub pre-release. The regular CI (`.github/workflows/ci.yml`) runs
the quality gate on the same runner.
