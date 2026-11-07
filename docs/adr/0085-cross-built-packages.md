# 0085: Cross-built Windows and Linux packages on the Mac

- Status: accepted
- Date: 2026-11-02
- Task: R14

## Context

R9 builds each platform on its own host and left the Windows and Linux
legs to CI runners, but GitHub Actions is billing-locked. The user
chose (2026-11-08) to cross-build both on this Mac (Apple silicon,
macOS).

## Decision

Add `--cross` to `cargo xtask dist windows` and `cargo xtask dist
linux`. On a macOS ARM host, `--cross` allows the build to proceed:

- **Linux x86_64**: `cargo zigbuild` (zig as the C compiler and
  linker), targeting `x86_64-unknown-linux-gnu` with glibc 2.31 or
  older (Ubuntu 20.04+). The zig toolchain provides a glibc sysroot
  without shipping one.

- **Windows x86_64**: `cargo xwin` (xwin downloads the Microsoft CRT
  and SDK libraries under Microsoft's licence, cached outside the repo
  at `/Volumes/Dara/larrygram/xwin`). The build script embeds the
  Larrygram icon using `llvm-rc` and `llvm-ar` from Homebrew LLVM;
  if those tools are missing the step is skipped with a warning, never
  an error.

Without `--cross` nothing changes: each platform still refuses the
wrong host with R9's message. A missing cross-build tool is a clear
error naming the install command, before anything builds.

`dist mac` does not accept `--cross` (macOS builds run on this Mac
only).

## Consequences

- The Windows and Linux packages can be built without Windows or Linux
  hosts or CI runners.
- The packages are untested on real Windows and Linux; the user tests
  them manually.
- The cross-build tools (`zig`, `cargo-zigbuild`, `cargo-xwin`,
  Homebrew LLVM) are machine-wide dependencies not tracked by the
  workspace Cargo.toml.
- `cargo-xwin` downloads Microsoft's CRT and SDK libraries at build
  time; nothing from them is committed or redistributed beyond what
  the linker puts in the `.exe` (clean-room: AGENTS.md section 4).
- R9's "nothing cross-builds" rule is amended: with `--cross`, Windows
  and Linux cross-build from macOS ARM.

## Alternatives considered

- **VM or Docker on the Mac**: heavier, slower, more complex to
  maintain. zig and xwin are lighter and integrate with cargo.
- **Wait for GitHub Actions billing**: the user chose not to wait.
- **ring cross workaround** (from R9): no longer needed with zig and
  xwin as the compilers; not carried over.
