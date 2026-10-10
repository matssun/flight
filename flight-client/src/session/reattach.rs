// SPDX-License-Identifier: MIT

use super::host::OpenFailure;
use flight_proto::ExitReasonCode;
use std::time::Duration;

/// When an attachment that went away is made again, for every kind of session: one surface full
/// screen, or each tile of a presentation. The rules are here so that they are the same; a
/// session only carries them out.
pub(crate) struct Reattach<'a> {
    delays: &'a [Duration],
}

/// What to do after a break.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Next {
    /// Attach again to the same process after this wait.
    After(Duration),
    /// The attempts are used up.
    GiveUp,
}

impl<'a> Reattach<'a> {
    pub(crate) fn new(delays: &'a [Duration]) -> Self {
        Self { delays }
    }

    /// Waits before each re-attachment of a surface whose stream broke; its length is the
    /// number of attempts.
    pub(crate) fn default_delays() -> Vec<Duration> {
        vec![
            Duration::from_millis(250),
            Duration::from_secs(1),
            Duration::from_secs(3),
        ]
    }

    /// After `attempts` attempts since the stream last worked.
    pub(crate) fn next(&self, attempts: usize) -> Next {
        match self.delays.get(attempts) {
            Some(delay) => Next::After(*delay),
            None => Next::GiveUp,
        }
    }

    /// Whether the node ending a terminal with `reason` is a break in the stream (the process is
    /// still there, so attach to it again) rather than the process having ended. A node that
    /// restarted lost its connection, not the user's work.
    pub(crate) fn exit_is_a_break(reason: ExitReasonCode) -> bool {
        reason == ExitReasonCode::NodeLost
    }

    /// Whether a failure to open is worth trying again: the far end could not be reached, as
    /// opposed to refusing. True the first time as much as after a break.
    pub(crate) fn open_failure_is_transient(failure: &OpenFailure) -> bool {
        matches!(failure, OpenFailure::Unavailable(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attempts_run_out_in_order_and_only_a_lost_node_is_a_break() {
        let delays = [Duration::from_millis(1), Duration::from_millis(2)];
        let policy = Reattach::new(&delays);
        assert_eq!(policy.next(0), Next::After(Duration::from_millis(1)));
        assert_eq!(policy.next(1), Next::After(Duration::from_millis(2)));
        assert_eq!(policy.next(2), Next::GiveUp);
        assert!(Reattach::exit_is_a_break(ExitReasonCode::NodeLost));
        assert!(!Reattach::exit_is_a_break(ExitReasonCode::ClientExited));
        assert!(Reattach::open_failure_is_transient(
            &OpenFailure::Unavailable("x".into())
        ));
        assert!(!Reattach::open_failure_is_transient(&OpenFailure::Refused(
            "x".into()
        )));
    }
}
