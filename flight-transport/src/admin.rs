// SPDX-License-Identifier: MIT

//! A local operator socket for a running orchestrator: a Unix socket, owner-only, in the
//! orchestrator's private directory. Enrollment tokens live in the orchestrator's memory,
//! so creating one is a request to that process, not a file edit.
//!
//! One request line, one reply: `ok [text]` lines, or `err <message>`.
//!
//! ```text
//! enroll [ttl_secs]             -> ok <bundle>
//! trust                         -> ok, then `peer <id> <role> <enabled|disabled> <name>` lines
//! revoke <id>                   -> ok
//! status                        -> ok nodes=<n> backlog=<n>
//! ```

use crate::{ServerControl, TransportError};
use flight_trust::{EnrollmentBundle, Fingerprint, Role, DEFAULT_TTL_SECS};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::task::JoinHandle;

const MAX_LINE: u64 = 1024;

/// Serve the admin socket at `path`; `advertise` is the address nodes will be told to dial.
pub fn serve_admin(
    path: PathBuf,
    control: ServerControl,
    advertise: String,
) -> Result<JoinHandle<()>, TransportError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
        restrict(dir, 0o700);
    }
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    restrict(&path, 0o600);
    Ok(tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let (control, advertise) = (control.clone(), advertise.clone());
            tokio::spawn(async move {
                let _ = handle(stream, control, advertise).await;
            });
        }
    }))
}

#[cfg(unix)]
fn restrict(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
}

async fn handle(
    stream: UnixStream,
    control: ServerControl,
    advertise: String,
) -> std::io::Result<()> {
    let (read, mut write) = stream.into_split();
    let mut line = String::new();
    BufReader::new(read.take(MAX_LINE))
        .read_line(&mut line)
        .await?;
    let reply = match run(line.trim(), &control, &advertise) {
        Ok(text) => format!("ok{}{}\n", if text.is_empty() { "" } else { " " }, text),
        Err(e) => format!("err {e}\n"),
    };
    write.write_all(reply.as_bytes()).await
}

fn run(line: &str, control: &ServerControl, advertise: &str) -> Result<String, String> {
    let mut words = line.split_whitespace();
    match words.next() {
        Some("enroll") => {
            let ttl = match words.next() {
                Some(t) => t
                    .parse::<u64>()
                    .map_err(|_| "ttl must be seconds".to_owned())?,
                None => DEFAULT_TTL_SECS,
            };
            let issued = control.create_enrollment(ttl).map_err(|e| e.to_string())?;
            let bundle = EnrollmentBundle {
                orchestrator: control.fingerprint(),
                address: advertise.to_owned(),
                token: issued.secret,
                expires_at: issued.expires_at,
            };
            // The role is not part of the bundle: the joining side states what it is.
            Ok(bundle.to_string())
        }
        Some("trust") => {
            let lines: Vec<String> = control
                .trust()
                .peers()
                .iter()
                .map(|p| {
                    format!(
                        "peer {} {} {} {}",
                        p.id,
                        if p.role == Role::Ui { "ui" } else { "node" },
                        if p.enabled { "enabled" } else { "disabled" },
                        p.display_name
                    )
                })
                .collect();
            Ok(lines.join("\n"))
        }
        Some("revoke") => {
            let id = words.next().ok_or("revoke needs an id")?;
            let id = Fingerprint::parse(id).map_err(|e| e.to_string())?;
            control.revoke(&id).map_err(|e| e.to_string())?;
            Ok(String::new())
        }
        Some("status") => Ok(format!(
            "nodes={} backlog={}",
            control.fleet_snapshot().nodes.len(),
            control.max_backlog()
        )),
        _ => Err("unknown command".to_owned()),
    }
}

/// Send one request to the admin socket at `path` and return the reply text.
pub async fn admin_request(path: &Path, request: &str) -> Result<String, TransportError> {
    let mut stream = UnixStream::connect(path).await.map_err(|e| {
        TransportError::Connect(format!(
            "no orchestrator is listening on {} ({e}); is `flight orchestrator run` running?",
            path.display()
        ))
    })?;
    stream.write_all(format!("{request}\n").as_bytes()).await?;
    stream.shutdown().await?;
    let mut reply = String::new();
    BufReader::new(stream).read_to_string(&mut reply).await?;
    match reply.strip_prefix("ok") {
        Some(rest) => Ok(rest.trim_start_matches(' ').trim_end().to_owned()),
        None => Err(TransportError::Refused(
            reply
                .strip_prefix("err ")
                .unwrap_or(&reply)
                .trim()
                .to_owned(),
        )),
    }
}
