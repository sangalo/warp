use std::collections::HashMap;
use std::time::Duration;

use super::{
    ProcessInfo, idle_playwright_servers, owning_pane, parse_edit_ledger, parse_lsof_listeners,
    parse_ps, playwright_browser, read_edit_ledger, session_uuid_from_env_dump,
};

const PS_OUTPUT: &str = "\
    1     0 /sbin/launchd
  1507  1357 -zsh -g --no_rcs
 65669  1507 claude --dangerously-skip-permissions
 65688 65669 npm exec @playwright/mcp@latest
 65807 65688 node /Users/me/.npm/_npx/x/node_modules/.bin/playwright-mcp
 96827 65807 /Applications/Google Chrome.app/Contents/MacOS/Google Chrome --user-data-dir=/Users/me/Library/Caches/ms-playwright-mcp/mcp-chrome-ea07755 --remote-debugging-pipe
 96851 96827 /Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Helper (GPU) --type=gpu-process --user-data-dir=/Users/me/Library/Caches/ms-playwright-mcp/mcp-chrome-ea07755
";

#[test]
fn parses_process_table() {
    let processes = parse_ps(PS_OUTPUT);
    assert_eq!(processes[&65669].ppid, 1507);
    assert_eq!(
        processes[&65669].command,
        "claude --dangerously-skip-permissions"
    );
    assert_eq!(processes.len(), 7);
}

#[test]
fn finds_main_playwright_browser_process() {
    assert_eq!(playwright_browser(&parse_ps(PS_OUTPUT)), Some(96827));
    assert_eq!(
        playwright_browser(&parse_ps("  10 1 /usr/bin/Google Chrome\n")),
        None
    );
}

#[test]
fn walks_up_to_the_pane_that_started_a_process() {
    let processes = parse_ps(PS_OUTPUT);
    let envs = HashMap::from([(65669, "abc123".to_owned())]);
    assert_eq!(
        owning_pane(96827, &processes, |pid| envs.get(&pid).cloned()),
        Some("abc123".to_owned())
    );
    assert_eq!(
        owning_pane(1507, &processes, |pid| envs.get(&pid).cloned()),
        None
    );
}

#[test]
fn parses_listening_ports() {
    let output = "p812\nf5\nn*:8000\nf6\nn[::]:8000\np900\nf3\nn127.0.0.1:6379\n";
    assert_eq!(parse_lsof_listeners(output), vec![(8000, 812), (6379, 900)]);
}

#[test]
fn reads_session_uuid_from_environment_dump() {
    let dump = "claude --resume x PATH=/bin WARP_TERMINAL_SESSION_UUID=ed7a32e2 TERM=xterm";
    assert_eq!(
        session_uuid_from_env_dump(dump),
        Some("ed7a32e2".to_owned())
    );
    assert_eq!(session_uuid_from_env_dump("claude PATH=/bin"), None);
    assert_eq!(
        session_uuid_from_env_dump("WARP_TERMINAL_SESSION_UUID=nothex!"),
        None
    );
}

#[test]
fn edit_ledger_keeps_absolute_paths_only() {
    let files = parse_edit_ledger("/repo/a.rs\nrelative.rs\n\n/repo/a.rs\n/repo/b.rs\n");
    assert_eq!(files.len(), 2);
    assert!(files.contains(std::path::Path::new("/repo/b.rs")));
}

#[test]
fn reads_one_ledger_file_per_session() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("s1.txt"), "/repo/a.rs\n").unwrap();
    std::fs::write(dir.path().join("notes.md"), "/repo/ignored.rs\n").unwrap();
    let ledger = read_edit_ledger(dir.path(), Duration::from_secs(60));
    assert_eq!(ledger.len(), 1);
    assert!(ledger["s1"].contains(std::path::Path::new("/repo/a.rs")));
    assert!(read_edit_ledger(&dir.path().join("missing"), Duration::from_secs(60)).is_empty());
}

#[test]
fn counts_playwright_servers_without_the_browser() {
    let mut processes = parse_ps(PS_OUTPUT);
    assert_eq!(idle_playwright_servers(&processes), 0);
    processes.insert(
        70001,
        ProcessInfo {
            pid: 70001,
            ppid: 70000,
            command: "node /Users/me/.npm/_npx/y/node_modules/.bin/playwright-mcp".to_owned(),
        },
    );
    assert_eq!(idle_playwright_servers(&processes), 1);
}
