//! Plays a sound when an agent session changes to a state the user asked to hear about.

use std::path::{Path, PathBuf};

use settings::Setting as _;
use warpui::{AppContext, SingletonEntity};

use crate::settings::AISettings;
use crate::terminal::cli_agent_sessions::CLIAgentSessionStatus;

const SYSTEM_SOUNDS_DIR: &str = "/System/Library/Sounds";
/// Played when a turn ends in a tab that asked to hear it, quieter than a system alert.
const FINISH_CHIME: &str = "Glass";
const FINISH_CHIME_VOLUME: f32 = 0.4;

/// Plays the sound configured for `status`, if any.
pub fn play_for_status(status: &CLIAgentSessionStatus, app: &AppContext) {
    let settings = AISettings::as_ref(app);
    let configured = match status {
        CLIAgentSessionStatus::Blocked { .. } => settings.cli_agent_sound_when_waiting.value(),
        CLIAgentSessionStatus::Success => settings.cli_agent_sound_when_finished.value(),
        CLIAgentSessionStatus::Failed { .. } => settings.cli_agent_sound_when_failed.value(),
        CLIAgentSessionStatus::InProgress | CLIAgentSessionStatus::Cancelled => return,
    };
    if let Some(path) = sound_path(configured) {
        play(&path, None);
    }
}

/// Whether a tab should chime when its agent's turn ends. A resumed session reports a finished
/// turn without one having run, and a configured finish sound already plays for every tab.
pub fn should_play_finish_chime(
    tab_wants_chime: bool,
    turn_started: bool,
    has_finish_sound: bool,
) -> bool {
    tab_wants_chime && turn_started && !has_finish_sound
}

pub fn has_finish_sound(app: &AppContext) -> bool {
    sound_path(
        AISettings::as_ref(app)
            .cli_agent_sound_when_finished
            .value(),
    )
    .is_some()
}

pub fn play_finish_chime() {
    if let Some(path) = sound_path(FINISH_CHIME) {
        play(&path, Some(FINISH_CHIME_VOLUME));
    }
}

/// Resolves a configured sound to a file: an absolute path to an existing file, or the name of a
/// macOS system sound such as `Glass`.
pub fn sound_path(configured: &str) -> Option<PathBuf> {
    let configured = configured.trim();
    if configured.is_empty() {
        return None;
    }
    let path = if configured.starts_with('/') {
        PathBuf::from(configured)
    } else if configured
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '_')
    {
        Path::new(SYSTEM_SOUNDS_DIR).join(format!("{configured}.aiff"))
    } else {
        return None;
    };
    path.is_file().then_some(path)
}

#[cfg(target_os = "macos")]
fn play(path: &Path, volume: Option<f32>) {
    let mut command = command::blocking::Command::new("afplay");
    if let Some(volume) = volume {
        command.arg("-v").arg(volume.to_string());
    }
    if let Err(err) = command.arg(path).spawn() {
        log::warn!("Failed to play agent session sound: {err}");
    }
}

#[cfg(not(target_os = "macos"))]
fn play(_path: &Path, _volume: Option<f32>) {}

#[cfg(test)]
#[path = "sounds_tests.rs"]
mod tests;
