//! Reads shared machine resources and per-session file edits from the system.
//!
//! Everything here is blocking and runs off the main thread. The parsers are kept separate from the
//! process calls so they can be tested against captured output.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use command::blocking::Command;

use super::subagents::Subagent;

/// Environment variable through which each Warp terminal tells its processes which pane they run in.
const SESSION_UUID_ENV: &str = "WARP_TERMINAL_SESSION_UUID";

/// How far up the process tree to look for the pane a process belongs to.
const MAX_ANCESTOR_DEPTH: usize = 32;

/// Edits older than this are ignored: the session that made them has long moved on.
const EDIT_LEDGER_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// Ports that local dev servers in the user's projects compete for.
pub const WATCHED_PORTS: &[u16] = &[8000, 8081];
const REDIS_PORT: u16 = 6379;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ResourceKind {
    Port(u16),
    Redis,
    Simulator,
    Playwright,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResourceState {
    Free,
    InUse {
        /// The hex session UUID of the pane the process was started from, if it was started from
        /// a Warp terminal.
        pane: Option<String>,
        /// Short name of the process holding the resource.
        process: String,
    },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScanResult {
    pub resources: Vec<(ResourceKind, ResourceState)>,
    /// Files each agent session edited, keyed by agent session id.
    pub edited_files: HashMap<String, HashSet<PathBuf>>,
    /// Playwright MCP servers running without the browser open.
    pub idle_playwright_servers: usize,
    /// Running and recently finished subagents, keyed by agent session id.
    pub subagents: HashMap<String, Vec<Subagent>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub ppid: u32,
    pub command: String,
}

/// Scans the machine. `edit_ledger_dir` holds one file per agent session listing edited paths.
pub fn scan(edit_ledger_dir: &Path) -> ScanResult {
    let processes = run(&["ps", "-axo", "pid=,ppid=,command="])
        .map(|output| parse_ps(&output))
        .unwrap_or_default();
    let listeners = run(&["lsof", "-nP", "-iTCP", "-sTCP:LISTEN", "-Fpn"])
        .map(|output| parse_lsof_listeners(&output))
        .unwrap_or_default();
    let mut env_cache = HashMap::new();
    let mut pane_of = |pid: u32| {
        owning_pane(pid, &processes, |pid| {
            env_cache
                .entry(pid)
                .or_insert_with(|| process_session_uuid(pid))
                .clone()
        })
    };

    let mut resources = Vec::new();
    for &port in WATCHED_PORTS {
        resources.push((
            ResourceKind::Port(port),
            listener_state(port, &listeners, &processes, &mut pane_of),
        ));
    }
    resources.push((
        ResourceKind::Redis,
        listener_state(REDIS_PORT, &listeners, &processes, &mut pane_of),
    ));
    resources.push((
        ResourceKind::Simulator,
        simulator_state(&processes, &mut pane_of),
    ));
    resources.push((
        ResourceKind::Playwright,
        playwright_state(&processes, &mut pane_of),
    ));

    ScanResult {
        resources,
        edited_files: read_edit_ledger(edit_ledger_dir, EDIT_LEDGER_MAX_AGE),
        idle_playwright_servers: idle_playwright_servers(&processes),
        subagents: HashMap::new(),
    }
}

fn run(args: &[&str]) -> Option<String> {
    let (program, rest) = args.split_first()?;
    let output = Command::new(program).args(rest).output().ok()?;
    String::from_utf8(output.stdout).ok()
}

fn process_session_uuid(pid: u32) -> Option<String> {
    run(&["ps", "eww", "-o", "command=", "-p", &pid.to_string()])
        .and_then(|output| session_uuid_from_env_dump(&output))
}

fn listener_state(
    port: u16,
    listeners: &[(u16, u32)],
    processes: &HashMap<u32, ProcessInfo>,
    pane_of: &mut impl FnMut(u32) -> Option<String>,
) -> ResourceState {
    match listeners
        .iter()
        .find(|(listening_port, _)| *listening_port == port)
    {
        Some(&(_, pid)) => ResourceState::InUse {
            pane: pane_of(pid),
            process: process_name(pid, processes),
        },
        None => ResourceState::Free,
    }
}

fn simulator_state(
    processes: &HashMap<u32, ProcessInfo>,
    pane_of: &mut impl FnMut(u32) -> Option<String>,
) -> ResourceState {
    let simulator_running = processes.values().any(|process| {
        process
            .command
            .contains("Simulator.app/Contents/MacOS/Simulator")
    });
    if !simulator_running {
        return ResourceState::Free;
    }
    let driver = processes
        .values()
        .find(|process| is_simulator_driver(&process.command));
    match driver {
        Some(process) => ResourceState::InUse {
            pane: pane_of(process.pid),
            process: process_name(process.pid, processes),
        },
        None => ResourceState::InUse {
            pane: None,
            process: "Simulator".to_owned(),
        },
    }
}

/// Whether `command` is a tool that builds for or drives the iOS simulator.
fn is_simulator_driver(command: &str) -> bool {
    ["simctl", "run-ios", "run:ios", "xcodebuild"]
        .iter()
        .any(|marker| command.contains(marker))
}

fn playwright_state(
    processes: &HashMap<u32, ProcessInfo>,
    pane_of: &mut impl FnMut(u32) -> Option<String>,
) -> ResourceState {
    match playwright_browser(processes) {
        Some(pid) => ResourceState::InUse {
            pane: pane_of(pid),
            process: "Playwright".to_owned(),
        },
        None => ResourceState::Free,
    }
}

/// The main browser process of the Playwright MCP server, if one is open.
///
/// Every MCP server instance shares one browser profile, so only one session can hold the browser
/// at a time.
pub fn playwright_browser(processes: &HashMap<u32, ProcessInfo>) -> Option<u32> {
    processes
        .values()
        .find(|process| {
            process.command.contains("--user-data-dir=")
                && process.command.contains("ms-playwright-mcp")
                && !process.command.contains("--type=")
        })
        .map(|process| process.pid)
}

/// How many Playwright MCP servers are running without being the one that opened the browser.
pub fn idle_playwright_servers(processes: &HashMap<u32, ProcessInfo>) -> usize {
    let browser_server = playwright_browser(processes)
        .and_then(|browser| processes.get(&browser))
        .map(|browser| browser.ppid);
    processes
        .values()
        .filter(|process| is_playwright_server(&process.command))
        .filter(|process| Some(process.pid) != browser_server)
        .count()
}

/// Whether `command` is the Playwright MCP server itself, not its `npm exec` wrapper or a browser.
fn is_playwright_server(command: &str) -> bool {
    command.ends_with("/playwright-mcp") || command.contains("/playwright-mcp ")
}

/// Finds the pane `pid` was started from by walking up its ancestors until one carries the pane's
/// session UUID.
pub fn owning_pane(
    pid: u32,
    processes: &HashMap<u32, ProcessInfo>,
    mut session_uuid_of: impl FnMut(u32) -> Option<String>,
) -> Option<String> {
    let mut current = pid;
    for _ in 0..MAX_ANCESTOR_DEPTH {
        if let Some(uuid) = session_uuid_of(current) {
            return Some(uuid);
        }
        let parent = processes.get(&current)?.ppid;
        if parent <= 1 || parent == current {
            return None;
        }
        current = parent;
    }
    None
}

fn process_name(pid: u32, processes: &HashMap<u32, ProcessInfo>) -> String {
    processes
        .get(&pid)
        .and_then(|process| process.command.split_whitespace().next())
        .and_then(|program| Path::new(program).file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("pid {pid}"))
}

/// Parses `ps -axo pid=,ppid=,command=`.
pub fn parse_ps(output: &str) -> HashMap<u32, ProcessInfo> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse().ok()?;
            let ppid = fields.next()?.parse().ok()?;
            let command = fields.collect::<Vec<_>>().join(" ");
            Some((pid, ProcessInfo { pid, ppid, command }))
        })
        .collect()
}

/// Parses `lsof -nP -iTCP -sTCP:LISTEN -Fpn` into `(port, pid)` pairs.
pub fn parse_lsof_listeners(output: &str) -> Vec<(u16, u32)> {
    let mut listeners = Vec::new();
    let mut pid = None;
    for line in output.lines() {
        if let Some(value) = line.strip_prefix('p') {
            pid = value.parse().ok();
        } else if let Some(address) = line.strip_prefix('n')
            && let (Some(pid), Some(port)) = (pid, address.rsplit(':').next())
            && let Ok(port) = port.parse()
            && !listeners.contains(&(port, pid))
        {
            listeners.push((port, pid));
        }
    }
    listeners
}

/// Extracts the pane session UUID from the output of `ps eww -o command=`.
pub fn session_uuid_from_env_dump(dump: &str) -> Option<String> {
    let prefix = format!("{SESSION_UUID_ENV}=");
    dump.split_whitespace()
        .find_map(|token| token.strip_prefix(&prefix))
        .filter(|uuid| !uuid.is_empty() && uuid.chars().all(|c| c.is_ascii_hexdigit()))
        .map(str::to_owned)
}

/// Reads `<dir>/<session id>.txt` files, each listing one edited path per line.
pub fn read_edit_ledger(dir: &Path, max_age: Duration) -> HashMap<String, HashSet<PathBuf>> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return HashMap::new();
    };
    let now = SystemTime::now();
    entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .is_some_and(|age| age <= max_age)
        })
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().is_none_or(|extension| extension != "txt") {
                return None;
            }
            let session_id = path.file_stem()?.to_str()?.to_owned();
            let contents = std::fs::read_to_string(&path).ok()?;
            Some((session_id, parse_edit_ledger(&contents)))
        })
        .collect()
}

pub fn parse_edit_ledger(contents: &str) -> HashSet<PathBuf> {
    contents
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('/'))
        .map(PathBuf::from)
        .collect()
}

#[cfg(test)]
#[path = "scanner_tests.rs"]
mod tests;
