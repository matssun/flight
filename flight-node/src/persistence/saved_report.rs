// SPDX-License-Identifier: MIT

use flight_proto::{
    SavedHealthCode, SavedResumeCode, SavedRootCode, SavedWorkspace, MAX_DETAIL_LEN, MAX_SAVED,
};
use flight_workspaces::{Blocker, Health, Item, RootCheck, RootState, WorkspaceDefinition};

/// What starting a saved workspace again would do about its agent's conversation.
pub(crate) struct ResumeStatus {
    pub code: SavedResumeCode,
    pub detail: String,
}

/// One saved workspace as the wire carries it. Pure: the planner has already done the looking.
pub(crate) fn saved_workspace(
    def: &WorkspaceDefinition,
    item: &Item,
    resume: ResumeStatus,
) -> SavedWorkspace {
    let (health, ambiguity) = match &item.health {
        Health::Running => (SavedHealthCode::Running, None),
        Health::Partial => (SavedHealthCode::Partial, None),
        Health::Stopped => (SavedHealthCode::Stopped, None),
        Health::Blocked(Blocker::Ambiguous(ids)) => (
            SavedHealthCode::Blocked,
            Some(format!(
                "more than one running workspace could be this one ({}); not choosing",
                ids.join(", ")
            )),
        ),
        Health::Blocked(_) => (SavedHealthCode::Blocked, None),
    };
    let (root_state, why) = root(&item.root);
    SavedWorkspace {
        config_key: def.key.to_string(),
        name: def.name.clone(),
        root: def.root.path.clone(),
        health: health as i32,
        root_state: root_state as i32,
        detail: bounded(ambiguity.or(why).unwrap_or_default()),
        workspace_id: item.runtime.clone().unwrap_or_default(),
        imported: def.origin == flight_workspaces::Origin::Imported,
        resume: resume.code as i32,
        resume_detail: bounded(resume.detail),
    }
}

fn root(check: &RootCheck) -> (SavedRootCode, Option<String>) {
    match check {
        RootCheck::Verified => (SavedRootCode::Verified, None),
        RootCheck::FirstSighting => (SavedRootCode::FirstSighting, None),
        RootCheck::Changed(why) => (SavedRootCode::Changed, Some(why.clone())),
        RootCheck::Unavailable(state) => match state {
            RootState::Missing => (SavedRootCode::Missing, None),
            RootState::NotADirectory => (SavedRootCode::NotADirectory, None),
            RootState::PermissionDenied => (SavedRootCode::PermissionDenied, None),
            RootState::Unverified { reason } | RootState::HostUnreachable { reason } => {
                (SavedRootCode::Unverified, Some(reason.clone()))
            }
            // Cannot happen (a present root is not unavailable); say so rather than guess.
            RootState::Present { .. } => (
                SavedRootCode::Unverified,
                Some("the root could not be read".to_owned()),
            ),
        },
    }
}

fn bounded(mut text: String) -> String {
    if text.len() > MAX_DETAIL_LEN {
        let mut cut = MAX_DETAIL_LEN;
        while !text.is_char_boundary(cut) {
            cut = cut.saturating_sub(1);
        }
        text.truncate(cut);
    }
    text
}

/// At most [`MAX_SAVED`] entries go on the wire; the rest stay saved and visible locally.
pub(crate) fn capped(mut list: Vec<SavedWorkspace>) -> Vec<SavedWorkspace> {
    list.truncate(MAX_SAVED);
    list
}
