//! A band above the active tab's content naming the session being typed into.
//!
//! With a dozen agent sessions open, the tab list alone does not stop a message from going to the
//! wrong one: the sidebar is out of the line of sight and several tabs can share a hue. The band
//! repeats the group, the tab name and the branch in large type, in the tab's color, across the top
//! of the session, and flashes when the active tab changes so a switch never goes unnoticed.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use instant::Instant;
use pathfinder_color::ColorU;
use warp_core::ui::color::coloru_with_opacity;
use warp_core::ui::theme::Fill as ThemeFill;
use warpui::r#async::Timer;
use warpui::elements::{
    ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, Element, Flex, MainAxisSize,
    ParentElement, Radius, Shrinkable, Text,
};
use warpui::fonts::{Properties, Weight};
use warpui::text_layout::ClipConfig;
use warpui::{AppContext, EntityId, ViewContext};

use super::Workspace;
use crate::appearance::Appearance;
use crate::code::buffer_location::LocalOrRemotePath;
use crate::pane_group::PaneGroup;
use crate::ui_components::color_dot::vivid_tab_color;
use crate::ui_components::icons::Icon;
use crate::workspace::group_tab_colors::display_tab_color;
use crate::workspace::session_signals::scanner::ResourceKind;
use crate::workspace::session_signals::ui::{
    self as signals_ui, CONTESTED_FILE_COLOR, SHARED_CHECKOUT_COLOR,
};

const FLASH_DURATION: Duration = Duration::from_millis(700);
const GROUP_FONT_SIZE: f32 = 11.;
const TITLE_FONT_SIZE: f32 = 16.;
const CHIP_FONT_SIZE: f32 = 11.;
/// How much the band is lightened toward white while it flashes, out of 255.
const FLASH_LIGHTEN: u8 = 90;
/// Hue shown for tabs without a color, so the band still marks the boundary of the prompt area.
const NEUTRAL_COLOR: ColorU = ColorU {
    r: 82,
    g: 87,
    b: 98,
    a: 255,
};

/// When each pane group last became the active tab, keyed by pane group.
static FLASH_STARTS: LazyLock<Mutex<HashMap<EntityId, Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

impl Workspace {
    /// Makes the band flash for the tab that just became active.
    pub(super) fn flash_session_identity_band(&self, ctx: &mut ViewContext<Self>) {
        let Some(tab) = self.tabs.get(self.active_tab_index) else {
            return;
        };
        if let Ok(mut starts) = FLASH_STARTS.lock() {
            starts.insert(tab.pane_group.id(), Instant::now());
        }
        let _ = ctx.spawn(Timer::after(FLASH_DURATION), |_, _, ctx| ctx.notify());
    }

    /// The band for the active tab, or `None` when there is a single tab and so nothing to confuse
    /// it with.
    pub(super) fn render_session_identity_band(
        &self,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Option<Box<dyn Element>> {
        if self.tabs.len() < 2 {
            return None;
        }
        let tab = self.tabs.get(self.active_tab_index)?;
        let pane_group = tab.pane_group.as_ref(app);
        let terminal_view = pane_group
            .active_session_view(app)
            .map(|terminal_view| terminal_view.as_ref(app));
        let title = tab_identity_name(pane_group, app);
        let group_name = tab
            .group_id
            .and_then(|group_id| self.tab_groups.get(&group_id))
            .and_then(|group| group.name.clone())
            .filter(|name| !name.trim().is_empty());
        let branch = terminal_view.and_then(|terminal_view| terminal_view.current_git_branch(app));
        let signals = self
            .session_signals(app)
            .into_iter()
            .nth(self.active_tab_index)
            .unwrap_or_default();
        let color = display_tab_color(tab, &self.tab_groups, app)
            .map(vivid_tab_color)
            .unwrap_or(NEUTRAL_COLOR);
        let is_flashing = FLASH_STARTS
            .lock()
            .ok()
            .and_then(|starts| starts.get(&tab.pane_group.id()).copied())
            .is_some_and(|start| start.elapsed() < FLASH_DURATION);

        let background = if is_flashing {
            signals_ui::lighten(color, FLASH_LIGHTEN)
        } else {
            color
        };
        let text_color = readable_text_on(background);
        let font_family = appearance.ui_font_family();

        let mut name = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(10.);
        if let Some(group_name) = group_name {
            name.add_child(
                Text::new_inline(group_name.to_uppercase(), font_family, GROUP_FONT_SIZE)
                    .with_color(coloru_with_opacity(text_color, 80))
                    .with_style(Properties::default().weight(Weight::Bold))
                    .finish(),
            );
        }
        name.add_child(
            Shrinkable::new(
                1.,
                Text::new_inline(title, font_family, TITLE_FONT_SIZE)
                    .with_clip(ClipConfig::ellipsis())
                    .with_color(text_color)
                    .with_style(Properties::default().weight(Weight::Bold))
                    .finish(),
            )
            .finish(),
        );

        let quiet_chip = coloru_with_opacity(ColorU::black(), 30);
        let mut chips: Vec<(Icon, String, ColorU)> = Vec::new();
        if let Some(branch) = branch {
            chips.push((Icon::GitBranch, branch, quiet_chip));
        }
        if !signals.contested_files.is_empty() {
            let count = signals.contested_files.len();
            let label = if count == 1 {
                "1 arquivo em conflito".to_owned()
            } else {
                format!("{count} arquivos em conflito")
            };
            chips.push((Icon::AlertTriangle, label, CONTESTED_FILE_COLOR));
        }
        if signals.shares_checkout {
            chips.push((
                Icon::Link,
                "pasta compartilhada".to_owned(),
                SHARED_CHECKOUT_COLOR,
            ));
        }
        if signals.held_resources.contains(&ResourceKind::Playwright) {
            chips.push((
                Icon::Globe,
                "navegador com esta sessão".to_owned(),
                quiet_chip,
            ));
        } else if let Some(holder) = self.browser_held_by_other_tab(app) {
            chips.push((
                Icon::Globe,
                format!("navegador com {}", holder.name),
                SHARED_CHECKOUT_COLOR,
            ));
        }

        let mut chip_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(6.);
        for (icon, label, chip_background) in chips {
            let chip_text = if chip_background.a == 255 {
                readable_text_on(chip_background)
            } else {
                text_color
            };
            chip_row.add_child(render_chip(
                icon,
                label,
                chip_text,
                chip_background,
                appearance,
            ));
        }

        let row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(16.)
            .with_child(Shrinkable::new(1., name.finish()).finish())
            .with_child(chip_row.finish())
            .finish();

        Some(
            Container::new(row)
                .with_background(ThemeFill::Solid(background))
                // Square at the bottom so the band reads as the top edge of the session below it.
                .with_corner_radius(CornerRadius::with_top(Radius::Pixels(12.)))
                .with_horizontal_padding(16.)
                .with_vertical_padding(4.)
                .with_margin_left(12.)
                .with_margin_right(12.)
                .with_margin_top(6.)
                .finish(),
        )
    }
}

/// The name a tab is known by: the one the user gave it (in the tab list or on the tab), otherwise
/// its repository folder. Agents set the terminal title to their own name, which would read the
/// same in every tab.
pub(super) fn tab_identity_name(pane_group: &PaneGroup, app: &AppContext) -> String {
    let renamed_pane = pane_group
        .pane_by_id(pane_group.focused_pane_id(app))
        .and_then(|pane| {
            pane.pane_configuration()
                .as_ref(app)
                .custom_vertical_tabs_title()
                .map(str::to_owned)
        })
        .filter(|name| !name.trim().is_empty());
    let repo_name = pane_group
        .active_session_view(app)
        .and_then(
            |terminal_view| match terminal_view.as_ref(app).current_repo_path()? {
                LocalOrRemotePath::Local(path) => path.file_name(),
                LocalOrRemotePath::Remote(_) => None,
            },
        )
        .map(|name| name.to_string_lossy().into_owned());
    renamed_pane
        .or_else(|| pane_group.custom_title(app))
        .or(repo_name)
        .unwrap_or_else(|| pane_group.display_title(app))
}

fn render_chip(
    icon: Icon,
    label: String,
    text_color: ColorU,
    background: ColorU,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(5.)
        .with_child(
            ConstrainedBox::new(icon.to_warpui_icon(ThemeFill::Solid(text_color)).finish())
                .with_width(CHIP_FONT_SIZE)
                .with_height(CHIP_FONT_SIZE)
                .finish(),
        )
        .with_child(
            Text::new_inline(label, appearance.ui_font_family(), CHIP_FONT_SIZE)
                .with_color(text_color)
                .with_style(Properties::default().weight(Weight::Semibold))
                .finish(),
        )
        .finish();
    Container::new(row)
        .with_background(ThemeFill::Solid(background))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
        .with_horizontal_padding(8.)
        .with_vertical_padding(2.)
        .finish()
}

/// Black or white, whichever reads better on `background`.
fn readable_text_on(background: ColorU) -> ColorU {
    let luminance = 0.2126 * f32::from(background.r)
        + 0.7152 * f32::from(background.g)
        + 0.0722 * f32::from(background.b);
    if luminance > 150. {
        ColorU::black()
    } else {
        ColorU::white()
    }
}
