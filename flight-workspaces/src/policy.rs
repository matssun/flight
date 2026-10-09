// SPDX-License-Identifier: MIT

/// What recovery may do. Everything that starts a process is off by default: looking at what
/// is saved and what is running, and reconnecting to what already runs, need no permission;
/// starting something does. There is no switch for touching the filesystem because there is no
/// such action (see `Action`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecoveryPolicy {
    /// Start surfaces that are saved but not running (a replacement process).
    pub start_missing: bool,
    /// Start surfaces from definitions that were imported rather than made here.
    pub start_imported: bool,
    /// Start an agent that was saved as running without permission prompts.
    pub start_skip_permissions: bool,
    /// Agent surfaces whose earlier session can be continued instead of replaced: the caller has
    /// checked that the provider supports it and holds a reference for the surface. A surface in
    /// this set is resumed (and, if that fails, reported, never silently replaced); one that is
    /// not is only ever reconnected or replaced. Leave it empty to always replace.
    pub resumable: std::collections::BTreeSet<crate::ConfigKey>,
}
