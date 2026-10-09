// SPDX-License-Identifier: MIT

string_id! {
    /// The identity of one surface of a workspace (its agent, its shell). Not a tmux pane or
    /// window id: those belong to the backend and change when it restarts.
    SurfaceId
}
