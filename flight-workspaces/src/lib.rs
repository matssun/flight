// SPDX-License-Identifier: MIT

//! Durable workspaces (ADR-008): what the user declared, kept apart from what is observed
//! running and from how it is presented.
//!
//! - [`Document`] / [`Store`]: versioned, migratable, atomically saved definitions, with named
//!   profiles and snapshots. Owned by the node that owns the roots.
//! - [`RootSpec`] / [`RootProbe`] / [`RootState`]: verify a root without touching it; a missing,
//!   unmounted, unreadable or changed root is reported, never repaired.
//! - [`plan`] / [`recover`]: reconcile saved intent with observed state under a
//!   [`RecoveryPolicy`]. The [`Action`] set has no filesystem member.

mod atomic;
mod config_key;
mod document;
mod executor;
mod fs_probe;
mod git_marker;
mod load_error;
mod matching;
mod migrate;
#[cfg(test)]
mod migration_tests;
mod observed;
mod plan;
mod policy;
mod profile;
mod reconcile;
mod recover;
mod recovery_report;
mod root_identity;
mod root_probe;
mod root_spec;
mod root_state;
mod store;
mod store_error;
mod surface_spec;
mod transfer;
mod workspace_definition;

pub use config_key::ConfigKey;
pub use document::{Document, ProfileError, CURRENT_SCHEMA, DEFAULT_PROFILE};
pub use executor::{ExecError, Executor, Started};
pub use fs_probe::FsProbe;
pub use git_marker::GitMarker;
pub use load_error::LoadError;
pub use observed::{HostView, ObservedSurface, ObservedWorkspace, Observer};
pub use plan::{Action, Blocker, Health, Item, Plan, Refusal};
pub use policy::RecoveryPolicy;
pub use profile::Profile;
pub use reconcile::plan;
pub use recover::recover;
pub use recovery_report::{Outcome, RecoveryReport};
pub use root_identity::RootIdentity;
pub use root_probe::RootProbe;
pub use root_spec::{RootCheck, RootSpec};
pub use root_state::RootState;
pub use store::{Loaded, Store, StoreLock};
pub use store_error::StoreError;
pub use surface_spec::{SurfaceKind, SurfaceSpec};
pub use transfer::{export, import, ImportReport, MAX_IMPORT_BYTES, PORTABLE_HOST};
pub use workspace_definition::{Origin, WorkspaceDefinition};
