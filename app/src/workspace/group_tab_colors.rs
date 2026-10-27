//! Tabs in a colored group are drawn in the group's color.
//!
//! With many sessions open, the color is how a tab is matched to its project at a glance, so every
//! tab of a group shares it: a tab dragged into the group takes it on, and changing the group's
//! color recolors all of its tabs. The color is resolved when drawing rather than written onto the
//! tabs, so a tab keeps its own color underneath and shows it again once it leaves the group.
//! A tab that was never given a color takes one derived from its repository, so tabs still differ
//! by project without any setup.

use std::collections::HashMap;
use std::path::Path;

use warpui::{AppContext, ViewHandle};

use crate::code::buffer_location::LocalOrRemotePath;
use crate::pane_group::PaneGroup;
use crate::tab::{SelectedTabColor, TabData};
use crate::themes::theme::AnsiColorIdentifier;
use crate::workspace::tab_group::{TabGroup, TabGroupId};

/// Colors a repository can be given. Red and yellow are left out because they mean a file conflict
/// and a session waiting on the user.
const REPO_COLORS: [AnsiColorIdentifier; 4] = [
    AnsiColorIdentifier::Blue,
    AnsiColorIdentifier::Magenta,
    AnsiColorIdentifier::Cyan,
    AnsiColorIdentifier::Green,
];

/// The color of `tab`'s group, if it is in a group that has one.
pub fn group_color(
    tab: &TabData,
    tab_groups: &HashMap<TabGroupId, TabGroup>,
) -> Option<AnsiColorIdentifier> {
    color_of_group(tab.group_id, tab_groups)
}

/// The color `tab` is drawn with: its group's color, else its own.
pub fn effective_tab_color(
    tab: &TabData,
    tab_groups: &HashMap<TabGroupId, TabGroup>,
) -> Option<AnsiColorIdentifier> {
    resolve(tab.group_id, tab.color(), tab_groups)
}

/// The color `tab` is drawn with, falling back to its repository's color when it has none of its
/// own and was not explicitly cleared.
pub fn display_tab_color(
    tab: &TabData,
    tab_groups: &HashMap<TabGroupId, TabGroup>,
    app: &AppContext,
) -> Option<AnsiColorIdentifier> {
    if let Some(color) = effective_tab_color(tab, tab_groups) {
        return Some(color);
    }
    match tab.selected_color {
        SelectedTabColor::Cleared => None,
        SelectedTabColor::Unset | SelectedTabColor::Color(_) => {
            pane_group_repo_color(&tab.pane_group, app)
        }
    }
}

/// The color of the repository the active session of `pane_group` is in.
pub fn pane_group_repo_color(
    pane_group: &ViewHandle<PaneGroup>,
    app: &AppContext,
) -> Option<AnsiColorIdentifier> {
    let terminal_view = pane_group.as_ref(app).active_session_view(app)?;
    match terminal_view.as_ref(app).current_repo_path()? {
        LocalOrRemotePath::Local(path) => Some(repo_color(path)),
        LocalOrRemotePath::Remote(_) => None,
    }
}

/// A fixed color for `repo_root`, the same on every run.
pub fn repo_color(repo_root: &Path) -> AnsiColorIdentifier {
    // FNV-1a: `DefaultHasher` makes no promise to stay the same across Rust releases.
    let hash = repo_root
        .as_os_str()
        .as_encoded_bytes()
        .iter()
        .fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        });
    REPO_COLORS[(hash % REPO_COLORS.len() as u64) as usize]
}

fn color_of_group(
    group_id: Option<TabGroupId>,
    tab_groups: &HashMap<TabGroupId, TabGroup>,
) -> Option<AnsiColorIdentifier> {
    group_id
        .and_then(|group_id| tab_groups.get(&group_id))
        .and_then(|group| group.color.resolve(None))
}

fn resolve(
    group_id: Option<TabGroupId>,
    own_color: Option<AnsiColorIdentifier>,
    tab_groups: &HashMap<TabGroupId, TabGroup>,
) -> Option<AnsiColorIdentifier> {
    color_of_group(group_id, tab_groups).or(own_color)
}

#[cfg(test)]
#[path = "group_tab_colors_tests.rs"]
mod tests;
