// SPDX-License-Identifier: MIT

use crate::{PaneInfo, Tmux, TmuxError, TmuxRunner};
use std::fmt;

/// Why a pane cannot be acted on as the process the caller saw.
#[derive(Debug)]
pub enum GuardError {
    /// tmux could not be asked.
    Tmux(TmuxError),
    /// There is no pane with that id.
    Missing,
    /// The pane id exists, but it now runs another process (the id was reused, or the agent
    /// was restarted in it).
    Changed { found: u32 },
}

impl fmt::Display for GuardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tmux(e) => e.fmt(f),
            Self::Missing => write!(f, "the pane no longer exists"),
            Self::Changed { .. } => write!(f, "the pane is no longer the process it was listed as"),
        }
    }
}

impl std::error::Error for GuardError {}

impl From<TmuxError> for GuardError {
    fn from(e: TmuxError) -> Self {
        Self::Tmux(e)
    }
}

impl<R: TmuxRunner> Tmux<R> {
    /// The pane `pane_id`, only if it still runs process `expected_pid`: the one the caller
    /// saw when it decided to act. Input and kills must never reach a process that replaced it
    /// (ADR-004, ADR-009), so every action that targets a pane by a request made earlier asks
    /// here first.
    pub fn guarded_pane(&self, pane_id: &str, expected_pid: u32) -> Result<PaneInfo, GuardError> {
        let pane = self
            .list_panes()?
            .into_iter()
            .find(|p| p.pane_id == pane_id)
            .ok_or(GuardError::Missing)?;
        if pane.pane_pid != expected_pid {
            return Err(GuardError::Changed {
                found: pane.pane_pid,
            });
        }
        Ok(pane)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TmuxOutput;

    struct Listing(&'static str);

    impl TmuxRunner for Listing {
        fn run(&self, _args: &[&str]) -> Result<TmuxOutput, TmuxError> {
            Ok(TmuxOutput {
                stdout: self.0.to_owned(),
            })
        }
    }

    const ROWS: &str = "%1\ts\tw\t@1\t0\t/\t7\t1\t1\t1\tzsh\t1700\t0\t\t\t\t/\t$1\t\t\tt\n\
                        %2\ts\tw\t@1\t1\t/\t9\t0\t0\t1\tzsh\t1700\t0\t\t\t\t/\t$1\t\t\tt\n";

    #[test]
    fn the_pane_is_returned_only_while_it_runs_the_process_the_caller_saw() {
        let tmux = Tmux::with_runner(Listing(ROWS));
        assert_eq!(tmux.guarded_pane("%1", 7).unwrap().pane_id, "%1");
        assert_eq!(tmux.guarded_pane("%2", 9).unwrap().window_id, "@1");
        assert!(matches!(
            tmux.guarded_pane("%1", 8),
            Err(GuardError::Changed { found: 7 })
        ));
        assert!(matches!(
            tmux.guarded_pane("%3", 7),
            Err(GuardError::Missing)
        ));
    }

    #[test]
    fn a_tmux_that_cannot_be_asked_is_not_mistaken_for_a_missing_pane() {
        struct Down;
        impl TmuxRunner for Down {
            fn run(&self, _args: &[&str]) -> Result<TmuxOutput, TmuxError> {
                Err(TmuxError::Failed {
                    code: Some(1),
                    stderr: "no server running".to_owned(),
                })
            }
        }
        assert!(matches!(
            Tmux::with_runner(Down).guarded_pane("%1", 7),
            Err(GuardError::Tmux(_))
        ));
    }
}
