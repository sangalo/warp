use std::collections::HashMap;
use std::path::Path;

use super::{REPO_COLORS, repo_color, resolve};
use crate::tab::SelectedTabColor;
use crate::themes::theme::AnsiColorIdentifier;
use crate::workspace::tab_group::{TabGroup, TabGroupId};

fn groups_with(color: SelectedTabColor) -> (TabGroupId, HashMap<TabGroupId, TabGroup>) {
    let group = TabGroup {
        color,
        ..TabGroup::new()
    };
    let id = group.id;
    (id, HashMap::from([(id, group)]))
}

#[test]
fn tab_in_colored_group_takes_group_color_over_its_own() {
    let (id, groups) = groups_with(SelectedTabColor::Color(AnsiColorIdentifier::Magenta));
    assert_eq!(
        resolve(Some(id), Some(AnsiColorIdentifier::Green), &groups),
        Some(AnsiColorIdentifier::Magenta)
    );
    assert_eq!(
        resolve(Some(id), None, &groups),
        Some(AnsiColorIdentifier::Magenta)
    );
}

#[test]
fn tab_in_uncolored_group_keeps_its_own_color() {
    for color in [SelectedTabColor::Unset, SelectedTabColor::Cleared] {
        let (id, groups) = groups_with(color);
        assert_eq!(
            resolve(Some(id), Some(AnsiColorIdentifier::Green), &groups),
            Some(AnsiColorIdentifier::Green)
        );
        assert_eq!(resolve(Some(id), None, &groups), None);
    }
}

#[test]
fn tab_outside_any_group_keeps_its_own_color() {
    let (_, groups) = groups_with(SelectedTabColor::Color(AnsiColorIdentifier::Magenta));
    assert_eq!(
        resolve(None, Some(AnsiColorIdentifier::Green), &groups),
        Some(AnsiColorIdentifier::Green)
    );
    assert_eq!(resolve(None, None, &groups), None);
}

#[test]
fn tab_pointing_at_a_removed_group_keeps_its_own_color() {
    assert_eq!(
        resolve(
            Some(TabGroupId::new()),
            Some(AnsiColorIdentifier::Green),
            &HashMap::new()
        ),
        Some(AnsiColorIdentifier::Green)
    );
}

#[test]
fn repo_color_is_stable_and_avoids_signal_colors() {
    let root = Path::new("/Users/someone/code/acme");
    assert_eq!(repo_color(root), repo_color(root));
    let colors: Vec<_> = ["/a", "/b", "/c", "/d", "/e", "/f", "/g", "/h"]
        .iter()
        .map(|path| repo_color(Path::new(path)))
        .collect();
    assert!(colors.iter().all(|color| REPO_COLORS.contains(color)));
    assert!(colors.iter().any(|color| *color != colors[0]));
}
