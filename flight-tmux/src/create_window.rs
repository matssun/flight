// SPDX-License-Identifier: MIT

use crate::{
    CreateError, Launch, SurfaceMark, Tmux, TmuxRunner, CONFIG_SURFACE_OPTION, SURFACE_ID_OPTION,
    SURFACE_OPTION,
};
use std::thread::sleep;
use std::time::Duration;

const LAUNCH_CHECKS: u32 = 4;
const LAUNCH_CHECK_EVERY: Duration = Duration::from_millis(50);

impl<R: TmuxRunner> Tmux<R> {
    /// Add one marked window to an existing session, started in `dir`. Returns the window's
    /// id (`@N`). The session is addressed by its id and the window, once made, by its own, so
    /// nothing here can touch another session or window. The window is named after its kind
    /// and carries its identity from the first moment (one tmux invocation), so an observer
    /// never sees it unmarked. If anything fails afterwards the new window, and only it, is
    /// removed.
    pub fn create_surface_window(
        &self,
        session_id: &str,
        dir: &str,
        launch: &Launch,
        mark: &SurfaceMark,
    ) -> Result<String, CreateError> {
        let name = mark.kind.as_str();
        let target = format!("{session_id}:");
        let by_name = format!("{session_id}:={name}");
        let mut args: Vec<String> = [
            "new-window",
            "-d",
            "-P",
            "-F",
            "#{window_id}",
            "-t",
            &target,
            "-n",
            name,
            "-c",
            dir,
        ]
        .map(str::to_owned)
        .into();
        if let Launch::Program { argv } = launch {
            args.extend(argv.iter().cloned());
        }
        let mut options = vec![
            (SURFACE_OPTION, name),
            (SURFACE_ID_OPTION, mark.surface_id.as_str()),
        ];
        if let Some(config) = &mark.config {
            options.push((CONFIG_SURFACE_OPTION, config.surface.as_str()));
        }
        for (option, value) in options {
            args.extend(
                [";", "set-option", "-w", "-t", &by_name, option, value].map(str::to_owned),
            );
        }
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let id = match self.runner().run(&refs) {
            Ok(out) => out.stdout.trim().to_owned(),
            Err(e) => {
                // The window may exist without its marks; remove an unmarked one of our name.
                self.remove_unmarked_window(session_id, name);
                return Err(e.into());
            }
        };
        if !id.starts_with('@') {
            self.remove_unmarked_window(session_id, name);
            return Err(crate::TmuxError::Unparseable(id).into());
        }
        for check in 0..LAUNCH_CHECKS {
            if check > 0 {
                sleep(LAUNCH_CHECK_EVERY);
            }
            if !self.window_exists(&id) {
                return Err(CreateError::Exited);
            }
        }
        Ok(id)
    }

    fn window_exists(&self, window_id: &str) -> bool {
        self.runner()
            .run(&["display-message", "-p", "-t", window_id, "#{window_id}"])
            .is_ok_and(|o| o.stdout.trim() == window_id)
    }

    /// Best effort: kill windows of this session named `name` that carry no surface id.
    fn remove_unmarked_window(&self, session_id: &str, name: &str) {
        let Ok(out) = self.runner().run(&[
            "list-windows",
            "-t",
            session_id,
            "-F",
            "#{window_id}\t#{window_name}\t#{@flight_surface_id}",
        ]) else {
            return;
        };
        for line in out.stdout.lines() {
            let mut f = line.split('\t');
            if let (Some(id), Some(n), Some("")) = (f.next(), f.next(), f.next()) {
                if n == name {
                    let _ = self.runner().run(&["kill-window", "-t", id]);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SurfaceTag, TmuxError, TmuxOutput};
    use std::cell::RefCell;

    /// A scripted tmux that records every call.
    struct Script {
        calls: RefCell<Vec<Vec<String>>>,
        /// `new-window` fails with this.
        new_window_fails: Option<&'static str>,
        /// What `list-windows` says (window id, name, surface id per line).
        windows: &'static str,
        /// The window id `display-message` reports back; empty when the window is gone.
        alive: &'static str,
    }

    impl Script {
        fn new() -> Self {
            Self {
                calls: RefCell::default(),
                new_window_fails: None,
                windows: "",
                alive: "@9",
            }
        }
    }

    impl TmuxRunner for &Script {
        fn run(&self, args: &[&str]) -> Result<TmuxOutput, TmuxError> {
            self.calls
                .borrow_mut()
                .push(args.iter().map(|s| (*s).to_owned()).collect());
            let stdout = match args[0] {
                "new-window" => match self.new_window_fails {
                    Some(stderr) => {
                        return Err(TmuxError::Failed {
                            code: Some(1),
                            stderr: stderr.to_owned(),
                        })
                    }
                    None => "@9\n",
                },
                "list-windows" => self.windows,
                "display-message" => self.alive,
                _ => "",
            };
            Ok(TmuxOutput {
                stdout: stdout.to_owned(),
            })
        }
    }

    fn mark() -> SurfaceMark {
        SurfaceMark {
            workspace_id: "w-1".into(),
            surface_id: "s-2".into(),
            kind: SurfaceTag::Shell,
            config: None,
        }
    }

    fn killed(script: &Script) -> Vec<Vec<String>> {
        script
            .calls
            .borrow()
            .iter()
            .filter(|c| c[0] == "kill-window")
            .cloned()
            .collect()
    }

    #[test]
    fn the_window_is_made_and_marked_by_one_invocation_addressed_by_session_id() {
        let script = Script::new();
        let id = Tmux::with_runner(&script)
            .create_surface_window("$3", "/work", &Launch::DefaultShell, &mark())
            .unwrap();
        assert_eq!(id, "@9");
        let calls = script.calls.borrow();
        let first = &calls[0];
        assert_eq!(first[0], "new-window");
        assert!(first.windows(2).any(|w| w == ["-t", "$3:"]));
        assert!(first.windows(2).any(|w| w == ["-c", "/work"]));
        // The marks ride in the same invocation, so no observer sees the window without them.
        assert_eq!(first.iter().filter(|a| *a == ";").count(), 2);
        assert!(first.contains(&"@flight_surface".to_owned()));
        assert!(first.contains(&"@flight_surface_id".to_owned()));
        assert!(first.contains(&"s-2".to_owned()));
    }

    #[test]
    fn a_failure_removes_only_an_unmarked_window_of_the_kind_and_name() {
        let script = Script {
            new_window_fails: Some("boom"),
            // A leftover unmarked `shell`, the marked agent, and the user's own `shell`-named
            // window that carries another surface id.
            windows: "@9\tshell\t\n@1\tagent\ts-1\n@4\tshell\ts-other\n",
            ..Script::new()
        };
        let err = Tmux::with_runner(&script)
            .create_surface_window("$3", "/work", &Launch::DefaultShell, &mark())
            .unwrap_err();
        assert!(matches!(err, CreateError::Tmux(_)));
        assert_eq!(
            killed(&script),
            vec![vec!["kill-window".to_owned(), "-t".into(), "@9".into()]],
            "only the orphan"
        );
    }

    #[test]
    fn a_shell_that_ends_at_once_is_an_exit_and_nothing_is_reported_as_created() {
        let script = Script {
            alive: "",
            ..Script::new()
        };
        let err = Tmux::with_runner(&script)
            .create_surface_window("$3", "/work", &Launch::DefaultShell, &mark())
            .unwrap_err();
        assert!(matches!(err, CreateError::Exited), "{err}");
    }
}
