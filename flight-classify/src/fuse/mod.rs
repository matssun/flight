// SPDX-License-Identifier: MIT

//! Fusion: weigh scrape, title, hook and event evidence into one final state, keeping
//! provenance. Ported from Fleet's engine (see THIRD_PARTY.md).

mod derive_events;
mod event_entry;
mod event_kind;
mod event_observation;
mod evidence;
mod fused;
mod fuser;
mod hook_observation;
mod notification_type;
mod scrape_slot;

pub use derive_events::derive_status_from_events;
pub use event_entry::EventEntry;
pub use event_kind::EventKind;
pub use event_observation::EventObservation;
pub use evidence::Evidence;
pub use fused::{Candidates, FusedClassification, Reason, ScrapeVia, Source};
pub use fuser::{fuse, WORKING_TIMEOUT_SECS};
pub use hook_observation::HookObservation;
pub use notification_type::NotificationType;
