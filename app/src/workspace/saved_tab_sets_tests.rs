use std::path::PathBuf;

use super::{SavedTabSet, UNNAMED_SET};
use crate::launch_configs::launch_config::{PaneTemplateType, TabTemplate};
use crate::tab::SelectedTabColor;

fn tab(title: Option<&str>) -> TabTemplate {
    TabTemplate {
        title: title.map(str::to_owned),
        layout: PaneTemplateType::PaneTemplate {
            cwd: PathBuf::from("/tmp"),
            commands: Vec::new(),
            is_focused: None,
            pane_mode: Default::default(),
            shell: None,
        },
        commands: Vec::new(),
        color: None,
    }
}

#[test]
fn a_named_group_keeps_its_name() {
    let set = SavedTabSet::new(
        Some("  FINTECH  ".to_owned()),
        SelectedTabColor::Unset,
        vec![tab(Some("api"))],
    )
    .expect("group has tabs");
    assert_eq!(set.name, "FINTECH");
}

#[test]
fn an_unnamed_group_borrows_its_first_tab_title() {
    let set = SavedTabSet::new(
        None,
        SelectedTabColor::Unset,
        vec![tab(Some("deploy")), tab(Some("logs"))],
    )
    .expect("group has tabs");
    assert_eq!(set.name, "deploy");
}

#[test]
fn a_group_with_nothing_to_name_it_falls_back() {
    let set = SavedTabSet::new(
        Some("   ".to_owned()),
        SelectedTabColor::Unset,
        vec![tab(None)],
    )
    .expect("group has tabs");
    assert_eq!(set.name, UNNAMED_SET);
}

#[test]
fn an_empty_group_is_not_worth_storing() {
    assert!(
        SavedTabSet::new(Some("FINTECH".to_owned()), SelectedTabColor::Unset, vec![]).is_none()
    );
}
