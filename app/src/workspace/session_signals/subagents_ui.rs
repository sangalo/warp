//! The subagent counter drawn on a tab and the list of missions it opens on hover.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::{Duration, SystemTime};

use instant::Instant;
use pathfinder_color::ColorU;
use warp_core::ui::color::coloru_with_opacity;
use warp_core::ui::theme::Fill as ThemeFill;
use warpui::elements::{
    Border, ClippedScrollStateHandle, ClippedScrollable, ConstrainedBox, Container, CornerRadius,
    CrossAxisAlignment, DropShadow, Element, Empty, Fill as ElementFill, Flex, Hoverable,
    MainAxisAlignment, MainAxisSize, MouseStateHandle, ParentElement, Radius, SavePosition,
    ScrollbarWidth, Shrinkable, Text,
};
use warpui::fonts::{Properties, Weight};
use warpui::platform::Cursor;

use super::subagents::{Subagent, SubagentState};
use super::ui::{WORKING_STAR_COLOR, WORKING_TEXT_COLOR, clock_label, elapsed_label};
use crate::appearance::Appearance;
use crate::ui_components::icons::Icon;

const POPOVER_BACKGROUND: ColorU = ColorU {
    r: 28,
    g: 29,
    b: 31,
    a: 255,
};
const POPOVER_BORDER: ColorU = ColorU {
    r: 52,
    g: 55,
    b: 58,
    a: 255,
};
const POPOVER_DIVIDER: ColorU = ColorU {
    r: 42,
    g: 44,
    b: 46,
    a: 255,
};
const POPOVER_TEXT: ColorU = ColorU {
    r: 232,
    g: 233,
    b: 234,
    a: 255,
};
const POPOVER_SUB_TEXT: ColorU = ColorU {
    r: 154,
    g: 158,
    b: 162,
    a: 255,
};
const PROMPT_BACKGROUND: ColorU = ColorU {
    r: 19,
    g: 20,
    b: 21,
    a: 255,
};
const PROMPT_TEXT: ColorU = ColorU {
    r: 201,
    g: 204,
    b: 207,
    a: 255,
};

const POPOVER_WIDTH: f32 = 440.;
const PROMPT_MAX_HEIGHT: f32 = 120.;
const MAX_COUNTER_DOTS: usize = 3;
const COUNTER_DOT_SIZE: f32 = 4.;
const LIVE_DOT_SIZE: f32 = 6.;
/// How far the live dot's halo spreads around it.
const LIVE_DOT_HALO: f32 = 4.;
const DOT_CYCLE: Duration = Duration::from_millis(1800);
/// Delay between one counter dot lighting up and the next.
const DOT_STAGGER: Duration = Duration::from_millis(300);
/// Lets the pointer travel from the counter to the list without the list closing on the way.
const HOVER_OUT_DELAY: Duration = Duration::from_millis(300);

/// Shared start time, so every counter's dots light up in step.
static DOT_ANCHOR: LazyLock<Instant> = LazyLock::new(Instant::now);

thread_local! {
    /// Hover state of each tab's counter, by tab index.
    static COUNTER_MOUSE_STATES: RefCell<Vec<MouseStateHandle>> = const { RefCell::new(Vec::new()) };
    static POPOVER_MOUSE_STATE: MouseStateHandle = MouseStateHandle::default();
    /// The tab whose list is open.
    static OPEN_POPOVER_TAB: Cell<Option<usize>> = const { Cell::new(None) };
    /// The subagent whose prompt is shown in the open list.
    static EXPANDED_SUBAGENT: RefCell<Option<String>> = const { RefCell::new(None) };
    static ROW_MOUSE_STATES: RefCell<HashMap<String, MouseStateHandle>> = RefCell::new(HashMap::new());
    static PROMPT_SCROLL_STATE: ClippedScrollStateHandle = ClippedScrollStateHandle::default();
}

/// The id under which a tab's counter saves its position, for the list to sit beside it.
pub fn counter_position_id(tab_index: usize) -> String {
    format!("session_signals_subagents_{tab_index}")
}

fn counter_mouse_state(tab_index: usize) -> MouseStateHandle {
    COUNTER_MOUSE_STATES.with(|states| {
        let mut states = states.borrow_mut();
        if states.len() <= tab_index {
            states.resize_with(tab_index + 1, MouseStateHandle::default);
        }
        states[tab_index].clone()
    })
}

fn row_mouse_state(subagent_id: &str) -> MouseStateHandle {
    ROW_MOUSE_STATES.with(|states| {
        states
            .borrow_mut()
            .entry(subagent_id.to_owned())
            .or_default()
            .clone()
    })
}

fn is_hovered(state: &MouseStateHandle) -> bool {
    state.lock().is_ok_and(|state| state.is_hovered())
}

/// The tab whose list should show: the one whose counter is hovered, or the one already open while
/// the pointer is over the list itself.
pub fn open_popover_tab() -> Option<usize> {
    let hovered_counter =
        COUNTER_MOUSE_STATES.with(|states| states.borrow().iter().position(is_hovered));
    let open = hovered_counter.or_else(|| {
        POPOVER_MOUSE_STATE
            .with(is_hovered)
            .then(|| OPEN_POPOVER_TAB.get())
            .flatten()
    });
    if open != OPEN_POPOVER_TAB.get() {
        EXPANDED_SUBAGENT.with(|expanded| expanded.borrow_mut().take());
    }
    OPEN_POPOVER_TAB.set(open);
    open
}

pub fn is_popover_open() -> bool {
    OPEN_POPOVER_TAB.get().is_some()
}

/// Closes the list of the tab at `tab_index`, for when it no longer has subagents running. Its
/// counter and the list vanish without the pointer leaving them, so their hover is reset here.
pub fn close_popover(tab_index: usize) {
    OPEN_POPOVER_TAB.set(None);
    EXPANDED_SUBAGENT.with(|expanded| expanded.borrow_mut().take());
    for state in [
        POPOVER_MOUSE_STATE.with(Clone::clone),
        counter_mouse_state(tab_index),
    ] {
        if let Ok(mut state) = state.lock() {
            state.reset_hover_state();
        }
    }
}

/// How lit counter dot `dot_index` is, `elapsed` into the animation, from 0.3 to 1.
pub(super) fn dot_opacity(dot_index: usize, elapsed: Duration) -> f32 {
    let cycle = DOT_CYCLE.as_secs_f32();
    let shifted = elapsed.as_secs_f32() - dot_index as f32 * DOT_STAGGER.as_secs_f32();
    let t = shifted.rem_euclid(cycle) / cycle;
    let smoothstep = |x: f32| x * x * (3. - 2. * x);
    let level = if t < 0.3 {
        smoothstep(t / 0.3)
    } else {
        1. - smoothstep((t - 0.3) / 0.7)
    };
    0.3 + 0.7 * level
}

/// One dot per running subagent, up to three, lighting up in turn, followed by their number.
pub fn render_counter(
    tab_index: usize,
    running: usize,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let elapsed = DOT_ANCHOR.elapsed();
    let mut row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(3.);
    for dot_index in 0..running.min(MAX_COUNTER_DOTS) {
        let alpha = (dot_opacity(dot_index, elapsed) * 255.) as u8;
        row.add_child(render_circle(
            ColorU {
                a: alpha,
                ..WORKING_STAR_COLOR
            },
            COUNTER_DOT_SIZE,
        ));
    }
    row.add_child(
        Container::new(
            Text::new_inline(running.to_string(), appearance.ui_font_family(), 10.)
                .with_color(WORKING_TEXT_COLOR)
                .with_style(Properties::default().weight(Weight::Semibold))
                .finish(),
        )
        .with_margin_left(2.)
        .finish(),
    );
    let position_id = counter_position_id(tab_index);
    let counter = row.finish();
    let is_open = OPEN_POPOVER_TAB.get() == Some(tab_index);
    Hoverable::new(counter_mouse_state(tab_index), move |state| {
        let container = Container::new(counter)
            .with_horizontal_padding(3.)
            .with_vertical_padding(1.)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)));
        let container = if is_open || state.is_hovered() {
            container
                .with_background(ThemeFill::Solid(coloru_with_opacity(
                    WORKING_STAR_COLOR,
                    22,
                )))
                .with_border(Border::all(1.).with_border_fill(ThemeFill::Solid(
                    coloru_with_opacity(WORKING_STAR_COLOR, 60),
                )))
        } else {
            container.with_border(
                Border::all(1.).with_border_fill(ThemeFill::Solid(ColorU::transparent_black())),
            )
        };
        SavePosition::new(container.finish(), &position_id).finish()
    })
    .with_hover_out_delay(HOVER_OUT_DELAY)
    .finish()
}

pub(super) fn render_circle(color: ColorU, size: f32) -> Box<dyn Element> {
    ConstrainedBox::new(
        Container::new(Empty::new().finish())
            .with_background(ThemeFill::Solid(color))
            .with_corner_radius(CornerRadius::with_all(Radius::Percentage(50.)))
            .finish(),
    )
    .with_width(size)
    .with_height(size)
    .finish()
}

/// "Explore · background · 4 min 12 s", as parts so the time can take its own color.
pub(super) fn running_meta_parts(subagent: &Subagent, now: SystemTime) -> (String, String) {
    let mut parts = Vec::new();
    if !subagent.agent_type.is_empty() {
        parts.push(subagent.agent_type.clone());
    }
    if subagent.background {
        parts.push("background".to_owned());
    }
    let running_for = now.duration_since(subagent.started).unwrap_or_default();
    (parts.join(" · "), clock_label(running_for))
}

/// "general-purpose · rodou 6 min · terminou há 2 min".
pub(super) fn finished_meta(
    subagent: &Subagent,
    finished_at: SystemTime,
    now: SystemTime,
) -> String {
    let ran = finished_at
        .duration_since(subagent.started)
        .unwrap_or_default();
    let ran = if ran.as_secs() < 60 {
        format!("{} s", ran.as_secs())
    } else {
        elapsed_label(ran)
    };
    let ago = match elapsed_label(now.duration_since(finished_at).unwrap_or_default()).as_str() {
        "agora" => "terminou agora".to_owned(),
        ago => format!("terminou há {ago}"),
    };
    let mut parts = Vec::new();
    if !subagent.agent_type.is_empty() {
        parts.push(subagent.agent_type.clone());
    }
    parts.push(format!("rodou {ran}"));
    parts.push(ago);
    parts.join(" · ")
}

/// The list of `subagents` launched from the tab named `tab_name`: running ones with their
/// mission and time, each opening the full request on click, then the ones that just finished.
pub fn render_popover(
    tab_name: &str,
    subagents: &[Subagent],
    appearance: &Appearance,
) -> Box<dyn Element> {
    let font_family = appearance.ui_font_family();
    let now = SystemTime::now();
    let running: Vec<&Subagent> = subagents.iter().filter(|s| s.is_running()).collect();
    let finished: Vec<(&Subagent, SystemTime)> = subagents
        .iter()
        .filter_map(|subagent| match subagent.state {
            SubagentState::Finished { at } => Some((subagent, at)),
            SubagentState::Running => None,
        })
        .collect();
    let expanded = EXPANDED_SUBAGENT.with(|expanded| expanded.borrow().clone());

    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    column.add_child(
        Container::new(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(
                    Shrinkable::new(
                        1.,
                        Text::new_inline(format!("Sub-agentes de {tab_name}"), font_family, 13.)
                            .with_color(POPOVER_TEXT)
                            .with_style(Properties::default().weight(Weight::Semibold))
                            .finish(),
                    )
                    .finish(),
                )
                .with_child(
                    Text::new_inline(format!("{} rodando", running.len()), font_family, 11.)
                        .with_color(WORKING_TEXT_COLOR)
                        .finish(),
                )
                .finish(),
        )
        .with_horizontal_padding(10.)
        .with_padding_top(2.)
        .with_padding_bottom(8.)
        .with_margin_bottom(4.)
        .with_border(Border::bottom(1.).with_border_fill(ThemeFill::Solid(POPOVER_DIVIDER)))
        .finish(),
    );
    for subagent in running {
        let is_expanded = expanded.as_deref() == Some(subagent.id.as_str());
        column.add_child(render_running_row(subagent, is_expanded, now, appearance));
        if is_expanded {
            column.add_child(render_prompt(&subagent.prompt, appearance));
        }
    }
    if !finished.is_empty() {
        column.add_child(
            Container::new(
                Text::new_inline("TERMINARAM HÁ POUCO", font_family, 10.)
                    .with_color(POPOVER_SUB_TEXT)
                    .with_style(Properties::default().weight(Weight::Bold))
                    .finish(),
            )
            .with_horizontal_padding(10.)
            .with_padding_top(10.)
            .with_padding_bottom(2.)
            .with_margin_top(4.)
            .with_border(Border::top(1.).with_border_fill(ThemeFill::Solid(POPOVER_DIVIDER)))
            .finish(),
        );
        for (subagent, finished_at) in finished {
            column.add_child(render_finished_row(subagent, finished_at, now, appearance));
        }
    }

    let popover = column.finish();
    ConstrainedBox::new(
        Hoverable::new(POPOVER_MOUSE_STATE.with(Clone::clone), move |_| {
            Container::new(popover)
                .with_background(ThemeFill::Solid(POPOVER_BACKGROUND))
                .with_border(Border::all(1.).with_border_fill(ThemeFill::Solid(POPOVER_BORDER)))
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(10.)))
                .with_drop_shadow(DropShadow::default())
                .with_padding_top(10.)
                .with_horizontal_padding(6.)
                .with_padding_bottom(8.)
                .finish()
        })
        .with_hover_out_delay(HOVER_OUT_DELAY)
        .finish(),
    )
    .with_width(POPOVER_WIDTH)
    .finish()
}

fn render_running_row(
    subagent: &Subagent,
    is_expanded: bool,
    now: SystemTime,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let font_family = appearance.ui_font_family();
    let (meta, running_for) = running_meta_parts(subagent, now);
    let description = subagent.description.clone();
    let subagent_id = subagent.id.clone();
    let chevron = if is_expanded {
        Icon::ChevronDown
    } else {
        Icon::ChevronRight
    };
    Hoverable::new(row_mouse_state(&subagent.id), move |state| {
        let mut meta_row = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(6.);
        if !meta.is_empty() {
            meta_row.add_child(
                Text::new_inline(format!("{meta} ·"), font_family, 11.)
                    .with_color(POPOVER_SUB_TEXT)
                    .finish(),
            );
        }
        meta_row.add_child(
            Text::new_inline(running_for.clone(), font_family, 11.)
                .with_color(WORKING_TEXT_COLOR)
                .finish(),
        );
        let text = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(3.)
            .with_child(
                Text::new(description.clone(), font_family, 13.)
                    .with_color(POPOVER_TEXT)
                    .with_style(Properties::default().weight(Weight::Semibold))
                    .finish(),
            )
            .with_child(meta_row.finish())
            .finish();
        let row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(10.)
            .with_child(render_live_dot())
            .with_child(Shrinkable::new(1., text).finish())
            .with_child(
                Container::new(
                    ConstrainedBox::new(
                        chevron
                            .to_warpui_icon(ThemeFill::Solid(POPOVER_SUB_TEXT))
                            .finish(),
                    )
                    .with_width(12.)
                    .with_height(12.)
                    .finish(),
                )
                .with_margin_top(3.)
                .finish(),
            )
            .finish();
        let background = if state.is_hovered() {
            coloru_with_opacity(ColorU::white(), 5)
        } else {
            ColorU::transparent_black()
        };
        Container::new(row)
            .with_background(ThemeFill::Solid(background))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
            .with_vertical_padding(9.)
            .with_horizontal_padding(10.)
            .finish()
    })
    .on_click(move |ctx, _, _| {
        EXPANDED_SUBAGENT.with(|expanded| {
            let mut expanded = expanded.borrow_mut();
            *expanded = match expanded.as_deref() {
                Some(id) if id == subagent_id => None,
                Some(_) | None => Some(subagent_id.clone()),
            };
        });
        ctx.notify();
    })
    .with_cursor(Cursor::PointingHand)
    .finish()
}

/// A dot in Claude's orange with a halo that spreads and fades, marking a subagent still at work.
fn render_live_dot() -> Box<dyn Element> {
    let t = DOT_ANCHOR.elapsed().as_secs_f32() % DOT_CYCLE.as_secs_f32() / DOT_CYCLE.as_secs_f32();
    let halo_spread = LIVE_DOT_HALO * t;
    let halo = Container::new(render_circle(WORKING_STAR_COLOR, LIVE_DOT_SIZE))
        .with_uniform_padding(halo_spread)
        .with_background(ThemeFill::Solid(ColorU {
            a: (140. * (1. - t)) as u8,
            ..WORKING_STAR_COLOR
        }))
        .with_corner_radius(CornerRadius::with_all(Radius::Percentage(50.)))
        .finish();
    Container::new(halo)
        .with_uniform_padding(LIVE_DOT_HALO - halo_spread)
        .finish()
}

fn render_prompt(prompt: &str, appearance: &Appearance) -> Box<dyn Element> {
    let theme = appearance.theme();
    let text = Text::new(prompt.to_owned(), appearance.monospace_font_family(), 11.)
        .with_color(PROMPT_TEXT)
        .with_line_height_ratio(1.55)
        .finish();
    let scrollable = ClippedScrollable::vertical(
        PROMPT_SCROLL_STATE.with(Clone::clone),
        Container::new(text)
            .with_vertical_padding(9.)
            .with_horizontal_padding(11.)
            .finish(),
        ScrollbarWidth::Auto,
        theme.nonactive_ui_detail().into(),
        theme.active_ui_detail().into(),
        ElementFill::None,
    )
    .with_overlayed_scrollbar()
    .finish();
    Container::new(
        ConstrainedBox::new(scrollable)
            .with_max_height(PROMPT_MAX_HEIGHT)
            .finish(),
    )
    .with_background(ThemeFill::Solid(PROMPT_BACKGROUND))
    .with_border(Border::all(1.).with_border_fill(ThemeFill::Solid(POPOVER_DIVIDER)))
    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
    .with_margin_left(26.)
    .with_margin_right(10.)
    .with_margin_bottom(8.)
    .finish()
}

fn render_finished_row(
    subagent: &Subagent,
    finished_at: SystemTime,
    now: SystemTime,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let font_family = appearance.ui_font_family();
    let dimmed = |color: ColorU| coloru_with_opacity(color, 70);
    let text = Flex::column()
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_spacing(3.)
        .with_child(
            Text::new(subagent.description.clone(), font_family, 13.)
                .with_color(dimmed(PROMPT_TEXT))
                .finish(),
        )
        .with_child(
            Text::new_inline(finished_meta(subagent, finished_at, now), font_family, 11.)
                .with_color(dimmed(POPOVER_SUB_TEXT))
                .finish(),
        )
        .finish();
    Container::new(
        Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(10.)
            .with_child(
                Container::new(
                    ConstrainedBox::new(
                        Icon::Check
                            .to_warpui_icon(ThemeFill::Solid(dimmed(POPOVER_SUB_TEXT)))
                            .finish(),
                    )
                    .with_width(10.)
                    .with_height(10.)
                    .finish(),
                )
                .with_margin_top(3.)
                .finish(),
            )
            .with_child(Shrinkable::new(1., text).finish())
            .finish(),
    )
    .with_vertical_padding(7.)
    .with_horizontal_padding(10.)
    .finish()
}

#[cfg(test)]
#[path = "subagents_ui_tests.rs"]
mod tests;
