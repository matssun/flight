// SPDX-License-Identifier: MIT

//! Wire enums. Numbers are part of the contract: never renumber, only add.

wire_enum! {
    /// Which agent a pane runs. `Other` is any agent without a detection manifest.
    AgentKindCode { Claude = 1, Codex = 2, OpenCode = 3, Pi = 4, Other = 5 }
}

wire_enum! {
    /// The seven agent states (mirrors `flight_state::AgentState`).
    StateCode { Permit = 1, Question = 2, Done = 3, Busy = 4, Idle = 5, Shell = 6, Down = 7 }
}

wire_enum! {
    /// A coarse summary of where the state came from. Rule-level detail stays in the node.
    SourceCode { Hook = 1, Event = 2, Scrape = 3, Title = 4, Timeout = 5, SynthesizedDone = 6, Default = 7, Glyph = 8 }
}

wire_enum! {
    /// Why a request failed (mirrors the host error kinds, plus protocol-level failures).
    ErrorKindCode {
        TmuxUnavailable = 1,
        TmuxServerUnavailable = 2,
        RemoteCommandFailed = 3,
        Unsupported = 4,
        NotAuthorized = 5,
        ProtocolMismatch = 6,
        UnknownPane = 7,
        InvalidRequest = 8,
        NodeUnreachable = 9,
        PaneChanged = 10,
        Busy = 11,
        AlreadyExists = 12,
        InvalidDirectory = 13,
        ProgramUnavailable = 14,
        UnknownWorkspace = 15,
    }
}

wire_enum! {
    /// What a new session runs: a closed set, never a command line.
    ProgramCode { Claude = 1, Shell = 2, ClaudeSkipPermissions = 3 }
}

wire_enum! {
    /// What a surface of a workspace is: a closed set.
    SurfaceKindCode { Agent = 1, Shell = 2 }
}

wire_enum! {
    /// The health of a saved workspace as its node sees it (ADR-008).
    SavedHealthCode { Running = 1, Partial = 2, Stopped = 3, Blocked = 4 }
}

wire_enum! {
    /// What looking at a saved workspace's root found (ADR-008).
    SavedRootCode {
        Verified = 1,
        FirstSighting = 2,
        Missing = 3,
        NotADirectory = 4,
        PermissionDenied = 5,
        Unverified = 6,
        Changed = 7,
    }
}

wire_enum! {
    /// What a user asks of a saved workspace (ADR-008).
    SavedActionCode { Retry = 1, Remove = 2, Restore = 3, AcceptRoot = 4, SetRoot = 5, Trust = 6 }
}

wire_enum! {
    /// Why a terminal session ended (ADR-003).
    ExitReasonCode {
        ClientExited = 1,
        StartFailed = 2,
        ClosedByUi = 3,
        NodeLost = 4,
        Stalled = 5,
        Revoked = 6,
        Shutdown = 7,
        LeaseExpired = 8,
    }
}

wire_enum! {
    /// What an enrolling identity is for.
    RoleCode { Node = 1, Ui = 2 }
}

wire_enum! {
    /// The orchestrator's view of a node's connection: liveness only. It never rewrites the
    /// semantic state of the node's panes, which stay as last reported.
    NodeStatusCode { Online = 1, Disconnected = 2, Stale = 3 }
}
