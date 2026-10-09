<!-- SPDX-License-Identifier: MIT -->

# ADR-010: Continuing an agent's earlier session

Status: implemented for Claude Code; Codex is explicitly unsupported. Scope: a node continues an agent it started, in the directory and as the user it was started in. Not in scope: discovering sessions Flight did not start, resuming across hosts, forking a conversation, anything that reads a conversation's content.

## Why this is its own decision

Replacing a stopped agent and continuing its conversation are different acts with different risks. A replacement is a new agent that knows nothing; a continuation carries a conversation (which may hold sensitive material) and, in some providers, a permission mode. Presenting one as the other is wrong in both directions. So: a reference to a session is opaque, made by the node, never read from the screen, kept apart from the definitions, scoped, checked before use, and a failed continuation is reported and replaces nothing.

## What the providers offer (checked against the installed CLIs, 2026-10)

| | Claude Code 2.1.295 | Codex CLI 0.148.0 |
|---|---|---|
| Choose the session id at launch | yes: `--session-id <uuid>` (a UUID that is not in use; the CLI refuses a used one: "Session ID … is already in use") | no: ids are assigned by the tool |
| Continue a session | `--resume <uuid>` (continues the same id unless `--fork-session`); an unknown id: "No conversation found with session ID: …" and exit | `codex resume <id>` / `--last` |
| Learn the id of an interactive session | not needed, Flight chose it | only through its own picker; no documented command, hook or output for the id of an interactive session |
| Where conversations live | `~/.claude/projects/<cwd with every non-ASCII-alphanumeric character replaced by '-'>/<id>.jsonl` (`CLAUDE_CONFIG_DIR` moves it); per directory | rollout files, location not documented |
| Permission mode on resume | `bypassPermissions` is **not** restored; `plan` and `auto` are; `--permission-mode` overrides | not documented for resume |

Verified by running the CLI (two prompts, one new session and one continued, in a scratch directory that was removed afterwards): a session started with `--session-id` writes `<id>.jsonl` under the project directory named as above (for the real, symlink-resolved directory), `--resume` of it remembers the conversation, `--resume` of an unknown id says so, and a second `--session-id` with the same id is refused.

**Decision.** Claude is supported through an identity the node chooses. Codex is unsupported, and `resume_support("codex")` says why. Any provider not listed is unsupported. Reading Codex's rollout files or screen-scraping an id would be guessing, so it is not done.

## The reference

`ResumeRef { provider, token, scope: { host, root, user } }` in `flight-workspaces`, with no provider logic:

- **Opaque.** Flight never looks inside the token; only the provider adapter interprets it. It is validated as plain text of a sane length (it ends up on a command line) and never printed (`Debug` and `Display` show four characters at most).
- **Made, not found.** The node mints it before the agent starts and starts the agent with it (`claude --session-id <uuid>`). Nothing is parsed from terminal text.
- **Kept apart.** In `resume.toml` beside `workspaces.toml`, mode 0600, replaced atomically, under the same node lock. Not in the definition file: definitions are exported, shared and imported, and the token is the key to a conversation history. `export` carries none (tested). A file from a newer Flight, or unreadable, is left exactly as found and nothing is resumed meanwhile; agents still start.
- **Scoped.** Good only for the host, the canonical (symlink-resolved) root and the operating-system user it was made for. A different one is refused. Changing a workspace's root drops its references (a conversation belongs to the directory it was held in); forgetting a workspace drops them too.
- **Replaced with the agent.** A replacement agent is a new session; its reference replaces the old one.

## Continuing, and not continuing

`Action::ResumeWorkspace` is planned for a stopped workspace whose agent surface has a reference (`RecoveryPolicy::resumable`, filled by the node from its store and `resume_support`). It passes the same gates as starting (verified root, trusted import, **skip-permissions agents are not started or resumed unprompted**), and then, in the executor, before anything is started:

1. the reference is for this provider, host, root and user;
2. Claude still has the conversation: `<config>/projects/<name of the root>/<token>.jsonl` exists and has content (a root whose name cannot be matched with certainty, such as a non-ASCII or very long path, cannot be verified and is not resumed);
3. no process on this machine was started with that token (a session still running is reconnected to, never resumed; two agents would write into one conversation).

Then `claude --resume <token> --permission-mode default` is started in the root. **The mode is set, not inherited**: the provider restores `plan` and `auto` from the earlier run, and a person who resumed after a restart did not choose those; the resumed session asks before acting. Only a saved definition that says the agent runs without asking brings `--dangerously-skip-permissions` back, and recovery refuses to start such an agent unprompted in the first place. If Claude exits as soon as it starts, nothing was created and the error says the earlier session could not be continued.

**A failed continuation is a refusal, not a replacement.** `Outcome::Refused("cannot continue the earlier conversation (…); nothing was started and the saved workspace was kept")`: the definition and the reference are untouched, nothing runs. Starting a new conversation is a separate, explicit choice (`SavedAction::RestoreFresh`), and it says what it is: a new session, a new reference. The recovery report distinguishes `Resumed` from `Started`, and so does the node log.

## Reconnect and resume are different

A running agent process is reconnected to: nothing is started, no reference is used. Resumption exists only when the process is gone (the tmux server, the node's machine or the agent itself ended). The check in 3 above is what keeps the two apart when something outside Flight is still running the session.

## Sensitivity, stated plainly

A token lets a process on the same machine and user reopen a conversation, with its history, by `claude --resume`. That is the same access the user already has to `~/.claude`. It is not a network credential and cannot be used from another machine or user, and the scope check enforces both. It is nevertheless kept private (0600, own file, absent from exports and logs).

## Consequences and limits

- Agents created before this change have no reference; they are replaced as before, and the dashboard (once the wire carries it) says "new conversation".
- Sessions started outside Flight are not discovered or adopted.
- Resuming starts a model session that may re-read a long history; the provider's own cost and cache behaviour apply.
- A non-ASCII or very long root cannot be matched to the provider's records and is not resumed.
- Tested against a fake `claude` that behaves as the real one was observed to; the real CLI was exercised by hand once, for the facts above, not in CI.
