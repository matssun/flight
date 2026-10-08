// SPDX-License-Identifier: MIT

use super::ControlLink;
use crate::pane_agent;
use crate::tmux_servers::SCRAPE_LINES;
use crate::{PaneObservation, ServerOutcome};
use flight_tmux::{parse_panes_checked, TmuxError, PANE_FORMAT};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// A screen is never reused longer than this, whatever tmux says about activity: a bound on
/// staleness for anything the activity signal does not capture.
const MAX_CAPTURE_AGE_SECS: u64 = 30;

/// After a failed (re)connect, wait this long before trying again; sequential observation
/// carries the node meanwhile.
const RECONNECT_BACKOFF: Duration = Duration::from_secs(5);

const CLIENTS: &str = "list-clients -F '#{client_pid}\t#{client_session}'";

pub(crate) type Connector = Box<dyn FnMut() -> Result<Box<dyn ControlLink>, TmuxError> + Send>;

struct Cached {
    pid: u32,
    activity: u64,
    command: String,
    title: String,
    /// The caller's clock when the screen was captured.
    captured_at: u64,
    lines: Vec<String>,
}

/// One server observed over a control connection, with captures skipped for panes that
/// provably did not change.
///
/// Skipping is conservative: a screen is reused only when the pane is known, has the same
/// process, command and title, tmux reports activity in the window strictly before the
/// second of the last capture, and the capture is younger than [`MAX_CAPTURE_AGE_SECS`].
/// Anything else is captured. Any error on the connection drops it and everything cached, so
/// the next healthy round is a full refresh; meanwhile `observe` returns `None` and the caller
/// falls back to sequential observation.
pub(crate) struct Watch {
    connector: Connector,
    link: Option<Box<dyn ControlLink>>,
    cache: HashMap<String, Cached>,
    retry_after: Option<Instant>,
    degraded: bool,
    notes: Vec<String>,
}

impl Watch {
    pub(crate) fn new(connector: Connector) -> Self {
        Self {
            connector,
            link: None,
            cache: HashMap::new(),
            retry_after: None,
            degraded: false,
            notes: Vec::new(),
        }
    }

    pub(crate) fn take_notes(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notes)
    }

    /// The server's outcome over the control connection, or `None` when it is not usable now.
    pub(crate) fn observe(&mut self, now: u64) -> Option<ServerOutcome> {
        let result = self.connect().and_then(|()| self.round(now));
        match result {
            Ok(outcome) => {
                if std::mem::take(&mut self.degraded) {
                    self.notes
                        .push("control-mode observation restored (full refresh)".to_owned());
                }
                Some(outcome)
            }
            Err(e) => {
                // What was believed before the error is not trusted after it.
                self.link = None;
                self.cache.clear();
                if !self.degraded {
                    self.degraded = true;
                    self.notes.push(format!(
                        "control-mode observation unavailable ({e}); using sequential capture"
                    ));
                }
                None
            }
        }
    }

    fn connect(&mut self) -> Result<(), TmuxError> {
        if self.link.is_some() {
            return Ok(());
        }
        if self.retry_after.is_some_and(|t| Instant::now() < t) {
            return Err(TmuxError::Control("waiting to reconnect".to_owned()));
        }
        match (self.connector)() {
            Ok(link) => {
                self.link = Some(link);
                self.cache.clear();
                self.retry_after = None;
                Ok(())
            }
            Err(e) => {
                self.retry_after = Some(Instant::now() + RECONNECT_BACKOFF);
                Err(e)
            }
        }
    }

    fn round(&mut self, now: u64) -> Result<ServerOutcome, TmuxError> {
        let link = self
            .link
            .as_mut()
            .ok_or_else(|| TmuxError::Control("not connected".to_owned()))?;
        let own_pid = link.client_pid().to_string();
        let mut replies = link
            .run(&[
                format!("list-panes -a -F '{PANE_FORMAT}'"),
                CLIENTS.to_owned(),
            ])?
            .into_iter();
        let (Some(list), Some(clients)) = (replies.next(), replies.next()) else {
            return Err(TmuxError::Control("missing reply".to_owned()));
        };
        if !list.ok || !clients.ok {
            return Err(TmuxError::Control("tmux rejected a listing".to_owned()));
        }
        let infos = parse_panes_checked(&list.lines.join("\n"))?;
        let own_session = clients
            .lines
            .iter()
            .filter_map(|l| l.split_once('\t'))
            .find_map(|(pid, session)| (pid == own_pid).then_some(session))
            .ok_or_else(|| TmuxError::Control("own client not listed".to_owned()))?;

        let agents: Vec<_> = infos
            .into_iter()
            .filter_map(|info| Some((pane_agent(&info)?, info)))
            .collect();
        let stale: Vec<&str> = agents
            .iter()
            .filter(|(_, info)| self.needs_capture(info, now))
            .map(|(_, info)| info.pane_id.as_str())
            .collect();
        let commands: Vec<String> = stale
            .iter()
            .map(|id| format!("capture-pane -p -t {id} -e -S -{SCRAPE_LINES}"))
            .collect();
        let captured = if commands.is_empty() {
            Vec::new()
        } else {
            let link = self
                .link
                .as_mut()
                .ok_or_else(|| TmuxError::Control("not connected".to_owned()))?;
            link.run(&commands)?
        };
        if captured.len() != commands.len() {
            return Err(TmuxError::Control("missing capture reply".to_owned()));
        }
        let mut fresh: HashMap<String, Option<Vec<String>>> = stale
            .iter()
            .map(|id| (*id).to_owned())
            .zip(captured.into_iter().map(|r| r.ok.then_some(r.lines)))
            .collect();

        let mut panes = Vec::with_capacity(agents.len());
        let mut kept = HashMap::new();
        for (agent, info) in agents {
            let own = u32::from(info.session_name == own_session);
            let focused = info.focused_excluding(own);
            let lines = match fresh.remove(&info.pane_id) {
                // A failed capture classifies with no screen evidence rather than dropping the
                // pane, and is not remembered: the next round captures it again.
                Some(None) => Vec::new(),
                Some(Some(lines)) => {
                    kept.insert(info.pane_id.clone(), self.remember(&info, now, &lines));
                    lines
                }
                None => match self.cache.remove(&info.pane_id) {
                    Some(c) => {
                        let lines = c.lines.clone();
                        kept.insert(info.pane_id.clone(), c);
                        lines
                    }
                    None => Vec::new(),
                },
            };
            panes.push(PaneObservation::from_info(info, agent, lines, focused));
        }
        self.cache = kept;
        Ok(ServerOutcome::Observed(panes))
    }

    fn remember(&self, info: &flight_tmux::PaneInfo, now: u64, lines: &[String]) -> Cached {
        Cached {
            pid: info.pane_pid,
            activity: info.window_activity,
            command: info.current_command.clone(),
            title: info.pane_title.clone(),
            captured_at: now,
            lines: lines.to_vec(),
        }
    }

    fn needs_capture(&self, info: &flight_tmux::PaneInfo, now: u64) -> bool {
        let Some(c) = self.cache.get(&info.pane_id) else {
            return true;
        };
        c.pid != info.pane_pid
            || c.command != info.current_command
            || c.title != info.pane_title
            // Unknown activity, or activity not provably before the capture's second.
            || info.window_activity == 0
            || info.window_activity >= c.captured_at
            || c.activity > info.window_activity
            // A clock that went backwards, or a screen that has simply been kept too long.
            || now < c.captured_at
            || now - c.captured_at >= MAX_CAPTURE_AGE_SECS
    }
}
