use std::time::Duration;

use super::{BAND_WIDTH_RATIO, SWEEP_PERIOD, WorkingSweep};

#[test]
fn band_enters_and_leaves_fully_outside_the_tab() {
    let half_band = BAND_WIDTH_RATIO / 2.;
    let start = WorkingSweep::band_center(Duration::ZERO);
    assert!((start + half_band).abs() < 1e-4);
    let end = WorkingSweep::band_center(SWEEP_PERIOD - Duration::from_millis(1));
    assert!(end - half_band > 0.99);
}

#[test]
fn band_crosses_the_middle_halfway_through() {
    let middle = WorkingSweep::band_center(SWEEP_PERIOD / 2);
    assert!((middle - 0.5).abs() < 1e-4);
}
