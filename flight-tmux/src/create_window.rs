// SPDX-License-Identifier: MIT

use crate::{
    CreateError, Launch, SurfaceMark, Tmux, TmuxRunner, SURFACE_ID_OPTION, SURFACE_OPTION,
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
        for (option, value) in [
            (SURFACE_OPTION, name),
            (SURFACE_ID_OPTION, mark.surface_id.as_str()),
        ] {
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
