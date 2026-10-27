//! Reads the subagents each Claude Code session launched from the files Claude Code keeps beside
//! the session's transcript.
//!
//! For a session whose transcript is `<project>/<session>.jsonl`, Claude Code writes one
//! `agent-<id>.meta.json` (type, description, how it was launched, and `finished` once done) and
//! one `agent-<id>.jsonl` transcript per subagent under `<project>/<session>/subagents/`. Everything
//! here is blocking and runs off the main thread.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Deserialize;

/// A subagent whose transcript has not changed for this long is taken as dead: its session was
/// closed or interrupted before it finished. Generous, since a single long tool call (a build, a
/// test run) writes nothing until it returns.
const STALE_AFTER: Duration = Duration::from_secs(30 * 60);
/// Finished subagents stay listed this long, so a result that just landed is still visible.
const RECENT_FINISH: Duration = Duration::from_secs(10 * 60);
/// How much of the end of a transcript is read to find its last entry.
const TAIL_BYTES: u64 = 256 * 1024;
/// Prompts are cut to this many characters for display.
const MAX_PROMPT_CHARS: usize = 4000;

/// Where to look for one agent session's subagents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubagentSource {
    pub session_id: String,
    /// The session's own transcript, `<project>/<session>.jsonl`. Claude Code only reports it when
    /// a turn ends, so it is missing during a session's first turn.
    pub transcript_path: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubagentState {
    Running,
    Finished { at: SystemTime },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Subagent {
    pub id: String,
    /// The short mission Claude gave the subagent.
    pub description: String,
    pub agent_type: String,
    pub background: bool,
    pub started: SystemTime,
    pub state: SubagentState,
    /// The full request Claude passed to the subagent, cut to a displayable length.
    pub prompt: String,
}

impl Subagent {
    pub fn is_running(&self) -> bool {
        matches!(self.state, SubagentState::Running)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubagentMeta {
    #[serde(default)]
    agent_type: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    request_shape: String,
    #[serde(default)]
    finished: bool,
}

/// The running and recently finished subagents of each session, keyed by session id. Running ones
/// come first, oldest first; finished ones follow, most recent first. `projects_dir` is where
/// Claude Code keeps its projects, searched for sessions whose transcript path is not known yet.
pub fn scan(
    sources: &[SubagentSource],
    projects_dir: &Path,
    now: SystemTime,
) -> HashMap<String, Vec<Subagent>> {
    sources
        .iter()
        .filter_map(|source| {
            let dir = match &source.transcript_path {
                Some(transcript_path) => subagents_dir(transcript_path),
                None => find_subagents_dir(projects_dir, &source.session_id),
            }?;
            let mut subagents = read_subagents_dir(&dir, now);
            if subagents.is_empty() {
                return None;
            }
            sort_subagents(&mut subagents);
            Some((source.session_id.clone(), subagents))
        })
        .collect()
}

fn subagents_dir(transcript_path: &Path) -> Option<PathBuf> {
    let session_dir = transcript_path.with_extension("");
    Some(session_dir.join("subagents")).filter(|dir| dir.is_dir())
}

fn find_subagents_dir(projects_dir: &Path, session_id: &str) -> Option<PathBuf> {
    fs::read_dir(projects_dir)
        .ok()?
        .flatten()
        .map(|project| project.path().join(session_id).join("subagents"))
        .find(|dir| dir.is_dir())
}

fn read_subagents_dir(dir: &Path, now: SystemTime) -> Vec<Subagent> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let file_name = entry.file_name();
            let id = file_name.to_str()?.strip_suffix(".meta.json")?.to_owned();
            read_subagent(dir, id, now)
        })
        .collect()
}

fn read_subagent(dir: &Path, file_stem: String, now: SystemTime) -> Option<Subagent> {
    let meta_path = dir.join(format!("{file_stem}.meta.json"));
    let transcript_path = dir.join(format!("{file_stem}.jsonl"));
    let transcript_modified = fs::metadata(&transcript_path).ok()?.modified().ok()?;
    let since_last_write = now.duration_since(transcript_modified).unwrap_or_default();
    // Most subagents on disk ended long ago; skip them before reading anything else.
    if since_last_write > STALE_AFTER.max(RECENT_FINISH) {
        return None;
    }

    let meta_file = fs::metadata(&meta_path).ok()?;
    let meta: SubagentMeta = serde_json::from_slice(&fs::read(&meta_path).ok()?).ok()?;
    let started = meta_file
        .created()
        .or_else(|_| meta_file.modified())
        .unwrap_or(transcript_modified);

    let finished = meta.finished || last_entry_ends_turn(&transcript_path);
    let state = if finished {
        if since_last_write > RECENT_FINISH {
            return None;
        }
        SubagentState::Finished {
            at: transcript_modified,
        }
    } else {
        if since_last_write > STALE_AFTER {
            return None;
        }
        SubagentState::Running
    };

    Some(Subagent {
        id: file_stem
            .strip_prefix("agent-")
            .unwrap_or(&file_stem)
            .to_owned(),
        description: meta.description,
        agent_type: meta.agent_type,
        background: meta.request_shape == "background",
        started,
        state,
        prompt: read_prompt(&transcript_path).unwrap_or_default(),
    })
}

pub(super) fn sort_subagents(subagents: &mut [Subagent]) {
    subagents.sort_by(|a, b| match (a.state, b.state) {
        (SubagentState::Running, SubagentState::Running) => a.started.cmp(&b.started),
        (SubagentState::Running, SubagentState::Finished { .. }) => std::cmp::Ordering::Less,
        (SubagentState::Finished { .. }, SubagentState::Running) => std::cmp::Ordering::Greater,
        (SubagentState::Finished { at: a_at }, SubagentState::Finished { at: b_at }) => {
            b_at.cmp(&a_at)
        }
    });
}

/// Whether the transcript's last entry is the subagent's final answer. Older Claude Code versions
/// never set `finished` in the meta file, so this is what tells most finished subagents apart.
fn last_entry_ends_turn(transcript_path: &Path) -> bool {
    read_last_line(transcript_path).is_some_and(|line| line_ends_turn(&line))
}

pub(super) fn line_ends_turn(line: &str) -> bool {
    let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
        return false;
    };
    entry["type"] == "assistant" && entry["message"]["stop_reason"] == "end_turn"
}

/// The last complete line of `path`, or `None` when it is longer than [`TAIL_BYTES`]. Such lines
/// are tool results, never a final answer, so the caller treats them as "still running".
fn read_last_line(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail).ok()?;
    last_line_of_tail(&tail, start == 0)
}

pub(super) fn last_line_of_tail(tail: &[u8], is_whole_file: bool) -> Option<String> {
    let trimmed = tail.strip_suffix(b"\n").unwrap_or(tail);
    let line = match trimmed.iter().rposition(|byte| *byte == b'\n') {
        Some(newline) => &trimmed[newline + 1..],
        None if is_whole_file => trimmed,
        None => return None,
    };
    String::from_utf8(line.to_vec()).ok()
}

/// The request Claude passed to the subagent: the first entry of its transcript.
fn read_prompt(transcript_path: &Path) -> Option<String> {
    let file = File::open(transcript_path).ok()?;
    let mut first_line = String::new();
    BufReader::new(file.take(TAIL_BYTES))
        .read_line(&mut first_line)
        .ok()?;
    prompt_from_first_entry(&first_line)
}

pub(super) fn prompt_from_first_entry(line: &str) -> Option<String> {
    let entry: serde_json::Value = serde_json::from_str(line).ok()?;
    let content = &entry["message"]["content"];
    let text = match content {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| block["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => return None,
    };
    let text = text.trim();
    Some(match text.char_indices().nth(MAX_PROMPT_CHARS) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_owned(),
    })
}

#[cfg(test)]
#[path = "subagents_tests.rs"]
mod tests;
