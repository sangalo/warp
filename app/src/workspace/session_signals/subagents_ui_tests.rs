use std::time::{Duration, SystemTime};

use super::{dot_opacity, finished_meta, running_meta_parts};
use crate::workspace::session_signals::subagents::{Subagent, SubagentState};

fn subagent(agent_type: &str, background: bool, started: SystemTime) -> Subagent {
    Subagent {
        id: "a1".to_owned(),
        description: "Investigar bugs de Pix".to_owned(),
        agent_type: agent_type.to_owned(),
        background,
        started,
        state: SubagentState::Running,
        prompt: String::new(),
    }
}

#[test]
fn dots_light_up_in_turn() {
    let peak = Duration::from_millis(540);
    assert!((dot_opacity(0, peak) - 1.).abs() < 0.01);
    assert!(dot_opacity(1, peak) < dot_opacity(0, peak));
    assert!((dot_opacity(1, peak + Duration::from_millis(300)) - 1.).abs() < 0.01);
    for millis in (0..3600).step_by(37) {
        let opacity = dot_opacity(2, Duration::from_millis(millis));
        assert!((0.3..=1.).contains(&opacity));
    }
}

#[test]
fn running_meta_names_type_launch_and_time() {
    let now = SystemTime::now();
    let started = now - Duration::from_secs(252);
    assert_eq!(
        running_meta_parts(&subagent("Explore", true, started), now),
        ("Explore · background".to_owned(), "4 min 12 s".to_owned())
    );
    assert_eq!(
        running_meta_parts(&subagent("", false, started), now),
        (String::new(), "4 min 12 s".to_owned())
    );
}

#[test]
fn finished_meta_says_how_long_it_ran_and_when_it_ended() {
    let now = SystemTime::now();
    let started = now - Duration::from_secs(8 * 60 + 10);
    let finished_at = now - Duration::from_secs(2 * 60 + 5);
    assert_eq!(
        finished_meta(
            &subagent("general-purpose", true, started),
            finished_at,
            now
        ),
        "general-purpose · rodou 6 min · terminou há 2 min"
    );
    assert_eq!(
        finished_meta(
            &subagent("Explore", true, now - Duration::from_secs(40)),
            now,
            now
        ),
        "Explore · rodou 40 s · terminou agora"
    );
}
