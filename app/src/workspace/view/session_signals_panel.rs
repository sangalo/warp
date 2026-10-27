//! Connects session signals to the workspace's tabs for the vertical tabs panel and the title bar.

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};

use pathfinder_color::ColorU;
use pathfinder_geometry::vector::vec2f;
use warp_core::ui::color::coloru_with_opacity;
use warp_core::ui::theme::Fill as ThemeFill;
use warpui::elements::{
    Border, ChildAnchor, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, Dash,
    Element, Flex, Hoverable, MainAxisAlignment, MainAxisSize, MouseStateHandle, OffsetPositioning,
    ParentAnchor, ParentElement, ParentOffsetBounds, Radius, SavePosition, Stack, Text,
};
use warpui::fonts::{Properties, Weight};
use warpui::platform::Cursor;
use warpui::{AppContext, EntityId, SingletonEntity, ViewContext};

use super::Workspace;
use super::session_identity_band::tab_identity_name;
use crate::appearance::Appearance;
use crate::terminal::cli_agent_sessions::CLIAgentSessionsModel;
use crate::ui_components::color_dot::vivid_tab_color;
use crate::ui_components::icons::Icon;
use crate::user_config::WarpConfig;
use crate::workspace::action::WorkspaceAction;
use crate::workspace::group_tab_colors::display_tab_color;
use crate::workspace::session_signals::scanner::{ResourceKind, ResourceState};
use crate::workspace::session_signals::server::{self, ServerPaneObservation};
use crate::workspace::session_signals::subagents::Subagent;
use crate::workspace::session_signals::ui::{
    ResourceHolder, ResourceRow, WAITING_COLOR, pulsing, render_resources_panel, render_tab_badges,
    render_title_bar_resources, tab_border_color,
};
use crate::workspace::session_signals::{
    SessionSignalsModel, TabSignals, WaitReason, sounds, subagents_ui,
};

const SIGNAL_BORDER_WIDTH: f32 = 1.5;
pub(super) const TITLE_BAR_RESOURCES_POSITION_ID: &str = "session_signals_title_bar_resources";
/// Where the badges sit inside a tab's row, from its top right corner.
const BADGE_INSET: f32 = 7.;
/// Where the subagent counter sits inside a tab's row, from its bottom right corner.
const SUBAGENT_COUNTER_RIGHT_INSET: f32 = 7.;
const SUBAGENT_COUNTER_BOTTOM_INSET: f32 = 9.;

static IS_RESOURCES_PANEL_OPEN: AtomicBool = AtomicBool::new(false);

/// Name of the tab config that opens an agent session in its own worktree, branch and port.
const NEW_ISOLATED_SESSION_CONFIG: &str = "Nova sessão isolada";

thread_local! {
    static NEW_ISOLATED_SESSION_MOUSE_STATE: MouseStateHandle = MouseStateHandle::default();
    /// Signals computed once per render of the tab list, read back by each tab's row. Computing
    /// them per row would redo the cross-tab comparison for every tab.
    static SIGNALS_FOR_RENDER: RefCell<Vec<TabSignals>> = const { RefCell::new(Vec::new()) };
}

pub(super) fn toggle_resources_panel() {
    IS_RESOURCES_PANEL_OPEN.fetch_xor(true, Ordering::Relaxed);
}

/// Makes `signals` available to [`signals_for_tab`] for the rest of this render.
pub(super) fn set_signals_for_render(signals: Vec<TabSignals>) {
    SIGNALS_FOR_RENDER.with(|cell| *cell.borrow_mut() = signals);
}

/// The signals of the tab at `tab_index`, as set by [`set_signals_for_render`].
pub(super) fn signals_for_tab(tab_index: usize) -> Option<TabSignals> {
    SIGNALS_FOR_RENDER.with(|cell| cell.borrow().get(tab_index).cloned())
}

impl Workspace {
    /// Chimes when the agent in `terminal_view_id` finishes a turn in a tab of this window that has
    /// the chime on.
    pub(super) fn maybe_play_finish_chime(&self, terminal_view_id: EntityId, app: &AppContext) {
        let tab_wants_chime = self.tabs.iter().any(|tab| {
            tab.finish_chime
                && tab
                    .pane_group
                    .as_ref(app)
                    .contains_terminal_view(terminal_view_id, app)
        });
        if sounds::should_play_finish_chime(
            tab_wants_chime,
            SessionSignalsModel::as_ref(app).has_started_turn(terminal_view_id),
            sounds::has_finish_sound(app),
        ) {
            sounds::play_finish_chime();
        }
    }

    /// Reads the output of every pane in this window that runs a server, for its card.
    pub(super) fn observe_server_panes(&self, ctx: &mut ViewContext<Self>) {
        let model = SessionSignalsModel::handle(ctx);
        let server_panes: HashSet<String> = model.as_ref(ctx).server_panes().cloned().collect();
        let mut observations = Vec::new();
        for (tab_index, tab) in self.tabs.iter().enumerate() {
            for (pane, terminal_view) in tab
                .pane_group
                .as_ref(ctx)
                .terminal_views_by_session_uuid(ctx)
            {
                if !server_panes.contains(&pane) {
                    continue;
                }
                let is_agent_pane = CLIAgentSessionsModel::as_ref(ctx)
                    .session(terminal_view.id())
                    .is_some();
                let (active_block, is_executing, output) = terminal_view
                    .as_ref(ctx)
                    .active_block_output_tail(server::OUTPUT_ROWS_TO_SCAN);
                observations.push(ServerPaneObservation {
                    pane,
                    active_block,
                    is_executing,
                    last_request: server::parse_last_request(&output),
                    is_active_tab: tab_index == self.active_tab_index,
                    is_agent_pane,
                });
            }
        }
        if !observations.is_empty() {
            model.update(ctx, |model, ctx| model.observe_servers(observations, ctx));
        }
    }

    /// Signals for every tab, indexed like `self.tabs`.
    pub(super) fn session_signals(&self, app: &AppContext) -> Vec<TabSignals> {
        let model = SessionSignalsModel::as_ref(app);
        let facts: Vec<_> = self
            .tabs
            .iter()
            .map(|tab| model.tab_facts(&tab.pane_group, app))
            .collect();
        model.tab_signals(&facts)
    }

    /// Every watched resource with who holds it, naming the tab when a process started from one.
    fn session_resource_rows(&self, app: &AppContext) -> Vec<ResourceRow> {
        let model = SessionSignalsModel::as_ref(app);
        model
            .resources()
            .iter()
            .map(|(kind, state)| {
                let holder = match state {
                    ResourceState::Free => None,
                    ResourceState::InUse { pane, process } => Some(
                        pane.as_deref()
                            .and_then(|pane_uuid| self.resource_holder_tab(pane_uuid, app))
                            .unwrap_or_else(|| ResourceHolder {
                                tab_index: None,
                                name: process.clone(),
                                color: None,
                            }),
                    ),
                };
                ResourceRow {
                    kind: *kind,
                    holder,
                    since: model.resource_since(*kind),
                }
            })
            .collect()
    }

    fn resource_holder_tab(&self, pane_uuid: &str, app: &AppContext) -> Option<ResourceHolder> {
        self.tabs.iter().enumerate().find_map(|(tab_index, tab)| {
            let pane_group = tab.pane_group.as_ref(app);
            pane_group
                .terminal_views_by_session_uuid(app)
                .iter()
                .any(|(uuid, _)| uuid == pane_uuid)
                .then(|| ResourceHolder {
                    tab_index: Some(tab_index),
                    name: tab_identity_name(pane_group, app),
                    color: display_tab_color(tab, &self.tab_groups, app).map(vivid_tab_color),
                })
        })
    }

    /// The tab holding the Playwright browser, when it is not the active tab.
    pub(super) fn browser_held_by_other_tab(&self, app: &AppContext) -> Option<ResourceHolder> {
        self.session_resource_rows(app)
            .into_iter()
            .find(|row| row.kind == ResourceKind::Playwright)?
            .holder
            .filter(|holder| holder.tab_index != Some(self.active_tab_index))
    }

    /// The resource pills shown in the title bar.
    pub(super) fn render_title_bar_resources(
        &self,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Option<Box<dyn Element>> {
        let pills = render_title_bar_resources(&self.session_resource_rows(app), appearance)?;
        Some(
            Container::new(SavePosition::new(pills, TITLE_BAR_RESOURCES_POSITION_ID).finish())
                .with_margin_right(8.)
                .finish(),
        )
    }

    /// The resources panel, when the user opened it from the title bar.
    pub(super) fn render_session_resources_panel(
        &self,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Option<Box<dyn Element>> {
        if !IS_RESOURCES_PANEL_OPEN.load(Ordering::Relaxed) {
            return None;
        }
        let browser_note = self.browser_held_by_other_tab(app).map(|holder| {
            let active_tab = self
                .tabs
                .get(self.active_tab_index)
                .map(|tab| tab_identity_name(tab.pane_group.as_ref(app), app))
                .unwrap_or_default();
            (active_tab, holder.name)
        });
        Some(render_resources_panel(
            &self.session_resource_rows(app),
            SessionSignalsModel::as_ref(app).idle_playwright_servers(),
            browser_note,
            appearance,
        ))
    }

    /// The missions of the subagents of the tab whose counter is hovered, with the id of the
    /// counter's saved position for the list to sit beside it.
    pub(super) fn render_subagents_popover(
        &self,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Option<(Box<dyn Element>, String)> {
        let tab_index = subagents_ui::open_popover_tab()?;
        let popover = self.tabs.get(tab_index).and_then(|tab| {
            let signals = self.session_signals(app).into_iter().nth(tab_index)?;
            signals.subagents.iter().any(Subagent::is_running).then(|| {
                subagents_ui::render_popover(
                    &tab_identity_name(tab.pane_group.as_ref(app), app),
                    &signals.subagents,
                    appearance,
                )
            })
        });
        match popover {
            Some(popover) => Some((popover, subagents_ui::counter_position_id(tab_index))),
            None => {
                subagents_ui::close_popover(tab_index);
                None
            }
        }
    }

    /// A button opening the "Nova sessão isolada" tab config, when that config is installed.
    pub(super) fn render_new_isolated_session_button(
        &self,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Option<Box<dyn Element>> {
        let tab_config = WarpConfig::as_ref(app)
            .tab_configs()
            .iter()
            .find(|config| config.name == NEW_ISOLATED_SESSION_CONFIG)?
            .clone();
        let theme = appearance.theme();
        let text_color = theme.main_text_color(theme.background());
        let border_color: ColorU = theme.sub_text_color(theme.background()).into();
        let font_family = appearance.ui_font_family();
        let mouse_state = NEW_ISOLATED_SESSION_MOUSE_STATE.with(Clone::clone);
        Some(
            Hoverable::new(mouse_state, move |state| {
                let label = Flex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_main_axis_alignment(MainAxisAlignment::Center)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(6.)
                    .with_child(
                        ConstrainedBox::new(Icon::Plus.to_warpui_icon(text_color).finish())
                            .with_width(12.)
                            .with_height(12.)
                            .finish(),
                    )
                    .with_child(
                        Text::new_inline(NEW_ISOLATED_SESSION_CONFIG, font_family, 12.)
                            .with_color(text_color.into())
                            .with_style(Properties::default().weight(Weight::Semibold))
                            .finish(),
                    )
                    .finish();
                let background = if state.is_hovered() {
                    coloru_with_opacity(border_color, 15)
                } else {
                    ColorU::transparent_black()
                };
                Container::new(label)
                    .with_background(ThemeFill::Solid(background))
                    .with_border(dashed_border(coloru_with_opacity(border_color, 60)))
                    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.)))
                    .with_vertical_padding(11.)
                    .with_margin_top(8.)
                    .with_horizontal_margin(8.)
                    .finish()
            })
            .on_click(move |ctx, _, _| {
                ctx.dispatch_typed_action(WorkspaceAction::SelectTabConfig(tab_config.clone()));
            })
            .with_cursor(Cursor::PointingHand)
            .finish(),
        )
    }
}

fn dashed_border(color: ColorU) -> Border {
    let mut border = Border::all(1.).with_border_fill(ThemeFill::Solid(color));
    border.dash = Some(Dash {
        dash_length: 4.,
        gap_length: 3.,
        force_consistent_gap_length: false,
    });
    border
}

/// Puts `signals`' badges at the top right of the tab at `tab_index`, its running subagents at the
/// bottom right, and outlines it in the color of its strongest signal, pulsing while the tab waits
/// on the user. `finish_chime` adds a speaker badge, since the toggle itself only shows on hover.
pub(super) fn decorate_tab_with_signals(
    content: Box<dyn Element>,
    tab_index: usize,
    signals: Option<&TabSignals>,
    finish_chime: bool,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let default_signals = TabSignals::default();
    let signals = match signals {
        Some(signals) => signals,
        None if finish_chime => &default_signals,
        None => return content,
    };
    let running_subagents = signals.subagents.iter().filter(|s| s.is_running()).count();
    let content = if running_subagents == 0 {
        content
    } else {
        let mut stack = Stack::new().with_child(content);
        stack.add_positioned_overlay_child(
            subagents_ui::render_counter(tab_index, running_subagents, appearance),
            OffsetPositioning::offset_from_parent(
                vec2f(
                    -SUBAGENT_COUNTER_RIGHT_INSET,
                    -SUBAGENT_COUNTER_BOTTOM_INSET,
                ),
                ParentOffsetBounds::ParentByPosition,
                ParentAnchor::BottomRight,
                ChildAnchor::BottomRight,
            ),
        );
        stack.finish()
    };
    let badges = render_tab_badges(signals, tab_index, finish_chime, appearance);
    let border_color = tab_border_color(signals).map(|color| {
        if color == WAITING_COLOR && matches!(signals.waiting, Some((WaitReason::Blocked, _))) {
            pulsing(color)
        } else {
            color
        }
    });
    if badges.is_none() && border_color.is_none() {
        return content;
    }

    let container = Container::new(content);
    let outlined = match border_color {
        Some(color) => container
            .with_foreground_border(
                Border::all(SIGNAL_BORDER_WIDTH).with_border_fill(ThemeFill::Solid(color)),
            )
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.)))
            .finish(),
        None => container.finish(),
    };
    match badges {
        Some(badges) => {
            let mut stack = Stack::new().with_child(outlined);
            stack.add_positioned_overlay_child(
                badges,
                OffsetPositioning::offset_from_parent(
                    vec2f(-BADGE_INSET, BADGE_INSET),
                    ParentOffsetBounds::ParentByPosition,
                    ParentAnchor::TopRight,
                    ChildAnchor::TopRight,
                ),
            );
            stack.finish()
        }
        None => outlined,
    }
}
