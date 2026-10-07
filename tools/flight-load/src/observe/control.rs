// SPDX-License-Identifier: MIT

use super::transport::{capture_args, parse_list, PaneRow, Transport, LIST_FORMAT};
use flight_tmux::{ControlConnection, TmuxEndpoint};

/// Every command over one persistent control-mode connection (`flight-tmux`'s
/// `ControlConnection`, the one the node uses). Any connection error is an error here too;
/// `recover` opens a new connection.
pub struct Control {
    endpoint: TmuxEndpoint,
    connection: ControlConnection,
}

impl Control {
    /// Attach to the first session of the server on the named socket.
    pub fn start(socket: &str) -> Result<Self, String> {
        let endpoint = TmuxEndpoint::named(socket).map_err(|e| e.to_string())?;
        let connection = ControlConnection::open(&endpoint).map_err(|e| e.to_string())?;
        Ok(Self {
            endpoint,
            connection,
        })
    }

    #[cfg(test)]
    pub fn client_pid(&self) -> u32 {
        self.connection.client_pid()
    }
}

impl Transport for Control {
    fn list(&mut self) -> Result<Vec<PaneRow>, String> {
        let replies = self
            .connection
            .run(&[format!("list-panes -a -F '{LIST_FORMAT}'")])
            .map_err(|e| e.to_string())?;
        let reply = replies.into_iter().next().ok_or("no reply")?;
        if !reply.ok {
            return Err(format!("list-panes failed: {}", reply.lines.join(" ")));
        }
        parse_list(&reply.lines.join("\n"))
    }

    fn capture(&mut self, ids: &[String]) -> Result<Vec<Option<String>>, String> {
        let commands: Vec<String> = ids.iter().map(|id| capture_args(id).join(" ")).collect();
        Ok(self
            .connection
            .run(&commands)
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|r| r.ok.then(|| r.lines.join("\n")))
            .collect())
    }

    fn recover(&mut self) -> Result<(), String> {
        self.connection = ControlConnection::open(&self.endpoint).map_err(|e| e.to_string())?;
        Ok(())
    }
}
