// SPDX-License-Identifier: MIT

use super::{new_id, Program, SessionEnv, SessionRequest};
use crate::ControlError;
use flight_proto::{valid_dir, valid_session_name, ErrorKindCode};
use flight_tmux::{
    ConfigMark, CreateError, Launch, NewSession, SurfaceMark, SurfaceTag, Tmux, TmuxRunner,
};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const CLAUDE: &str = "claude";

fn refuse(kind: ErrorKindCode, message: impl Into<String>) -> ControlError {
    ControlError::new(kind, message)
}

/// Create the session. Every check that can fail without touching tmux happens first; what
/// tmux does after that is [`Tmux::create_session`]'s to undo.
pub(crate) fn create<R: TmuxRunner>(
    tmux: &Tmux<R>,
    request: &SessionRequest,
    env: &SessionEnv,
    config: Option<ConfigMark>,
    tmux_failure: impl Fn(flight_tmux::TmuxError) -> ControlError,
) -> Result<SurfaceMark, ControlError> {
    if !valid_session_name(&request.name) {
        return Err(refuse(
            ErrorKindCode::InvalidRequest,
            "invalid session name",
        ));
    }
    if !valid_dir(&request.dir) {
        return Err(refuse(ErrorKindCode::InvalidRequest, "invalid directory"));
    }
    let dir = existing_dir(&request.dir, env)?;
    let launch = match request.program {
        Program::Shell => Launch::DefaultShell,
        Program::Claude | Program::ClaudeSkipPermissions => Launch::Program {
            // Found on the node's PATH, then run by absolute path. tmux gives the session the
            // PATH of its client, which is this process: the node's environment, not
            // whatever the tmux server was started with.
            argv: claude_argv(request.program, find_program(CLAUDE, env)?),
        },
    };
    // The new session is a workspace: it gets an identity of its own, kept in the backend, and
    // its first window is the surface the program makes.
    let mark = SurfaceMark {
        workspace_id: new_id('w')?,
        surface_id: new_id('s')?,
        kind: match request.program {
            Program::Shell => SurfaceTag::Shell,
            Program::Claude | Program::ClaudeSkipPermissions => SurfaceTag::Agent,
        },
        config,
    };
    let spec = NewSession {
        name: request.name.clone(),
        dir: dir.to_string_lossy().into_owned(),
        launch,
        mark: Some(mark.clone()),
    };
    match tmux.create_session(&spec) {
        Ok(_id) => Ok(mark),
        Err(CreateError::AlreadyExists) => Err(refuse(
            ErrorKindCode::AlreadyExists,
            format!("a session named {} already exists", request.name),
        )),
        Err(CreateError::Exited) => Err(refuse(
            ErrorKindCode::ProgramUnavailable,
            format!(
                "{} exited as soon as it started; nothing was created. Check that it runs \
                 in the node's own environment (login, PATH)",
                program_name(request.program)
            ),
        )),
        Err(CreateError::Tmux(e)) => Err(tmux_failure(e)),
    }
}

/// The one flag the closed set can add; no other text ever reaches the command line.
fn claude_argv(program: Program, claude: PathBuf) -> Vec<String> {
    let mut argv = vec![claude.to_string_lossy().into_owned()];
    if program == Program::ClaudeSkipPermissions {
        argv.push("--dangerously-skip-permissions".to_owned());
    }
    argv
}

fn program_name(program: Program) -> &'static str {
    match program {
        Program::Claude | Program::ClaudeSkipPermissions => CLAUDE,
        Program::Shell => "the shell",
    }
}

/// The directory on this node, `~` expanded, only if it exists and is a directory. Nothing is
/// ever created.
fn existing_dir(dir: &str, env: &SessionEnv) -> Result<PathBuf, ControlError> {
    let invalid = |m: String| refuse(ErrorKindCode::InvalidDirectory, m);
    let path = match dir.strip_prefix('~') {
        Some(rest) => {
            let home = env
                .home
                .as_ref()
                .ok_or_else(|| invalid("this node has no home directory for ~".to_owned()))?;
            home.join(rest.trim_start_matches('/'))
        }
        None => PathBuf::from(dir),
    };
    match std::fs::metadata(&path) {
        Ok(m) if m.is_dir() => Ok(path),
        Ok(_) => Err(invalid(format!("{dir} is not a directory"))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(invalid(format!("{dir} does not exist on this node")))
        }
        Err(e) => Err(invalid(format!("cannot use {dir}: {}", e.kind()))),
    }
}

/// The first executable file called `name` on the node's `PATH`, as an absolute path.
fn find_program(name: &str, env: &SessionEnv) -> Result<PathBuf, ControlError> {
    std::env::split_paths(&env.path)
        .filter(|d| d.is_absolute())
        .map(|d| d.join(name))
        .find(|p| is_executable(p))
        .ok_or_else(|| {
            refuse(
                ErrorKindCode::ProgramUnavailable,
                format!("{name} was not found on this node's PATH"),
            )
        })
}

fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}
