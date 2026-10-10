// SPDX-License-Identifier: MIT

use flight_state::SurfaceId;
use flight_ui::SurfaceChoice;

/// What a workspace's agent and shell are called in a layout. The one place that says so.
pub fn surface_id(choice: SurfaceChoice) -> SurfaceId {
    SurfaceId::new(match choice {
        SurfaceChoice::Agent => "agent",
        SurfaceChoice::Shell => "shell",
    })
}

/// The inverse: which of the workspace's surfaces a layout's name is, if it is one.
pub fn surface_choice(surface: &SurfaceId) -> Option<SurfaceChoice> {
    match surface.as_str() {
        "agent" => Some(SurfaceChoice::Agent),
        "shell" => Some(SurfaceChoice::Shell),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_names_round_trip_and_nothing_else_is_a_surface() {
        for choice in [SurfaceChoice::Agent, SurfaceChoice::Shell] {
            assert_eq!(surface_choice(&surface_id(choice)), Some(choice));
        }
        assert_eq!(surface_choice(&SurfaceId::new("logs")), None);
    }
}
