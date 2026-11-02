//! The Windows console attach (task R11 goal 2, ADR 0074).
//!
//! The desktop app is a GUI-subsystem binary
//! (`windows_subsystem = "windows"`), so it has no console of its own
//! and its prints go nowhere. When it was started from a console
//! (`larrygram.exe --help`, or a startup error run from a terminal),
//! the parent process has one and `AttachConsole(ATTACH_PARENT_PROCESS)`
//! puts the app on it, so both prints appear there. A double-click
//! start has no parent console: the attach fails and the prints stay
//! silent, exactly the behaviour of the GUI build (task R11 goal 2).
//!
//! A GUI process's standard handles are not the console's screen
//! buffer, so after the attach the module points them at `CONOUT$`
//! before the first write. All `unsafe` code of the Windows build is
//! confined to this one module, the pattern of ADR 0074 (amended for
//! this module): the crate denies `unsafe` everywhere else.

#![allow(unsafe_code)] // ADR 0074: the console attach needs the Win32 calls.

use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::Console::{
    ATTACH_PARENT_PROCESS, AttachConsole, STD_ERROR_HANDLE, STD_HANDLE, STD_OUTPUT_HANDLE,
    SetStdHandle,
};

/// `CONOUT$`, the attached console's screen buffer, as a
/// NUL-terminated wide string (`C`, `O`, `N`, `O`, `U`, `T`, `$`).
const CONOUT: [u16; 8] = [
    0x0043, 0x004F, 0x004E, 0x004F, 0x0055, 0x0054, 0x0024, 0x0000,
];

/// Attaches the process to its parent's console and points the
/// standard output and error handles at it (task R11 goal 2).
///
/// Best effort, never a panic (AGENTS.md section 6): no parent
/// console is a silent no-op and a failure leaves the prints silent,
/// as they were before.
pub(crate) fn attach_parent_console() {
    // SAFETY: `AttachConsole` takes only the `ATTACH_PARENT_PROCESS`
    // sentinel, dereferences no pointer and returns zero when there
    // is no parent console (a double-click start), which is not an
    // error for us.
    if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } == 0 {
        return;
    }
    reopen(STD_OUTPUT_HANDLE);
    reopen(STD_ERROR_HANDLE);
}

/// Points one standard handle at the attached console's screen
/// buffer (task R11 goal 2): `AttachConsole` alone leaves the GUI
/// process's standard handles invalid, and the first print writes
/// through them.
fn reopen(which: STD_HANDLE) {
    // SAFETY: `CONOUT` is a valid NUL-terminated wide string. The
    // null pointers mean "no security attributes" and "no template"
    // for `OPEN_EXISTING`; the flags argument is 0. The returned
    // handle is checked before use.
    let console: HANDLE = unsafe {
        CreateFileW(
            CONOUT.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        )
    };
    if console == INVALID_HANDLE_VALUE {
        return;
    }
    // SAFETY: `SetStdHandle` takes the handle just opened for the
    // console and one standard-handle selector. A failure keeps the
    // previous handle and is not an error for us; the open handle is
    // then owned by the process until exit.
    unsafe {
        SetStdHandle(which, console);
    }
}
