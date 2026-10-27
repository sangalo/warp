use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use super::scanner::{ResourceKind, ResourceState, ScanResult};
use super::subagents::{Subagent, SubagentState};
use super::{TabSessionFacts, WaitReason, compute_tab_signals};

fn agent_tab(uuid: &str, checkout: &str, session_id: &str) -> TabSessionFacts {
    TabSessionFacts {
        session_uuids: vec![uuid.to_owned()],
        agent_checkout: Some(PathBuf::from(checkout)),
        agent_session_id: Some(session_id.to_owned()),
        agent_session_ids: vec![session_id.to_owned()],
        waiting: None,
        server: None,
    }
}

fn files(paths: &[&str]) -> HashSet<PathBuf> {
    paths.iter().map(PathBuf::from).collect()
}

#[test]
fn flags_agents_sharing_a_checkout_but_not_separate_worktrees() {
    let tabs = [
        agent_tab("a", "/code/acme", "s1"),
        agent_tab("b", "/code/acme", "s2"),
        agent_tab("c", "/code/acme-wt-fix", "s3"),
        TabSessionFacts::default(),
    ];
    let signals = compute_tab_signals(&tabs, &ScanResult::default());
    assert_eq!(
        signals
            .iter()
            .map(|s| s.shares_checkout)
            .collect::<Vec<_>>(),
        vec![true, true, false, false]
    );
}

#[test]
fn flags_files_edited_by_two_open_sessions() {
    let tabs = [
        agent_tab("a", "/code/x", "s1"),
        agent_tab("b", "/code/y", "s2"),
    ];
    let scan = ScanResult {
        edited_files: HashMap::from([
            ("s1".to_owned(), files(&["/code/x/a.rs", "/code/x/b.rs"])),
            ("s2".to_owned(), files(&["/code/x/b.rs"])),
            // A session no tab shows any more does not make a conflict.
            ("closed".to_owned(), files(&["/code/x/a.rs"])),
        ]),
        ..Default::default()
    };
    let signals = compute_tab_signals(&tabs, &scan);
    assert_eq!(
        signals[0].contested_files,
        vec![PathBuf::from("/code/x/b.rs")]
    );
    assert_eq!(
        signals[1].contested_files,
        vec![PathBuf::from("/code/x/b.rs")]
    );
}

#[test]
fn attributes_resources_to_the_tab_that_started_them() {
    let tabs = [
        agent_tab("a", "/code/x", "s1"),
        agent_tab("b", "/code/y", "s2"),
    ];
    let scan = ScanResult {
        resources: vec![
            (
                ResourceKind::Playwright,
                ResourceState::InUse {
                    pane: Some("b".to_owned()),
                    process: "Playwright".to_owned(),
                },
            ),
            (
                ResourceKind::Redis,
                ResourceState::InUse {
                    pane: None,
                    process: "redis-server".to_owned(),
                },
            ),
            (ResourceKind::Port(8000), ResourceState::Free),
        ],
        ..Default::default()
    };
    let signals = compute_tab_signals(&tabs, &scan);
    assert!(signals[0].held_resources.is_empty());
    assert_eq!(signals[1].held_resources, vec![ResourceKind::Playwright]);
}

#[test]
fn passes_waiting_state_through() {
    let mut tab = agent_tab("a", "/code/x", "s1");
    tab.waiting = Some((WaitReason::Blocked, Duration::from_secs(90)));
    let signals = compute_tab_signals(&[tab], &ScanResult::default());
    assert_eq!(
        signals[0].waiting,
        Some((WaitReason::Blocked, Duration::from_secs(90)))
    );
}

#[test]
fn gathers_subagents_of_every_session_in_a_split_tab() {
    let subagent = |id: &str, started_secs_ago: u64| Subagent {
        id: id.to_owned(),
        description: String::new(),
        agent_type: String::new(),
        background: true,
        started: SystemTime::now() - Duration::from_secs(started_secs_ago),
        state: SubagentState::Running,
        prompt: String::new(),
    };
    let mut split_tab = agent_tab("a", "/code/x", "s1");
    split_tab.agent_session_ids.push("s2".to_owned());
    let tabs = [split_tab, agent_tab("b", "/code/y", "s3")];
    let scan = ScanResult {
        subagents: HashMap::from([
            ("s1".to_owned(), vec![subagent("newer", 10)]),
            ("s2".to_owned(), vec![subagent("older", 60)]),
        ]),
        ..Default::default()
    };
    let signals = compute_tab_signals(&tabs, &scan);
    let ids: Vec<_> = signals[0].subagents.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, vec!["older", "newer"]);
    assert!(signals[1].subagents.is_empty());
}
