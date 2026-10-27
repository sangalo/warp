use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use super::{
    SubagentSource, SubagentState, last_line_of_tail, line_ends_turn, prompt_from_first_entry, scan,
};

const PROMPT_LINE: &str =
    r#"{"type":"user","message":{"role":"user","content":"Investigue os bugs de Pix."}}"#;
const TOOL_USE_LINE: &str =
    r#"{"type":"assistant","message":{"stop_reason":"tool_use","content":[{"type":"tool_use"}]}}"#;
const END_TURN_LINE: &str =
    r#"{"type":"assistant","message":{"stop_reason":"end_turn","content":[{"type":"text"}]}}"#;

/// Writes a subagent's meta and transcript files the way Claude Code lays them out, and returns
/// the session's source.
fn write_subagent(
    project: &Path,
    id: &str,
    meta: &str,
    transcript_lines: &[&str],
) -> SubagentSource {
    let transcript_path = project.join("session-1.jsonl");
    let dir = project.join("session-1").join("subagents");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(format!("agent-{id}.meta.json")), meta).unwrap();
    fs::write(
        dir.join(format!("agent-{id}.jsonl")),
        transcript_lines.join("\n") + "\n",
    )
    .unwrap();
    SubagentSource {
        session_id: "session-1".to_owned(),
        transcript_path: Some(transcript_path),
    }
}

#[test]
fn reads_a_running_background_subagent() {
    let project = tempfile::tempdir().unwrap();
    let source = write_subagent(
        project.path(),
        "a1",
        r#"{"agentType":"Explore","description":"Investigar bugs de Pix","requestShape":"background"}"#,
        &[PROMPT_LINE, TOOL_USE_LINE],
    );
    let found = scan(&[source], project.path(), SystemTime::now());
    let subagents = &found["session-1"];
    assert_eq!(subagents.len(), 1);
    let subagent = &subagents[0];
    assert_eq!(subagent.id, "a1");
    assert_eq!(subagent.description, "Investigar bugs de Pix");
    assert_eq!(subagent.agent_type, "Explore");
    assert!(subagent.background);
    assert_eq!(subagent.state, SubagentState::Running);
    assert_eq!(subagent.prompt, "Investigue os bugs de Pix.");
}

#[test]
fn finished_flag_or_final_answer_marks_a_subagent_finished() {
    let project = tempfile::tempdir().unwrap();
    let source = write_subagent(
        project.path(),
        "flagged",
        r#"{"description":"a","finished":true}"#,
        &[PROMPT_LINE, TOOL_USE_LINE],
    );
    write_subagent(
        project.path(),
        "answered",
        r#"{"description":"b"}"#,
        &[PROMPT_LINE, END_TURN_LINE],
    );
    let found = scan(&[source], project.path(), SystemTime::now());
    let subagents = &found["session-1"];
    assert_eq!(subagents.len(), 2);
    assert!(subagents.iter().all(|subagent| !subagent.is_running()));
}

#[test]
fn drops_stale_running_and_long_finished_subagents() {
    let project = tempfile::tempdir().unwrap();
    let source = write_subagent(
        project.path(),
        "running",
        r#"{"description":"a"}"#,
        &[PROMPT_LINE, TOOL_USE_LINE],
    );
    write_subagent(
        project.path(),
        "done",
        r#"{"description":"b","finished":true}"#,
        &[PROMPT_LINE, END_TURN_LINE],
    );
    let in_eleven_minutes = SystemTime::now() + Duration::from_secs(11 * 60);
    let found = scan(
        std::slice::from_ref(&source),
        project.path(),
        in_eleven_minutes,
    );
    let ids: Vec<_> = found["session-1"].iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, vec!["running"]);

    let in_31_minutes = SystemTime::now() + Duration::from_secs(31 * 60);
    assert!(scan(&[source], project.path(), in_31_minutes).is_empty());
}

#[test]
fn sessions_without_subagents_are_left_out() {
    let project = tempfile::tempdir().unwrap();
    let source = SubagentSource {
        session_id: "session-2".to_owned(),
        transcript_path: Some(project.path().join("session-2.jsonl")),
    };
    assert!(scan(&[source], project.path(), SystemTime::now()).is_empty());
}

#[test]
fn finds_the_session_by_id_before_its_transcript_path_is_known() {
    let projects = tempfile::tempdir().unwrap();
    let project = projects.path().join("-Users-me-code-app");
    write_subagent(
        &project,
        "a1",
        r#"{"description":"a"}"#,
        &[PROMPT_LINE, TOOL_USE_LINE],
    );
    let source = SubagentSource {
        session_id: "session-1".to_owned(),
        transcript_path: None,
    };
    let found = scan(&[source], projects.path(), SystemTime::now());
    assert_eq!(found["session-1"].len(), 1);
}

#[test]
fn last_line_of_tail_handles_partial_and_oversized_lines() {
    assert_eq!(
        last_line_of_tail(b"first\nsecond\n", true).as_deref(),
        Some("second")
    );
    assert_eq!(last_line_of_tail(b"only\n", true).as_deref(), Some("only"));
    assert_eq!(
        last_line_of_tail(b"cut-off start\nlast\n", false).as_deref(),
        Some("last")
    );
    // A line longer than the tail read: its start is not in the buffer.
    assert_eq!(last_line_of_tail(b"middle of a huge line\n", false), None);
}

#[test]
fn only_a_final_assistant_answer_ends_the_turn() {
    assert!(line_ends_turn(END_TURN_LINE));
    assert!(!line_ends_turn(TOOL_USE_LINE));
    assert!(!line_ends_turn(PROMPT_LINE));
    assert!(!line_ends_turn("not json"));
}

#[test]
fn prompt_reads_text_blocks_and_is_cut_to_length() {
    let blocks =
        r#"{"message":{"content":[{"type":"text","text":"um"},{"type":"text","text":"dois"}]}}"#;
    assert_eq!(prompt_from_first_entry(blocks).as_deref(), Some("um\ndois"));

    let long = format!(
        r#"{{"message":{{"content":"{}"}}}}"#,
        "é".repeat(super::MAX_PROMPT_CHARS + 10)
    );
    let prompt = prompt_from_first_entry(&long).unwrap();
    assert_eq!(prompt.chars().count(), super::MAX_PROMPT_CHARS + 1);
    assert!(prompt.ends_with('…'));
}
