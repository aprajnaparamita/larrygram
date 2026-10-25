//! Handing operations to the desktop app's UI thread (ADR 0076; task
//! P9-06).
//!
//! The app's state lives on the UI thread, so a socket connection
//! thread never touches it. It sends each [`Op`] through an
//! [`InboxSender`] (a [`Backend`]) and waits for the answer; the sender
//! wakes the UI (the app passes egui's `request_repaint` as the
//! [`Waker`]). Each frame the app calls [`Inbox::drain`], which runs the
//! waiting operations between frames with the app's [`Executor`] and
//! [`Host`], so a remote edit never races a user edit. An answer that
//! does not come within `Limits::host_reply_timeout_ms` is `Busy`.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel as mpsc_channel};

use crate::backend::Backend;
use crate::error::ControlError;
use crate::exec::Executor;
use crate::host::Host;
use crate::limits::Limits;
use crate::ops::{Op, OpResult};

/// Wakes the UI thread when an operation is waiting (egui's
/// `Context::request_repaint`).
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// One waiting operation and the way back to its caller.
// Built and read by task P9-06 (`call` and `drain`); unused by the stubs.
#[allow(dead_code)]
struct Envelope {
    op: Op,
    reply: Sender<Result<OpResult, ControlError>>,
}

/// The sending side, one clone per connection (task P9-06).
#[derive(Clone)]
pub struct InboxSender {
    sender: Sender<Envelope>,
    waker: Waker,
    timeout_ms: u64,
}

impl std::fmt::Debug for InboxSender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InboxSender")
            .field("timeout_ms", &self.timeout_ms)
            .finish_non_exhaustive()
    }
}

/// The receiving side, owned by the UI thread (task P9-06).
pub struct Inbox {
    receiver: Receiver<Envelope>,
}

impl std::fmt::Debug for Inbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inbox").finish_non_exhaustive()
    }
}

/// A connected sender and inbox; `waker` runs whenever an operation is
/// sent.
pub fn channel(waker: Waker, limits: &Limits) -> (InboxSender, Inbox) {
    let (sender, receiver) = mpsc_channel();
    (
        InboxSender {
            sender,
            waker,
            timeout_ms: limits.host_reply_timeout_ms,
        },
        Inbox { receiver },
    )
}

impl InboxSender {
    /// How long a caller waits for the UI thread, in milliseconds.
    pub fn timeout_ms(&self) -> u64 {
        self.timeout_ms
    }
}

impl Backend for InboxSender {
    /// Sends `op`, wakes the UI and waits for the answer (task P9-06): a
    /// closed inbox (the app is quitting) is `Busy`, and so is a timeout.
    fn call(&mut self, op: Op) -> Result<OpResult, ControlError> {
        let _ = (op, &self.sender, &self.waker);
        Err(ControlError::not_implemented("inbox::InboxSender::call"))
    }
}

impl Inbox {
    /// Runs up to `max_ops` waiting operations with `executor` against
    /// `host` and sends each answer back (task P9-06); returns how many
    /// ran. Never blocks: an empty inbox returns 0 at once.
    pub fn drain(&mut self, executor: &mut Executor, host: &mut dyn Host, max_ops: usize) -> usize {
        let _ = (executor, host, max_ops, &self.receiver);
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use crate::host::HeadlessHost;
    use crate::policy::Policy;
    use larrygram_commands::Registry;
    use larrygram_commands::app::{AppCtx, Settings};

    #[test]
    fn the_stubs_report_not_implemented() {
        let limits = Limits::default();
        let (mut sender, mut inbox) = channel(Arc::new(|| {}), &limits);
        assert_eq!(sender.timeout_ms(), limits.host_reply_timeout_ms);
        assert!(format!("{sender:?}").starts_with("InboxSender"));
        assert!(format!("{inbox:?}").starts_with("Inbox"));
        let error = sender.clone().call(Op::Discover).expect_err("stub");
        assert_eq!(error.code(), ErrorCode::NotImplemented);
        let error = sender.call(Op::ListDocuments).expect_err("stub");
        assert_eq!(error.code(), ErrorCode::NotImplemented);
        let mut executor = Executor::new(Policy::default(), limits, true);
        let mut host = HeadlessHost::new(Registry::new(), AppCtx::new(Settings::default()));
        assert_eq!(inbox.drain(&mut executor, &mut host, 8), 0);
    }
}
