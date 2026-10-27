//! Brings CLI agent conversations back after Warp restarts.
//!
//! Warp restores each pane's directory and scrollback, but a CLI agent that was running in it is
//! gone, and picking the right conversation back up by hand across a dozen tabs is error prone:
//! `claude --continue` resumes the most recent conversation in the directory, which is the wrong
//! one whenever several sessions share a checkout. Each pane therefore remembers the exact
//! conversation it was showing and replays its own launch command with `--resume <id>`.
//!
//! The replayed command is typed into the user's shell and persisted to disk, so it is rebuilt from
//! an allowlist rather than copied: nothing from the original line reaches the shell unless it is
//! a known flag with a plain value, and inline secrets such as `KEY=… claude` never get stored.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use warpui::{AppContext, SingletonEntity, ViewHandle};

use super::{CLIAgentSession, CLIAgentSessionContext, CLIAgentSessionsModel};
use crate::settings::AISettings;
use crate::terminal::{CLIAgent, TerminalView};

/// Flags that carry over to the resumed process unchanged.
const CARRIED_SWITCHES: &[&str] = &["--dangerously-skip-permissions"];

/// Flags whose value carries over to the resumed process.
const CARRIED_OPTIONS: &[&str] = &["--model", "--permission-mode"];

/// Flags that select which conversation to open and take a value, which must be dropped along with
/// them. `--resume` alone opens a picker, so its value is optional. Value-less selectors such as
/// `--continue` need no entry: every flag not carried over is dropped.
const SESSION_OPTIONS: &[&str] = &["--session-id", "-r", "--resume"];

/// Flags that make the agent run non-interactively. Such a process has nothing to resume into.
const NON_INTERACTIVE_SWITCHES: &[&str] = &["-p", "--print"];

/// What is needed to reopen a pane's CLI agent conversation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CLIAgentResume {
    pub agent: CLIAgent,
    pub session_id: String,
    pub launch: LaunchCommand,
    /// Shown as the tab's name until the agent reports a newer prompt, so restored tabs stay
    /// distinguishable instead of all falling back to a generic title.
    pub title: Option<String>,
    /// The agent's transcript file. A conversation whose transcript is gone cannot be resumed.
    pub transcript_path: Option<PathBuf>,
}

impl CLIAgentResume {
    /// Captures what is needed to resume `session`, or `None` if it cannot be resumed.
    ///
    /// `command` is the command line that started the agent.
    pub fn capture(session: &CLIAgentSession, command: &str) -> Option<Self> {
        if session.agent != CLIAgent::Claude || session.is_remote() {
            return None;
        }
        let CLIAgentSessionContext {
            session_id,
            transcript_path,
            ..
        } = &session.session_context;
        Some(Self {
            agent: session.agent,
            session_id: canonical_session_id(session_id.as_deref()?)?,
            launch: LaunchCommand::parse(command)?,
            title: session.session_context.display_title(),
            transcript_path: transcript_path.as_deref().map(PathBuf::from),
        })
    }

    /// The shell command that reopens this conversation, or `None` if it should not be resumed.
    ///
    /// Everything is validated again here because the stored value is read back from disk.
    pub fn command(&self) -> Option<String> {
        if self.agent != CLIAgent::Claude {
            return None;
        }
        if self
            .transcript_path
            .as_deref()
            .is_some_and(|path| !path.is_file())
        {
            return None;
        }
        let session_id = canonical_session_id(&self.session_id)?;
        if !self.launch.is_valid() {
            return None;
        }
        let mut parts = vec![self.launch.program.as_str()];
        parts.extend(self.launch.args.iter().map(String::as_str));
        parts.extend(["--resume", session_id.as_str()]);
        Some(parts.join(" "))
    }
}

/// Reopens `resume`'s conversation in a restored terminal once its shell is ready, if resuming is
/// enabled.
///
/// The terminal must have been restored into the directory the agent was started from, because
/// that is where the agent looks the conversation up.
pub fn resume_in_terminal(
    resume: &CLIAgentResume,
    terminal_view: &ViewHandle<TerminalView>,
    ctx: &mut AppContext,
) {
    if !*AISettings::as_ref(ctx).resume_cli_agents_on_restart {
        return;
    }
    let Some(command) = resume.command() else {
        return;
    };
    let terminal_view_id = terminal_view.id();
    CLIAgentSessionsModel::handle(ctx).update(ctx, |sessions, _| {
        sessions.expect_resumed_session(terminal_view_id, resume.clone());
    });
    terminal_view.update(ctx, |terminal_view, ctx| {
        // Locked because anything typed before the command runs would be appended to it, and text
        // after a space would reach the agent as its first prompt.
        terminal_view.set_locked_pending_command(&command, ctx);
    });
}

/// The program and the carried-over flags of the command that started an agent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchCommand {
    /// The binary or alias the user typed, so an alias that bakes in flags keeps working.
    pub program: String,
    pub args: Vec<String>,
}

impl LaunchCommand {
    /// Keeps only the allowlisted parts of `command`. Returns `None` when the command is not an
    /// interactive agent launch that can be safely replayed.
    pub fn parse(command: &str) -> Option<Self> {
        let tokens = shlex::split(command.trim())?;
        let (program, rest) = tokens.split_first()?;
        if !is_safe_program(program) {
            return None;
        }

        let mut args = Vec::new();
        let mut rest = rest.iter().peekable();
        while let Some(token) = rest.next() {
            let (flag, inline_value) = match token.split_once('=') {
                Some((flag, value)) if flag.starts_with("--") => (flag, Some(value)),
                _ => (token.as_str(), None),
            };

            if NON_INTERACTIVE_SWITCHES.contains(&flag) {
                return None;
            }
            let takes_value = CARRIED_OPTIONS.contains(&flag) || SESSION_OPTIONS.contains(&flag);
            let value = match inline_value {
                Some(value) => Some(value.to_owned()),
                None if takes_value => rest.next_if(|next| !next.starts_with('-')).cloned(),
                None => None,
            };

            if CARRIED_SWITCHES.contains(&flag) {
                args.push(flag.to_owned());
            } else if CARRIED_OPTIONS.contains(&flag)
                && let Some(value) = value.filter(|value| is_safe_value(value))
            {
                args.extend([flag.to_owned(), value]);
            }
        }

        Some(Self {
            program: program.clone(),
            args,
        })
    }

    /// Whether this holds only what [`Self::parse`] can produce.
    fn is_valid(&self) -> bool {
        if !is_safe_program(&self.program) {
            return false;
        }
        let mut args = self.args.iter();
        while let Some(arg) = args.next() {
            let is_valid = if CARRIED_SWITCHES.contains(&arg.as_str()) {
                true
            } else if CARRIED_OPTIONS.contains(&arg.as_str()) {
                args.next().is_some_and(|value| is_safe_value(value))
            } else {
                false
            };
            if !is_valid {
                return false;
            }
        }
        true
    }
}

/// Returns the hyphenated lowercase form of `session_id`, or `None` if it is not a UUID.
fn canonical_session_id(session_id: &str) -> Option<String> {
    Uuid::parse_str(session_id)
        .ok()
        .map(|id| id.hyphenated().to_string())
}

/// Accepts a bare command name or path. Rejects anything the shell would interpret, and
/// environment assignments such as `KEY=value`.
fn is_safe_program(program: &str) -> bool {
    !program.is_empty()
        && !program.starts_with('-')
        && program
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | '-'))
}

fn is_safe_value(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

#[cfg(test)]
#[path = "resume_on_restart_tests.rs"]
mod tests;
