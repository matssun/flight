// SPDX-License-Identifier: MIT

use flight_tmux::{tmux_args, TmuxEndpoint};

/// The tmux invocation that attaches a client to `pane`, but only if the pane still runs
/// `pid`. The pid check, the selection and the attach are one tmux command queue, so there is
/// no window between "still the same process" and "client attached". A wrong pid exits 1
/// without ever attaching.
///
/// `pane` is a pane id from the node's own tmux listing and `pid` an integer; no string from
/// a peer is placed in the command. No `-d` (other clients are never detached) and no `-r`.
pub fn tmux_attach_command(endpoint: &TmuxEndpoint, pane: &str, pid: u32) -> Vec<String> {
    let attach =
        format!("select-window -t {pane} ; select-pane -t {pane} ; attach-session -t {pane}");
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
    fn the_guard_and_the_attach_are_one_tmux_command() {
        let ep = TmuxEndpoint::named("flight").unwrap();
        let args = tmux_attach_command(&ep, "%7", 4321);
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
                "select-window -t %7 ; select-pane -t %7 ; attach-session -t %7",
                "run-shell 'exit 1'",
            ]
        );
        assert!(!args.iter().any(|a| a == "-d" || a == "-r"));
    }
}
