//! Named sets of tabs that can be closed as a unit and brought back later.
//!
//! Closing a tab group destroys the only record of what was in it: which
//! directories its tabs sat in, what they were named, what they were running. A
//! group is how someone parks a whole line of work, so closing one should be
//! reversible rather than final. Every close writes the group's shape here
//! first, and reopening rebuilds it and takes it back off the list, which makes
//! a group behave like a set that opens and closes rather than a folder that
//! can only be emptied.
//!
//! Stored as JSON under [`state_dir`] rather than in the tabs database: this is
//! app state nobody edits by hand, it belongs to no single window's restore
//! snapshot, and it has to outlive every one of them.

use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

use serde::{Deserialize, Serialize};
use warp_core::paths::state_dir;

use crate::launch_configs::launch_config::TabTemplate;
use crate::tab::SelectedTabColor;

/// Upper bound on stored sets. Closing groups is cheap and reopening is
/// deliberate, so the list would otherwise grow without anyone ever pruning it.
/// Oldest entries fall off the end.
const MAX_SAVED_SETS: usize = 50;

const FILE_NAME: &str = "saved_tab_sets.json";

/// Shown when a group is closed without ever having been named.
const UNNAMED_SET: &str = "Untitled group";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedTabSet {
    pub name: String,
    #[serde(default)]
    pub color: SelectedTabColor,
    pub tabs: Vec<TabTemplate>,
}

impl SavedTabSet {
    /// Builds a set from a closed group. Returns `None` for an empty group,
    /// which has nothing worth restoring.
    pub fn new(
        name: Option<String>,
        color: SelectedTabColor,
        tabs: Vec<TabTemplate>,
    ) -> Option<Self> {
        if tabs.is_empty() {
            return None;
        }
        let name = name
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty())
            // An unnamed group still has recognizable tabs in it, so borrow the
            // first tab's title before falling back to a generic label.
            .or_else(|| tabs.first().and_then(|tab| tab.title.clone()))
            .unwrap_or_else(|| UNNAMED_SET.to_owned());
        Some(Self { name, color, tabs })
    }
}

/// In-memory mirror of the file, loaded once. Reading the disk on every menu
/// build would put file IO on a render path.
static SETS: LazyLock<Mutex<Vec<SavedTabSet>>> = LazyLock::new(|| Mutex::new(load_from_disk()));

fn file_path() -> PathBuf {
    state_dir().join(FILE_NAME)
}

fn load_from_disk() -> Vec<SavedTabSet> {
    let Ok(contents) = std::fs::read_to_string(file_path()) else {
        return Vec::new();
    };
    // A file written by a newer version, or one a user corrupted by hand, must
    // not take the app's tab handling down with it: an unreadable list of parked
    // groups is a lost convenience, not a failure worth surfacing.
    serde_json::from_str(&contents).unwrap_or_default()
}

fn write_to_disk(sets: &[SavedTabSet]) {
    let path = file_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(sets) {
        let _ = std::fs::write(path, json);
    }
}

/// Every stored set, newest first.
pub fn all() -> Vec<SavedTabSet> {
    SETS.lock().map(|sets| sets.clone()).unwrap_or_default()
}

/// Stores a set at the front of the list. A set whose name is already taken is
/// replaced: closing the same group twice should leave one entry holding its
/// latest shape, not two entries the user has to tell apart by guesswork.
pub fn save(set: SavedTabSet) {
    let Ok(mut sets) = SETS.lock() else {
        return;
    };
    sets.retain(|existing| existing.name != set.name);
    sets.insert(0, set);
    sets.truncate(MAX_SAVED_SETS);
    write_to_disk(&sets);
}

/// Removes a set and hands it back. Reopening consumes the entry so the list
/// only ever holds groups that are actually closed right now.
pub fn take(name: &str) -> Option<SavedTabSet> {
    let mut sets = SETS.lock().ok()?;
    let index = sets.iter().position(|set| set.name == name)?;
    let set = sets.remove(index);
    write_to_disk(&sets);
    Some(set)
}

#[cfg(test)]
#[path = "saved_tab_sets_tests.rs"]
mod tests;
