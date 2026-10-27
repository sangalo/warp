use std::time::Duration;

use chrono::NaiveDateTime;
use instant::Instant;

use super::{ServerLine, ServerPaneObservation, ServerRequest, ServerTrack, parse_last_request};
use crate::terminal::model::BlockId;

fn at(text: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(text, "%d/%b/%Y %H:%M:%S").unwrap()
}

fn request(path: &str, status: u16, logged_at: &str) -> ServerRequest {
    ServerRequest {
        method: "GET".to_owned(),
        path: path.to_owned(),
        status,
        logged_at: at(logged_at),
    }
}

fn observation(block: &str, is_executing: bool) -> ServerPaneObservation {
    ServerPaneObservation {
        pane: "pane".to_owned(),
        active_block: BlockId::from(block.to_owned()),
        is_executing,
        last_request: None,
        is_active_tab: false,
        is_agent_pane: false,
    }
}

#[test]
fn reads_the_last_runserver_request() {
    let output = "\
Starting development server at http://0.0.0.0:8000/
[27/Oct/2026 13:55:04] \"GET /api/pedidos HTTP/1.1\" 200 1234
[27/Oct/2026 13:55:09] \"POST /api/pagar HTTP/1.1\" 500 145
";
    let request = parse_last_request(output).unwrap();
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/pagar");
    assert_eq!(request.status, 500);
    assert_eq!(request.logged_at, at("27/Oct/2026 13:55:09"));
}

#[test]
fn skips_static_files_and_tracebacks() {
    let output = "\
[27/Oct/2026 13:55:04] \"GET /api/pedidos HTTP/1.1\" 200 1234
Internal Server Error: /api/pedidos
Traceback (most recent call last):
  File \"views.py\", line 3, in get
[27/Oct/2026 13:55:05] \"GET /static/app.css HTTP/1.1\" 304 0
[27/Oct/2026 13:55:05] \"GET /favicon.ico HTTP/1.1\" 404 179
/Users/me/app/views.py changed, reloading.
";
    assert_eq!(
        parse_last_request(output),
        Some(request("/api/pedidos", 200, "27/Oct/2026 13:55:04"))
    );
}

#[test]
fn keeps_not_modified_responses_and_query_strings() {
    let output = "[27/Oct/2026 13:55:04] \"GET /api/itens?page=2 HTTP/1.1\" 304 0\n";
    assert_eq!(
        parse_last_request(output),
        Some(request("/api/itens?page=2", 304, "27/Oct/2026 13:55:04"))
    );
}

#[test]
fn output_without_requests_has_none() {
    assert_eq!(parse_last_request("Watching for file changes\n"), None);
    assert_eq!(parse_last_request(""), None);
}

#[test]
fn ctrl_c_ends_the_server() {
    let now = Instant::now();
    let mut track = ServerTrack::new(8000, now);
    assert!(track.observe(observation("b1", true), now));
    track.down_since = Some(now);
    assert!(!track.observe(observation("b1", false), now));
}

#[test]
fn a_crashed_reload_is_down_while_its_block_runs() {
    let now = Instant::now();
    let mut track = ServerTrack::new(8000, now);
    assert!(track.observe(observation("b1", true), now));
    track.down_since = Some(now);
    assert!(track.observe(observation("b1", true), now));
    assert!(matches!(
        track.line(now, at("27/Oct/2026 13:55:04")),
        ServerLine::Down(_)
    ));
}

#[test]
fn a_later_command_in_the_same_pane_is_not_the_server() {
    let now = Instant::now();
    let mut track = ServerTrack::new(8000, now);
    assert!(track.observe(observation("b1", true), now));
    track.down_since = Some(now);
    assert!(!track.observe(observation("b2", true), now));
}

#[test]
fn requests_already_in_the_output_do_not_ring_or_alert() {
    let now = Instant::now();
    let mut track = ServerTrack::new(8000, now);
    let mut first = observation("b1", true);
    first.last_request = Some(request("/api/pagar", 500, "27/Oct/2026 13:55:04"));
    track.observe(first, now);
    assert_eq!(track.blink_progress(now), None);
    assert!(matches!(
        track.line(now, at("27/Oct/2026 13:55:10")),
        ServerLine::Request {
            highlight_status: false,
            ..
        }
    ));
}

#[test]
fn a_new_error_rings_and_stays_until_the_tab_is_opened() {
    let now = Instant::now();
    let mut track = ServerTrack::new(8000, now);
    track.observe(observation("b1", true), now);

    let mut error = observation("b1", true);
    error.last_request = Some(request("/api/pagar", 500, "27/Oct/2026 13:55:04"));
    track.observe(error.clone(), now);
    assert_eq!(track.blink_progress(now), Some(0.));

    let much_later = at("27/Oct/2026 14:30:00");
    assert!(matches!(
        track.line(now + Duration::from_secs(30 * 60), much_later),
        ServerLine::Request {
            highlight_status: true,
            ..
        }
    ));

    let mut opened = error;
    opened.is_active_tab = true;
    track.observe(opened, now + Duration::from_secs(5));
    assert!(matches!(
        track.line(now + Duration::from_secs(30 * 60), much_later),
        ServerLine::Idle {
            last_request: Some(_)
        }
    ));
}

#[test]
fn an_error_arriving_in_the_open_tab_is_not_highlighted() {
    let now = Instant::now();
    let mut track = ServerTrack::new(8000, now);
    track.observe(observation("b1", true), now);
    let mut error = observation("b1", true);
    error.last_request = Some(request("/api/pagar", 500, "27/Oct/2026 13:55:04"));
    error.is_active_tab = true;
    track.observe(error, now);
    assert!(matches!(
        track.line(now, at("27/Oct/2026 13:55:05")),
        ServerLine::Request {
            highlight_status: false,
            ..
        }
    ));
}

#[test]
fn an_old_request_reads_as_idle() {
    let now = Instant::now();
    let mut track = ServerTrack::new(8000, now);
    let mut seen = observation("b1", true);
    seen.last_request = Some(request("/api/pedidos", 200, "27/Oct/2026 13:50:00"));
    track.observe(seen, now);
    assert_eq!(
        track.line(now, at("27/Oct/2026 13:55:00")),
        ServerLine::Idle {
            last_request: Some(Duration::from_secs(300))
        }
    );
}

#[test]
fn chrome_devtools_probes_are_skipped() {
    let output = "\
[27/Oct/2026 13:55:04] \"GET /api/pedidos HTTP/1.1\" 200 1234
[27/Oct/2026 13:55:05] \"GET /.well-known/appspecific/com.chrome.devtools.json HTTP/1.1\" 404 179
";
    assert_eq!(parse_last_request(output).unwrap().path, "/api/pedidos");
}

#[test]
fn a_tab_nobody_observes_is_abandoned() {
    let now = Instant::now();
    let mut track = ServerTrack::new(8000, now);
    assert!(!track.is_abandoned(now + Duration::from_secs(5)));
    assert!(track.is_abandoned(now + Duration::from_secs(11)));
    track.observe(observation("b1", true), now + Duration::from_secs(11));
    assert!(!track.is_abandoned(now + Duration::from_secs(12)));
}

#[test]
fn a_server_started_by_a_coding_agent_is_marked_as_the_agents() {
    let now = Instant::now();
    let mut track = ServerTrack::new(8000, now);
    let mut agent = observation("b1", true);
    agent.is_agent_pane = true;
    assert!(track.observe(agent, now));
    assert!(track.is_agent_pane);
}
