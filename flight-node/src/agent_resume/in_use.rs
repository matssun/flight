// SPDX-License-Identifier: MIT

use std::process::Command;

/// Whether some process on this machine was started with `token` on its command line: a session
/// that is still running somewhere must be reconnected to, not resumed, or two agents would
/// write into one conversation. Looks at every process of every user that `ps` shows.
pub(crate) fn session_in_use(token: &str) -> bool {
    let Ok(out) = Command::new("ps").args(["-axo", "args="]).output() else {
        // Not being able to look is not "free": say it is in use rather than risk two.
        return true;
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .any(|line| line.split_whitespace().any(|word| word == token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_running_process_with_the_token_as_an_argument_is_found() {
        let token = format!("probe-token-{}", std::process::id());
        let mut child = Command::new("sh")
            .args(["-c", "sleep 5; true", "probe", &token])
            .spawn()
            .unwrap();
        let found = session_in_use(&token);
        let _ = child.kill();
        let _ = child.wait();
        assert!(found);
        assert!(!session_in_use("a-token-nothing-runs-with-8f3a1c"));
    }
}
