// SPDX-License-Identifier: MIT

use flight_tmux::{tmux_args, TmuxEndpoint};

/// The tmux invocation that shows `pane` in a client of its own, but only if the pane still
/// runs `pid`.
///
/// The client does not attach to the pane's session. It attaches to a *view*: a new session in
/// the workspace's session group (ADR-009), named `view`, whose current window and pane are the
/// ones asked for. The windows are shared, so the surface is the same one, but the current
/// window is not: a terminal showing the shell does not move a terminal showing the agent, and
/// keys typed in one can only reach the window that terminal shows. The view removes itself when
/// its client goes (`destroy-unattached`, set once a client is attached: before that, tmux
/// would remove a session nobody has attached yet).
///
/// The pid check and everything after it are one tmux command queue, so there is no window
/// between "still the same process" and "client attached". A wrong pid exits 1 without creating
/// anything. `pane` and `window` are ids from the node's own tmux listing, `pid` an integer and
/// `view` a name made from hex digits; no string from a peer is placed in the command. No `-d`
/// on the attach (other clients are never detached) and no `-r`.
pub fn tmux_attach_command(
    endpoint: &TmuxEndpoint,
    pane: &str,
    window: &str,
    pid: u32,
    view: &str,
) -> Vec<String> {
    let attach = format!(
        "new-session -d -t {pane} -s {view} ; select-window -t ={view}:{window} ; \
         select-pane -t ={view}:{window}.{pane} ; attach-session -t ={view} ; \
         set-option -t {view} destroy-unattached on"
    );
    let guard = format!("#{{==:#{{pane_pid}},{pid}}}");
    tmux_args(
        endpoint,
        &[
            "if-shell",
            "-t",
            pane,
            "-F",
            &guard,
            &attach,
            "run-shell 'exit 1'",
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reserved_prefix_is_the_prefix_views_are_made_with() {
        assert_eq!(
            flight_state::RESERVED_VIEW_PREFIX,
            flight_tmux::VIEW_SESSION_PREFIX
        );
    }

    #[test]
    fn the_guard_and_the_attach_are_one_tmux_command() {
        let ep = TmuxEndpoint::named("flight").unwrap();
        let args = tmux_attach_command(&ep, "%7", "@3", 4321, "flight-view-ab");
        assert_eq!(
            args,
            [
                "-u",
                "-L",
                "flight",
                "if-shell",
                "-t",
                "%7",
                "-F",
                "#{==:#{pane_pid},4321}",
                "new-session -d -t %7 -s flight-view-ab ; select-window -t =flight-view-ab:@3 ; \
                 select-pane -t =flight-view-ab:@3.%7 ; attach-session -t =flight-view-ab ; \
                 set-option -t flight-view-ab destroy-unattached on",
                "run-shell 'exit 1'",
            ]
        );
        // The only `-d` is the one that creates the view detached; the client never detaches
        // another, and is not read-only.
        assert!(!args.iter().any(|a| a == "-d" || a == "-r"));
        assert!(!args[8].contains("attach-session -d"));
    }
}
