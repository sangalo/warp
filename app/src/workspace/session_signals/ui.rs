//! Renders session signals in the vertical tabs panel and the title bar.

use std::cell::RefCell;
use std::f32::consts::TAU;
use std::sync::LazyLock;
use std::time::Duration;

use chrono::{DateTime, Local};
use instant::Instant;
use pathfinder_color::ColorU;
use pathfinder_geometry::vector::vec2f;
use warp_core::ui::color::coloru_with_opacity;
use warp_core::ui::theme::Fill as ThemeFill;
use warpui::elements::{
    Border, ChildAnchor, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, Element,
    Empty, Flex, Highlight, Hoverable, MainAxisAlignment, MainAxisSize, MouseStateHandle,
    OffsetPositioning, ParentAnchor, ParentElement, ParentOffsetBounds, Radius, Shrinkable, Stack,
    Text,
};
use warpui::fonts::{Properties, Weight};
use warpui::platform::Cursor;
use warpui::text_layout::{ClipConfig, TextStyle};
use warpui::ui_components::components::UiComponent;

use super::scanner::ResourceKind;
use super::server::{ServerLine, ServerTrack};
use super::subagents_ui::render_circle;
use super::{ServerBadge, TabSignals, WaitReason};
use crate::appearance::Appearance;
use crate::ui_components::icons::Icon;
use crate::workspace::action::WorkspaceAction;

pub const SHARED_CHECKOUT_COLOR: ColorU = ColorU {
    r: 245,
    g: 158,
    b: 11,
    a: 255,
};
pub const CONTESTED_FILE_COLOR: ColorU = ColorU {
    r: 239,
    g: 68,
    b: 68,
    a: 255,
};
pub const WAITING_COLOR: ColorU = ColorU {
    r: 250,
    g: 204,
    b: 21,
    a: 255,
};
/// Gold of the raised hand on a tab whose agent finished its turn and waits for the next step.
/// Gray enough to sit quietly next to the signal badges.
const FINISH_CHIME_BADGE_COLOR: ColorU = ColorU {
    r: 156,
    g: 163,
    b: 175,
    a: 255,
};
const SERVER_LINE_FONT_SIZE: f32 = 11.;
const SERVER_DOT_SIZE: f32 = 7.;
/// How far the ring around a server's dot spreads past the dot when a request comes in.
const SERVER_RING_SPREAD: f32 = 7.;
const SERVER_UP_COLOR: ColorU = ColorU {
    r: 34,
    g: 197,
    b: 94,
    a: 255,
};
const SERVER_DOWN_COLOR: ColorU = ColorU {
    r: 239,
    g: 68,
    b: 68,
    a: 255,
};
const SERVER_DOWN_TEXT_COLOR: ColorU = ColorU {
    r: 252,
    g: 165,
    b: 165,
    a: 255,
};
const SERVER_LINE_TEXT_COLOR: ColorU = ColorU {
    r: 163,
    g: 163,
    b: 163,
    a: 255,
};
const SERVER_REQUEST_TEXT_COLOR: ColorU = ColorU {
    r: 229,
    g: 231,
    b: 235,
    a: 255,
};
const SERVER_ERROR_COLOR: ColorU = ColorU {
    r: 220,
    g: 38,
    b: 38,
    a: 255,
};
const SERVER_WARNING_COLOR: ColorU = ColorU {
    r: 251,
    g: 191,
    b: 36,
    a: 255,
};
const SERVER_WARNING_TEXT_COLOR: ColorU = ColorU {
    r: 31,
    g: 19,
    b: 0,
    a: 255,
};
const PORT_BADGE_BACKGROUND: ColorU = ColorU {
    r: 76,
    g: 58,
    b: 143,
    a: 255,
};
const PORT_BADGE_TEXT: ColorU = ColorU {
    r: 237,
    g: 233,
    b: 254,
    a: 255,
};
const PORT_BADGE_DOWN_BACKGROUND: ColorU = ColorU {
    r: 42,
    g: 42,
    b: 46,
    a: 255,
};
const PORT_BADGE_DOWN_TEXT: ColorU = ColorU {
    r: 156,
    g: 163,
    b: 175,
    a: 255,
};
const SHARED_CHECKOUT_TOOLTIP: &str = "Pasta git dividida com outra aba";
const FINISHED_HAND_COLOR: ColorU = ColorU {
    r: 251,
    g: 191,
    b: 36,
    a: 255,
};
/// Light yellow for text about waiting sessions, readable on the dark sidebar.
const WAITING_TEXT_COLOR: ColorU = ColorU {
    r: 253,
    g: 230,
    b: 138,
    a: 255,
};
/// Background of the badge marking a tab that holds a resource.
const RESOURCE_BADGE_BACKGROUND: ColorU = ColorU {
    r: 30,
    g: 27,
    b: 75,
    a: 255,
};
const RESOURCE_BADGE_TEXT: ColorU = ColorU {
    r: 199,
    g: 210,
    b: 254,
    a: 255,
};
const FREE_TEXT_COLOR: ColorU = ColorU {
    r: 134,
    g: 239,
    b: 172,
    a: 255,
};
const LINK_COLOR: ColorU = ColorU {
    r: 165,
    g: 180,
    b: 252,
    a: 255,
};
const PANEL_BACKGROUND: ColorU = ColorU {
    r: 23,
    g: 24,
    b: 28,
    a: 255,
};
const PANEL_DIVIDER: ColorU = ColorU {
    r: 38,
    g: 39,
    b: 45,
    a: 255,
};
const PANEL_SUB_TEXT: ColorU = ColorU {
    r: 156,
    g: 163,
    b: 175,
    a: 255,
};
const PANEL_TEXT: ColorU = ColorU {
    r: 230,
    g: 231,
    b: 235,
    a: 255,
};

const BADGE_FONT_SIZE: f32 = 10.;
const PILL_FONT_SIZE: f32 = 11.;
const PULSE_PERIOD: Duration = Duration::from_millis(1600);
const RESOURCES_PANEL_WIDTH: f32 = 600.;

static PULSE_EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);

thread_local! {
    /// Hover state for each "ir para a aba" link of the resources panel, by row position.
    static RESOURCE_LINK_MOUSE_STATES: RefCell<Vec<MouseStateHandle>> = const { RefCell::new(Vec::new()) };
    /// Hover state for each tab's shared-checkout link icon, by tab position.
    static SHARED_CHECKOUT_MOUSE_STATES: RefCell<Vec<MouseStateHandle>> = const { RefCell::new(Vec::new()) };
    /// Hover state for each tab's server port badge, by tab position.
    static PORT_BADGE_MOUSE_STATES: RefCell<Vec<MouseStateHandle>> = const { RefCell::new(Vec::new()) };
    static TITLE_BAR_RESOURCES_MOUSE_STATE: MouseStateHandle = MouseStateHandle::default();
}

/// Where the pulse of waiting tabs is, from 0 (faint) to 1 (strong).
pub fn pulse_level() -> f32 {
    let phase = PULSE_EPOCH.elapsed().as_secs_f32() / PULSE_PERIOD.as_secs_f32();
    (1. - (phase * TAU).cos()) / 2.
}

/// `color` with its opacity following the pulse.
pub fn pulsing(color: ColorU) -> ColorU {
    ColorU {
        a: (90. + 165. * pulse_level()) as u8,
        ..color
    }
}

/// The border that marks a tab needing attention, strongest signal first.
pub fn tab_border_color(signals: &TabSignals) -> Option<ColorU> {
    if !signals.contested_files.is_empty() {
        Some(CONTESTED_FILE_COLOR)
    } else if matches!(signals.waiting, Some((WaitReason::Blocked, _))) {
        Some(WAITING_COLOR)
    } else {
        None
    }
}

/// Short badges describing `signals`, drawn at the top right of the tab at `tab_index`, or `None`
/// when there is nothing to show. `finish_chime` adds a speaker for a tab that chimes when its
/// agent finishes.
pub fn render_tab_badges(
    signals: &TabSignals,
    tab_index: usize,
    finish_chime: bool,
    appearance: &Appearance,
) -> Option<Box<dyn Element>> {
    let mut badges: Vec<Box<dyn Element>> = Vec::new();
    if let Some(server) = signals.server {
        badges.push(render_port_badge(server, tab_index, appearance));
    }
    for resource in &signals.held_resources {
        if matches!((resource, signals.server), (ResourceKind::Port(port), Some(server)) if *port == server.port)
        {
            continue;
        }
        let icon = match resource {
            ResourceKind::Playwright => Some(Icon::Globe),
            ResourceKind::Port(_) | ResourceKind::Redis | ResourceKind::Simulator => None,
        };
        badges.push(render_badge(
            icon,
            resource_badge_label(*resource).to_owned(),
            RESOURCE_BADGE_TEXT,
            RESOURCE_BADGE_BACKGROUND,
            Weight::Medium,
            appearance,
        ));
    }
    if finish_chime {
        badges.push(
            ConstrainedBox::new(
                Icon::VolumeOn
                    .to_warpui_icon(ThemeFill::Solid(FINISH_CHIME_BADGE_COLOR))
                    .finish(),
            )
            .with_width(12.)
            .with_height(12.)
            .finish(),
        );
    }
    if signals.shares_checkout {
        badges.push(render_shared_checkout_badge(tab_index, appearance));
    }
    if !signals.contested_files.is_empty() {
        let count = signals.contested_files.len();
        let label = if count == 1 {
            "1 arquivo".to_owned()
        } else {
            format!("{count} arquivos")
        };
        badges.push(render_badge(
            None,
            label,
            ColorU::white(),
            CONTESTED_FILE_COLOR,
            Weight::Bold,
            appearance,
        ));
    }
    if let Some((reason, waited)) = signals.waiting {
        badges.push(match reason {
            WaitReason::Blocked => render_badge(
                None,
                elapsed_label(waited),
                ColorU {
                    r: 31,
                    g: 26,
                    b: 0,
                    a: 255,
                },
                WAITING_COLOR,
                Weight::Semibold,
                appearance,
            ),
            WaitReason::Finished => render_badge(
                Some(Icon::Hand),
                elapsed_label(waited),
                FINISHED_HAND_COLOR,
                coloru_with_opacity(ColorU::black(), 55),
                Weight::Semibold,
                appearance,
            ),
        });
    }
    if badges.is_empty() {
        return None;
    }
    Some(
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(4.)
            .with_children(badges)
            .finish(),
    )
}

fn render_badge(
    icon: Option<Icon>,
    label: String,
    text_color: ColorU,
    background: ColorU,
    weight: Weight,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let mut row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(3.);
    if let Some(icon) = icon {
        row.add_child(
            ConstrainedBox::new(icon.to_warpui_icon(ThemeFill::Solid(text_color)).finish())
                .with_width(BADGE_FONT_SIZE + 1.)
                .with_height(BADGE_FONT_SIZE + 1.)
                .finish(),
        );
    }
    row.add_child(
        Text::new_inline(label, appearance.ui_font_family(), BADGE_FONT_SIZE)
            .with_color(text_color)
            .with_style(Properties::default().weight(weight))
            .finish(),
    );
    Container::new(row.finish())
        .with_background(ThemeFill::Solid(background))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
        .with_horizontal_padding(6.)
        .with_vertical_padding(2.)
        .finish()
}

/// Who holds a resource, as shown to the user.
pub struct ResourceHolder {
    /// The tab the holding process was started from, if it was started from one.
    pub tab_index: Option<usize>,
    /// The tab's title, or the process name when it was not started from a tab.
    pub name: String,
    pub color: Option<ColorU>,
}

/// One resource as listed in the title bar and the resources panel.
pub struct ResourceRow {
    pub kind: ResourceKind,
    /// `None` when the resource is free.
    pub holder: Option<ResourceHolder>,
    pub since: Option<DateTime<Local>>,
}

/// One pill per resource for the title bar, tinted with the holder's color when in use; clicking
/// toggles the resources panel.
pub fn render_title_bar_resources(
    rows: &[ResourceRow],
    appearance: &Appearance,
) -> Option<Box<dyn Element>> {
    if rows.is_empty() {
        return None;
    }
    let monospace_font_family = appearance.monospace_font_family();
    let mut pills = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.);
    for row in rows {
        let (holder_name, background, text_color) = match &row.holder {
            None => (
                "livre".to_owned(),
                ColorU {
                    r: 34,
                    g: 35,
                    b: 42,
                    a: 255,
                },
                PANEL_SUB_TEXT,
            ),
            Some(holder) => (
                holder.name.clone(),
                coloru_with_opacity(holder.color.unwrap_or(PANEL_SUB_TEXT), 22),
                lighten(holder.color.unwrap_or(PANEL_TEXT), 70),
            ),
        };
        let mut pill = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(5.);
        if row.kind == ResourceKind::Playwright {
            pill.add_child(
                ConstrainedBox::new(
                    Icon::Globe
                        .to_warpui_icon(ThemeFill::Solid(text_color))
                        .finish(),
                )
                .with_width(PILL_FONT_SIZE + 1.)
                .with_height(PILL_FONT_SIZE + 1.)
                .finish(),
            );
        }
        pill.add_child(
            Text::new_inline(
                format!("{} · {holder_name}", resource_label(row.kind)),
                monospace_font_family,
                PILL_FONT_SIZE,
            )
            .with_color(text_color)
            .finish(),
        );
        pills.add_child(
            Container::new(pill.finish())
                .with_background(ThemeFill::Solid(background))
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(999.)))
                .with_horizontal_padding(10.)
                .with_vertical_padding(4.)
                .finish(),
        );
    }
    let pills = pills.finish();
    let mouse_state = TITLE_BAR_RESOURCES_MOUSE_STATE.with(Clone::clone);
    Some(
        Hoverable::new(mouse_state, move |_| pills)
            .on_click(|ctx, _, _| {
                ctx.dispatch_typed_action(WorkspaceAction::ToggleSessionResourcesPanel);
            })
            .with_cursor(Cursor::PointingHand)
            .finish(),
    )
}

/// The resources panel: who holds each resource and since when, a note about the browser for the
/// active tab, and what each signal color means.
///
/// `browser_note` names the active tab and the tab holding the browser, when they differ.
pub fn render_resources_panel(
    rows: &[ResourceRow],
    idle_playwright_servers: usize,
    browser_note: Option<(String, String)>,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let font_family = appearance.ui_font_family();
    let monospace_font_family = appearance.monospace_font_family();
    let column_widths = [150., 210., 70.];

    let header_cell = |label: &str, width: f32| {
        ConstrainedBox::new(
            Text::new_inline(label.to_owned(), font_family, 11.)
                .with_color(PANEL_SUB_TEXT)
                .with_style(Properties::default().weight(Weight::Semibold))
                .finish(),
        )
        .with_width(width)
        .finish()
    };
    let mut table = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    table.add_child(
        Container::new(
            Flex::row()
                .with_child(header_cell("RECURSO", column_widths[0]))
                .with_child(header_cell("SESSÃO", column_widths[1]))
                .with_child(header_cell("DESDE", column_widths[2]))
                .finish(),
        )
        .with_horizontal_padding(10.)
        .with_vertical_padding(8.)
        .finish(),
    );
    for (row_index, row) in rows.iter().enumerate() {
        let session: Box<dyn Element> = match &row.holder {
            None => Text::new_inline("livre", font_family, 13.)
                .with_color(FREE_TEXT_COLOR)
                .finish(),
            Some(holder) => {
                let mut cell = Flex::row()
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(8.);
                if let Some(color) = holder.color {
                    cell.add_child(render_square(color, 10.));
                }
                cell.add_child(
                    Shrinkable::new(
                        1.,
                        Text::new_inline(holder.name.clone(), font_family, 13.)
                            .with_clip(ClipConfig::ellipsis())
                            .with_color(PANEL_TEXT)
                            .finish(),
                    )
                    .finish(),
                );
                cell.finish()
            }
        };
        let since = row
            .since
            .map(|since| since.format("%H:%M").to_string())
            .unwrap_or_else(|| "—".to_owned());
        let mut kind_cell = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(8.);
        if row.kind == ResourceKind::Playwright {
            kind_cell.add_child(
                ConstrainedBox::new(
                    Icon::Globe
                        .to_warpui_icon(ThemeFill::Solid(PANEL_TEXT))
                        .finish(),
                )
                .with_width(13.)
                .with_height(13.)
                .finish(),
            );
        }
        kind_cell.add_child(
            Text::new_inline(resource_panel_label(row.kind), monospace_font_family, 13.)
                .with_color(PANEL_TEXT)
                .finish(),
        );

        let mut cells = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                ConstrainedBox::new(kind_cell.finish())
                    .with_width(column_widths[0])
                    .finish(),
            )
            .with_child(
                ConstrainedBox::new(session)
                    .with_width(column_widths[1])
                    .finish(),
            )
            .with_child(
                ConstrainedBox::new(
                    Text::new_inline(since, font_family, 13.)
                        .with_color(PANEL_SUB_TEXT)
                        .finish(),
                )
                .with_width(column_widths[2])
                .finish(),
            );
        if let Some(tab_index) = row.holder.as_ref().and_then(|holder| holder.tab_index) {
            cells.add_child(
                Shrinkable::new(
                    1.,
                    Flex::row()
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_main_axis_alignment(MainAxisAlignment::End)
                        .with_child(render_go_to_tab_link(row_index, tab_index, appearance))
                        .finish(),
                )
                .finish(),
            );
        }
        let background = if row.kind == ResourceKind::Playwright && row.holder.is_some() {
            row.holder
                .as_ref()
                .and_then(|holder| holder.color)
                .map(|color| coloru_with_opacity(color, 10))
                .unwrap_or(ColorU::transparent_black())
        } else {
            ColorU::transparent_black()
        };
        table.add_child(
            Container::new(cells.finish())
                .with_background(ThemeFill::Solid(background))
                .with_border(
                    Border::new(1.)
                        .with_sides(true, false, false, false)
                        .with_border_fill(ThemeFill::Solid(PANEL_DIVIDER)),
                )
                .with_horizontal_padding(10.)
                .with_vertical_padding(11.)
                .finish(),
        );
    }

    let mut panel = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(14.);
    panel.add_child(
        Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(12.)
            .with_child(
                Text::new_inline("Recursos", font_family, 20.)
                    .with_color(PANEL_TEXT)
                    .with_style(Properties::default().weight(Weight::Bold))
                    .finish(),
            )
            .with_child(
                Text::new_inline("quem está usando agora", font_family, 13.)
                    .with_color(PANEL_SUB_TEXT)
                    .finish(),
            )
            .finish(),
    );
    panel.add_child(table.finish());
    if idle_playwright_servers > 0 {
        let note = if idle_playwright_servers == 1 {
            "Mais 1 sessão tem o Playwright ligado, sem navegador aberto.".to_owned()
        } else {
            format!(
                "Mais {idle_playwright_servers} sessões têm o Playwright ligado, sem navegador aberto."
            )
        };
        panel.add_child(
            Text::new(note, font_family, 13.)
                .with_color(PANEL_SUB_TEXT)
                .finish(),
        );
    }
    if let Some((active_tab, browser_holder)) = browser_note {
        panel.add_child(render_browser_note(active_tab, browser_holder, appearance));
    }
    panel.add_child(render_legend(appearance));

    ConstrainedBox::new(
        Container::new(panel.finish())
            .with_background(ThemeFill::Solid(PANEL_BACKGROUND))
            .with_border(Border::all(1.).with_border_fill(ThemeFill::Solid(PANEL_DIVIDER)))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(14.)))
            .with_uniform_padding(24.)
            .finish(),
    )
    .with_width(RESOURCES_PANEL_WIDTH)
    .finish()
}

fn render_go_to_tab_link(
    row_index: usize,
    tab_index: usize,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let font_family = appearance.ui_font_family();
    Hoverable::new(resource_link_mouse_state(row_index), move |state| {
        let color = if state.is_hovered() {
            lighten(LINK_COLOR, 60)
        } else {
            LINK_COLOR
        };
        Text::new_inline("ir para a aba", font_family, 13.)
            .with_color(color)
            .finish()
    })
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(WorkspaceAction::ActivateTab(tab_index));
        ctx.dispatch_typed_action(WorkspaceAction::ToggleSessionResourcesPanel);
    })
    .with_cursor(Cursor::PointingHand)
    .finish()
}

fn render_browser_note(
    active_tab: String,
    browser_holder: String,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let font_family = appearance.ui_font_family();
    let bold = Properties::default().weight(Weight::Bold);
    let text = |label: String, properties: Properties| {
        Text::new_inline(label, font_family, 13.)
            .with_color(WAITING_TEXT_COLOR)
            .with_style(properties)
            .finish()
    };
    let row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_child(
            Container::new(
                ConstrainedBox::new(
                    Icon::Globe
                        .to_warpui_icon(ThemeFill::Solid(WAITING_TEXT_COLOR))
                        .finish(),
                )
                .with_width(15.)
                .with_height(15.)
                .finish(),
            )
            .with_margin_right(10.)
            .finish(),
        )
        .with_child(text("Na aba ".to_owned(), Properties::default()))
        .with_child(text(active_tab, bold))
        .with_child(text(
            ": o navegador do Playwright está com ".to_owned(),
            Properties::default(),
        ))
        .with_child(text(browser_holder, bold))
        .with_child(text(".".to_owned(), Properties::default()))
        .finish();
    Container::new(row)
        .with_background(ThemeFill::Solid(ColorU {
            r: 31,
            g: 26,
            b: 11,
            a: 255,
        }))
        .with_border(Border::all(1.).with_border_fill(ThemeFill::Solid(ColorU {
            r: 161,
            g: 98,
            b: 7,
            a: 255,
        })))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(10.)))
        .with_horizontal_padding(14.)
        .with_vertical_padding(12.)
        .finish()
}

fn render_legend(appearance: &Appearance) -> Box<dyn Element> {
    let font_family = appearance.ui_font_family();
    let swatch = |background: ColorU, border: Option<ColorU>| {
        let mut container = Container::new(Empty::new().finish())
            .with_background(ThemeFill::Solid(background))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)));
        if let Some(border) = border {
            container =
                container.with_border(Border::all(2.).with_border_fill(ThemeFill::Solid(border)));
        }
        ConstrainedBox::new(container.finish())
            .with_width(40.)
            .with_height(26.)
            .finish()
    };
    let purple = ColorU {
        r: 139,
        g: 92,
        b: 246,
        a: 255,
    };
    let entries: Vec<(Box<dyn Element>, &str)> = vec![
        (
            swatch(coloru_with_opacity(purple, 32), None),
            "Cor de fundo: o grupo / repositório",
        ),
        (
            ConstrainedBox::new(
                Container::new(
                    Icon::Link
                        .to_warpui_icon(ThemeFill::Solid(SHARED_CHECKOUT_COLOR))
                        .finish(),
                )
                .with_uniform_padding(4.)
                .finish(),
            )
            .with_width(40.)
            .with_height(26.)
            .finish(),
            "Corrente laranja: pasta git dividida com outra aba",
        ),
        (
            swatch(CONTESTED_FILE_COLOR, None),
            "Selo vermelho: outra sessão mexeu no mesmo arquivo",
        ),
        (
            swatch(ColorU::transparent_black(), Some(pulsing(WAITING_COLOR))),
            "Pulsando amarelo: esperando sua resposta",
        ),
        (
            ConstrainedBox::new(
                Container::new(
                    Icon::Hand
                        .to_warpui_icon(ThemeFill::Solid(FINISHED_HAND_COLOR))
                        .finish(),
                )
                .with_uniform_padding(4.)
                .finish(),
            )
            .with_width(40.)
            .with_height(26.)
            .finish(),
            "Mão dourada: terminou, sua vez",
        ),
        (
            ConstrainedBox::new(
                Container::new(
                    Icon::VolumeOn
                        .to_warpui_icon(ThemeFill::Solid(FINISH_CHIME_BADGE_COLOR))
                        .finish(),
                )
                .with_uniform_padding(4.)
                .finish(),
            )
            .with_width(40.)
            .with_height(26.)
            .finish(),
            "Alto-falante: toca um som quando termina",
        ),
        (
            ConstrainedBox::new(
                Container::new(render_status_dot(SERVER_UP_COLOR, None))
                    .with_padding_left(16.)
                    .with_padding_top(9.)
                    .finish(),
            )
            .with_width(40.)
            .with_height(26.)
            .finish(),
            "Ponto verde: servidor no ar · vermelho: caiu",
        ),
        (
            swatch(RESOURCE_BADGE_BACKGROUND, None),
            "Selo de navegador: esta sessão está com o Playwright",
        ),
    ];
    let mut column = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_spacing(10.);
    column.add_child(
        Text::new_inline("O que cada cor quer dizer", font_family, 15.)
            .with_color(PANEL_TEXT)
            .with_style(Properties::default().weight(Weight::Bold))
            .finish(),
    );
    for (swatch, label) in entries {
        column.add_child(
            Flex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(12.)
                .with_child(swatch)
                .with_child(
                    Text::new_inline(label.to_owned(), font_family, 13.)
                        .with_color(PANEL_TEXT)
                        .finish(),
                )
                .finish(),
        );
    }
    Container::new(column.finish())
        .with_border(
            Border::new(1.)
                .with_sides(true, false, false, false)
                .with_border_fill(ThemeFill::Solid(PANEL_DIVIDER)),
        )
        .with_padding_top(14.)
        .finish()
}

fn render_square(color: ColorU, size: f32) -> Box<dyn Element> {
    ConstrainedBox::new(
        Container::new(Empty::new().finish())
            .with_background(ThemeFill::Solid(color))
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(3.)))
            .finish(),
    )
    .with_width(size)
    .with_height(size)
    .finish()
}

/// `color` mixed toward white by `amount` out of 255.
pub fn lighten(color: ColorU, amount: u8) -> ColorU {
    let mix = |channel: u8| {
        let channel = u16::from(channel);
        (channel + (255 - channel) * u16::from(amount) / 255) as u8
    };
    ColorU {
        r: mix(color.r),
        g: mix(color.g),
        b: mix(color.b),
        a: color.a,
    }
}

fn render_shared_checkout_badge(tab_index: usize, appearance: &Appearance) -> Box<dyn Element> {
    let ui_builder = appearance.ui_builder().clone();
    Hoverable::new(shared_checkout_mouse_state(tab_index), move |state| {
        let icon = ConstrainedBox::new(
            Icon::Link
                .to_warpui_icon(ThemeFill::Solid(SHARED_CHECKOUT_COLOR))
                .finish(),
        )
        .with_width(14.)
        .with_height(14.)
        .finish();
        if !state.is_hovered() {
            return icon;
        }
        let tooltip = ui_builder
            .tool_tip(SHARED_CHECKOUT_TOOLTIP.to_owned())
            .build()
            .finish();
        let mut stack = Stack::new().with_child(icon);
        stack.add_positioned_overlay_child(
            tooltip,
            OffsetPositioning::offset_from_parent(
                vec2f(0., -4.),
                ParentOffsetBounds::WindowByPosition,
                ParentAnchor::TopMiddle,
                ChildAnchor::BottomMiddle,
            ),
        );
        stack.finish()
    })
    .finish()
}

/// The port a tab's server listens on, opening it in the browser while the server is up.
fn render_port_badge(
    server: ServerBadge,
    tab_index: usize,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let font_family = appearance.ui_font_family();
    if server.is_down {
        let label = format!(":{}", server.port);
        let strikethrough = Highlight::new().with_text_style(
            TextStyle::new()
                .with_show_strikethrough(true)
                .with_foreground_color(PORT_BADGE_DOWN_TEXT),
        );
        let indices = (0..label.chars().count()).collect();
        return Container::new(
            Text::new_inline(label, font_family, 10.)
                .with_color(PORT_BADGE_DOWN_TEXT)
                .with_style(Properties::default().weight(Weight::Semibold))
                .with_single_highlight(strikethrough, indices)
                .finish(),
        )
        .with_background(ThemeFill::Solid(PORT_BADGE_DOWN_BACKGROUND))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
        .with_horizontal_padding(5.)
        .with_vertical_padding(1.)
        .finish();
    }
    let url = format!("http://localhost:{}", server.port);
    Hoverable::new(port_badge_mouse_state(tab_index), move |state| {
        let background = if state.is_hovered() {
            lighten(PORT_BADGE_BACKGROUND, 30)
        } else {
            PORT_BADGE_BACKGROUND
        };
        Container::new(
            Text::new_inline(format!(":{} ↗", server.port), font_family, 10.)
                .with_color(PORT_BADGE_TEXT)
                .with_style(Properties::default().weight(Weight::Semibold))
                .finish(),
        )
        .with_background(ThemeFill::Solid(background))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
        .with_horizontal_padding(5.)
        .with_vertical_padding(1.)
        .finish()
    })
    .with_cursor(Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(WorkspaceAction::OpenLink(url.clone()));
    })
    .finish()
}

fn port_badge_mouse_state(tab_index: usize) -> MouseStateHandle {
    PORT_BADGE_MOUSE_STATES.with(|states| {
        let mut states = states.borrow_mut();
        while states.len() <= tab_index {
            states.push(MouseStateHandle::default());
        }
        states[tab_index].clone()
    })
}

/// The line under a server tab's name: a dot for whether the server is up, and its last request.
pub fn render_server_line(track: &ServerTrack, appearance: &Appearance) -> Box<dyn Element> {
    let now = Instant::now();
    let font_family = appearance.ui_font_family();
    let text = move |label: String, color: ColorU| {
        Text::new_inline(label, font_family, SERVER_LINE_FONT_SIZE)
            .with_clip(ClipConfig::ellipsis())
            .with_color(color)
            .finish()
    };
    let ring = track.blink_progress(now);
    let mut row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.);
    match track.line(now, Local::now().naive_local()) {
        ServerLine::Down(down_for) => {
            row.add_child(render_status_dot(SERVER_DOWN_COLOR, None));
            row.add_child(
                Shrinkable::new(
                    1.,
                    text(
                        format!("fora do ar {}", time_ago(down_for)),
                        SERVER_DOWN_TEXT_COLOR,
                    ),
                )
                .finish(),
            );
        }
        ServerLine::Idle { last_request } => {
            row.add_child(render_status_dot(SERVER_UP_COLOR, ring));
            let label = match last_request {
                Some(age) => format!("no ar · última requisição {}", time_ago(age)),
                None => "no ar".to_owned(),
            };
            row.add_child(Shrinkable::new(1., text(label, SERVER_LINE_TEXT_COLOR)).finish());
        }
        ServerLine::Request {
            request,
            age,
            highlight_status,
        } => {
            row.add_child(render_status_dot(SERVER_UP_COLOR, ring));
            row.add_child(
                Shrinkable::new(
                    1.,
                    text(
                        format!("{} {}", request.method, request.path),
                        SERVER_REQUEST_TEXT_COLOR,
                    ),
                )
                .finish(),
            );
            if highlight_status {
                let (background, foreground) = if request.status >= 500 {
                    (SERVER_ERROR_COLOR, ColorU::white())
                } else {
                    (SERVER_WARNING_COLOR, SERVER_WARNING_TEXT_COLOR)
                };
                row.add_child(
                    Container::new(
                        Text::new_inline(
                            request.status.to_string(),
                            font_family,
                            SERVER_LINE_FONT_SIZE,
                        )
                        .with_color(foreground)
                        .with_style(Properties::default().weight(Weight::Bold))
                        .finish(),
                    )
                    .with_background(ThemeFill::Solid(background))
                    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(3.)))
                    .with_horizontal_padding(4.)
                    .finish(),
                );
                row.add_child(text(time_ago(age), SERVER_LINE_TEXT_COLOR));
            } else {
                row.add_child(text(
                    format!("· {} · {}", request.status, time_ago(age)),
                    SERVER_LINE_TEXT_COLOR,
                ));
            }
        }
    }
    row.finish()
}

/// A server's status dot, with a fading ring spreading around it at `ring` progress.
fn render_status_dot(color: ColorU, ring: Option<f32>) -> Box<dyn Element> {
    let Some(progress) = ring else {
        return render_circle(color, SERVER_DOT_SIZE);
    };
    let centered = || {
        OffsetPositioning::offset_from_parent(
            vec2f(0., 0.),
            ParentOffsetBounds::Unbounded,
            ParentAnchor::Center,
            ChildAnchor::Center,
        )
    };
    let halo = render_circle(
        ColorU {
            a: (180. * (1. - progress)) as u8,
            ..color
        },
        SERVER_DOT_SIZE + 2. * SERVER_RING_SPREAD * progress,
    );
    let mut stack = Stack::new().with_child(render_circle(color, SERVER_DOT_SIZE));
    stack.add_positioned_overlay_child(halo, centered());
    stack.add_positioned_overlay_child(render_circle(color, SERVER_DOT_SIZE), centered());
    stack.finish()
}

/// "agora" under a minute, otherwise "há 5 min".
fn time_ago(elapsed: Duration) -> String {
    match elapsed_label(elapsed) {
        label if elapsed.as_secs() < 60 => label,
        label => format!("há {label}"),
    }
}

fn shared_checkout_mouse_state(tab_index: usize) -> MouseStateHandle {
    SHARED_CHECKOUT_MOUSE_STATES.with(|states| {
        let mut states = states.borrow_mut();
        while states.len() <= tab_index {
            states.push(MouseStateHandle::default());
        }
        states[tab_index].clone()
    })
}

fn resource_link_mouse_state(row_index: usize) -> MouseStateHandle {
    RESOURCE_LINK_MOUSE_STATES.with(|states| {
        let mut states = states.borrow_mut();
        while states.len() <= row_index {
            states.push(MouseStateHandle::default());
        }
        states[row_index].clone()
    })
}

fn resource_label(kind: ResourceKind) -> String {
    match kind {
        ResourceKind::Port(port) => format!(":{port}"),
        ResourceKind::Redis => "Redis".to_owned(),
        ResourceKind::Simulator => "Simulador".to_owned(),
        ResourceKind::Playwright => "Playwright".to_owned(),
    }
}

fn resource_badge_label(kind: ResourceKind) -> String {
    match kind {
        ResourceKind::Playwright => "navegador".to_owned(),
        ResourceKind::Port(_) | ResourceKind::Redis | ResourceKind::Simulator => {
            resource_label(kind)
        }
    }
}

fn resource_panel_label(kind: ResourceKind) -> String {
    match kind {
        ResourceKind::Port(port) => format!("Porta {port}"),
        ResourceKind::Simulator => "Simulador iOS".to_owned(),
        ResourceKind::Redis | ResourceKind::Playwright => resource_label(kind),
    }
}

/// How long ago, in a few characters: "agora", "8 min", "1h15".
pub fn elapsed_label(waited: Duration) -> String {
    let minutes = waited.as_secs() / 60;
    if minutes == 0 {
        "agora".to_owned()
    } else if minutes < 60 {
        format!("{minutes} min")
    } else {
        format!("{}h{:02}", minutes / 60, minutes % 60)
    }
}

/// "trabalhando · 2 min 14 s": seconds included, since a turn often lasts under a minute.
pub fn working_label(elapsed: Duration) -> String {
    format!("trabalhando · {}", clock_label(elapsed))
}

/// "45 s", "2 min 14 s", "1 h 05 min".
pub fn clock_label(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    if secs < 60 {
        format!("{secs} s")
    } else if secs < 60 * 60 {
        format!("{} min {} s", secs / 60, secs % 60)
    } else {
        format!("{} h {:02} min", secs / 3600, secs / 60 % 60)
    }
}

pub fn idle_label(idle: Duration) -> String {
    if idle.as_secs() < 60 {
        "parado agora".to_owned()
    } else {
        format!("parado há {}", elapsed_label(idle))
    }
}

/// Claude's own orange, for the spinning star.
pub const WORKING_STAR_COLOR: ColorU = ColorU {
    r: 217,
    g: 119,
    b: 87,
    a: 255,
};
pub const WORKING_TEXT_COLOR: ColorU = ColorU {
    r: 243,
    g: 181,
    b: 156,
    a: 255,
};

/// The frames of the star Claude Code spins in its own status line.
const WORKING_STAR_FRAMES: [&str; 10] = ["·", "✢", "✳", "✶", "✻", "✽", "✻", "✶", "✳", "✢"];
const WORKING_STAR_FRAME_DURATION: Duration = Duration::from_millis(120);

/// Shared start time, so every working tab's star turns in step.
static WORKING_STAR_ANCHOR: LazyLock<Instant> = LazyLock::new(Instant::now);

pub fn working_star_frame() -> &'static str {
    let frame = WORKING_STAR_ANCHOR.elapsed().as_millis() / WORKING_STAR_FRAME_DURATION.as_millis();
    WORKING_STAR_FRAMES[frame as usize % WORKING_STAR_FRAMES.len()]
}

#[cfg(test)]
#[path = "ui_tests.rs"]
mod tests;
