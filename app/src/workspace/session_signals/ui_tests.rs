use std::time::Duration;

use super::{
    CONTESTED_FILE_COLOR, WAITING_COLOR, elapsed_label, idle_label, tab_border_color, working_label,
};
use crate::workspace::session_signals::{TabSignals, WaitReason};

#[test]
fn border_shows_the_strongest_signal() {
    let mut signals = TabSignals {
        waiting: Some((WaitReason::Blocked, Duration::from_secs(60))),
        ..Default::default()
    };
    assert_eq!(tab_border_color(&signals), Some(WAITING_COLOR));
    signals.contested_files.push("/repo/a.rs".into());
    assert_eq!(tab_border_color(&signals), Some(CONTESTED_FILE_COLOR));
}

#[test]
fn shared_checkout_does_not_add_a_border() {
    let signals = TabSignals {
        shares_checkout: true,
        ..Default::default()
    };
    assert_eq!(tab_border_color(&signals), None);
}

#[test]
fn finished_turn_alone_does_not_add_a_border() {
    let signals = TabSignals {
        waiting: Some((WaitReason::Finished, Duration::from_secs(60))),
        ..Default::default()
    };
    assert_eq!(tab_border_color(&signals), None);
    assert_eq!(tab_border_color(&TabSignals::default()), None);
}

#[test]
fn elapsed_labels_read_naturally() {
    assert_eq!(elapsed_label(Duration::from_secs(20)), "agora");
    assert_eq!(elapsed_label(Duration::from_secs(8 * 60)), "8 min");
    assert_eq!(elapsed_label(Duration::from_secs(75 * 60)), "1h15");
}

#[test]
fn working_label_counts_seconds() {
    assert_eq!(working_label(Duration::from_secs(14)), "trabalhando · 14 s");
    assert_eq!(
        working_label(Duration::from_secs(134)),
        "trabalhando · 2 min 14 s"
    );
    assert_eq!(
        working_label(Duration::from_secs(3900)),
        "trabalhando · 1 h 05 min"
    );
}

#[test]
fn idle_label_reads_naturally() {
    assert_eq!(idle_label(Duration::from_secs(20)), "parado agora");
    assert_eq!(idle_label(Duration::from_secs(12 * 60)), "parado há 12 min");
}
