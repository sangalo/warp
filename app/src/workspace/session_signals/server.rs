//! Tabs running a local dev server: whether it is up, and the last request it answered.
//!
//! The server is known by the port it listens on (from the machine scan) and the terminal block it
//! runs in. The request comes from the last line Django's runserver logged in that block.

use std::sync::LazyLock;
use std::time::Duration;

use chrono::NaiveDateTime;
use instant::Instant;
use regex::Regex;

use crate::terminal::model::BlockId;

/// How long the last request stays on the card before the card reads as idle.
const REQUEST_DETAIL_DURATION: Duration = Duration::from_secs(60);
/// How long the status dot rings after a new request.
pub const BLINK_DURATION: Duration = Duration::from_millis(1400);
/// Output rows read back to find the last request; enough to get past a traceback.
pub const OUTPUT_ROWS_TO_SCAN: usize = 80;

/// `[27/Oct/2026 13:55:04] "GET /api/pedidos HTTP/1.1" 200 1234`
static RUNSERVER_REQUEST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"^\[(\d{2}/[A-Za-z]{3}/\d{4} \d{2}:\d{2}:\d{2})\] "([A-Z]+) (\S+) HTTP/[\d.]+" (\d{3})\b"#,
    )
    .expect("valid runserver request regex")
});

/// Paths a page load requests by the dozen, which would hide the request that matters.
const NOISE_PREFIXES: &[&str] = &[
    "/static/",
    "/media/",
    "/favicon.ico",
    "/__debug__/",
    // Chrome asks for this whenever DevTools is open, and Django answers 404.
    "/.well-known/",
];
/// A server no window has looked at for this long belongs to a tab that was closed.
const UNOBSERVED_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerRequest {
    pub method: String,
    pub path: String,
    pub status: u16,
    /// Local time the server logged it, to the second.
    pub logged_at: NaiveDateTime,
}

/// The last request in `output` that is not a static file, if any.
pub fn parse_last_request(output: &str) -> Option<ServerRequest> {
    output.lines().rev().find_map(|line| {
        let captures = RUNSERVER_REQUEST.captures(line.trim_end())?;
        let path = captures[3].to_owned();
        if NOISE_PREFIXES.iter().any(|prefix| path.starts_with(prefix)) {
            return None;
        }
        Some(ServerRequest {
            method: captures[2].to_owned(),
            path,
            status: captures[4].parse().ok()?,
            logged_at: NaiveDateTime::parse_from_str(&captures[1], "%d/%b/%Y %H:%M:%S").ok()?,
        })
    })
}

/// A server seen listening from a tab's pane.
#[derive(Clone, Debug, PartialEq)]
pub struct ServerTrack {
    pub port: u16,
    /// The block the server runs in. A later command in the same pane is not the server.
    block: Option<BlockId>,
    /// Since when the port has been free while the server's block keeps running.
    pub down_since: Option<Instant>,
    pub last_request: Option<ServerRequest>,
    /// When a request was first seen, for the dot's ring. `None` for requests already in the
    /// output when the server was first observed.
    request_seen_at: Option<Instant>,
    /// Whether the user has looked at the tab since its last error.
    error_acknowledged: bool,
    is_observed: bool,
    /// The pane runs a coding agent, whose own line wins over the server's.
    pub is_agent_pane: bool,
    last_observed_at: Instant,
}

/// What a tab's pane looks like when its server is checked.
#[derive(Clone, Debug)]
pub struct ServerPaneObservation {
    pub pane: String,
    pub active_block: BlockId,
    pub is_executing: bool,
    pub last_request: Option<ServerRequest>,
    pub is_active_tab: bool,
    pub is_agent_pane: bool,
}

impl ServerTrack {
    pub fn new(port: u16, now: Instant) -> Self {
        Self {
            port,
            block: None,
            down_since: None,
            last_request: None,
            request_seen_at: None,
            error_acknowledged: false,
            is_observed: false,
            is_agent_pane: false,
            last_observed_at: now,
        }
    }

    /// Whether no window has observed this server's pane for long enough that its tab is gone.
    pub fn is_abandoned(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.last_observed_at) > UNOBSERVED_TIMEOUT
    }

    /// Records `observation`. Returns `false` when the pane no longer runs the server: it was
    /// stopped, or another command took its place, and the tab should read as a normal tab again.
    pub fn observe(&mut self, observation: ServerPaneObservation, now: Instant) -> bool {
        self.last_observed_at = now;
        self.is_agent_pane = observation.is_agent_pane;
        let is_server_block =
            observation.is_executing && self.block.as_ref() == Some(&observation.active_block);
        if self.down_since.is_none() {
            if observation.is_executing {
                self.block = Some(observation.active_block);
            }
        } else if !is_server_block {
            return false;
        }

        if observation.last_request.is_some() && observation.last_request != self.last_request {
            self.last_request = observation.last_request;
            self.request_seen_at = self.is_observed.then_some(now);
            // Errors already in the output when Warp started are not news.
            self.error_acknowledged = observation.is_active_tab || !self.is_observed;
        } else if observation.is_active_tab {
            self.error_acknowledged = true;
        }
        self.is_observed = true;
        true
    }

    /// How far the dot's ring has grown, from 0 to 1, while it rings for a new request.
    pub fn blink_progress(&self, now: Instant) -> Option<f32> {
        let elapsed = now.checked_duration_since(self.request_seen_at?)?;
        (elapsed < BLINK_DURATION).then(|| elapsed.as_secs_f32() / BLINK_DURATION.as_secs_f32())
    }

    pub fn line(&self, now: Instant, now_local: NaiveDateTime) -> ServerLine {
        if let Some(since) = self.down_since {
            return ServerLine::Down(now.saturating_duration_since(since));
        }
        let Some(request) = &self.last_request else {
            return ServerLine::Idle { last_request: None };
        };
        let age = match self.request_seen_at {
            Some(seen_at) => now.saturating_duration_since(seen_at),
            None => (now_local - request.logged_at)
                .to_std()
                .unwrap_or(Duration::ZERO),
        };
        let highlight_status = request.status >= 400 && !self.error_acknowledged;
        if age < REQUEST_DETAIL_DURATION || highlight_status {
            ServerLine::Request {
                request: request.clone(),
                age,
                highlight_status,
            }
        } else {
            ServerLine::Idle {
                last_request: Some(age),
            }
        }
    }
}

/// What the line under a server tab's name says.
#[derive(Clone, Debug, PartialEq)]
pub enum ServerLine {
    /// Up, with no recent request: how long ago the last one came, if any did.
    Idle { last_request: Option<Duration> },
    /// Up, showing its last request. `highlight_status` while an error is unseen.
    Request {
        request: ServerRequest,
        age: Duration,
        highlight_status: bool,
    },
    /// The port went free while the server's command kept running, for this long.
    Down(Duration),
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
