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
    SourceCode { Hook = 1, Event = 2, Scrape = 3, Title = 4, Timeout = 5, SynthesizedDone = 6, Default = 7 }
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
    }
}

wire_enum! {
    /// Orchestrator's view of a node's connection.
    NodeStatusCode { Online = 1, Unreachable = 2 }
}
