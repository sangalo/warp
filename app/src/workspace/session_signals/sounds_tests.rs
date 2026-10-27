use super::{should_play_finish_chime, sound_path};

#[test]
fn empty_setting_plays_nothing() {
    assert_eq!(sound_path(""), None);
    assert_eq!(sound_path("   "), None);
}

#[test]
fn rejects_names_that_could_escape_the_sounds_directory() {
    assert_eq!(sound_path("../../etc/passwd"), None);
    assert_eq!(sound_path("Glass/../Ping"), None);
}

#[test]
fn accepts_existing_absolute_paths_only() {
    let file = tempfile::NamedTempFile::new().unwrap();
    assert_eq!(
        sound_path(file.path().to_str().unwrap()).as_deref(),
        Some(file.path())
    );
    assert_eq!(sound_path("/nonexistent/sound.aiff"), None);
}

#[cfg(target_os = "macos")]
#[test]
fn resolves_macos_system_sound_names() {
    assert!(sound_path("Glass").is_some());
    assert_eq!(sound_path("NoSuchSound"), None);
}

#[test]
fn finish_chime_needs_the_tab_toggle_and_a_real_turn() {
    assert!(should_play_finish_chime(true, true, false));
    assert!(!should_play_finish_chime(false, true, false));
    assert!(!should_play_finish_chime(true, false, false));
}

#[test]
fn finish_chime_defers_to_a_configured_finish_sound() {
    assert!(!should_play_finish_chime(true, true, true));
}
