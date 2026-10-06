// SPDX-License-Identifier: MIT

/// `notification_type` of a `Notification` event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationType {
    PermissionPrompt,
    ElicitationDialog,
    IdlePrompt,
    Other,
}

impl NotificationType {
    pub fn from_wire(name: &str) -> Self {
        match name {
            "permission_prompt" => Self::PermissionPrompt,
            "elicitation_dialog" => Self::ElicitationDialog,
            "idle_prompt" => Self::IdlePrompt,
            _ => Self::Other,
        }
    }
}
