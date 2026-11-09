//! A strip above the active tab's content showing the state of the PR package, as written by the
//! `pacote-pr` Claude Code mod to `~/.claude/pacote-pr/estado.json`.
//!
//! The strip only informs: it takes no input, makes no sound and blocks nothing. It stays absent
//! unless the file is fresh, the mod marked itself active and it has something to say.

use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, Utc};
use pathfinder_color::ColorU;
use serde::Deserialize;
use warp_core::ui::color::coloru_with_opacity;
use warp_core::ui::theme::Fill as ThemeFill;
use warpui::ViewContext;
use warpui::elements::{
    ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, Element, Flex, MainAxisSize,
    ParentElement, Radius, Shrinkable, Text,
};
use warpui::fonts::{Properties, Weight};
use warpui::text_layout::ClipConfig;

use super::Workspace;
use crate::appearance::Appearance;
use crate::ui_components::icons::Icon;
use crate::workspace::session_signals::ui::WAITING_COLOR;

const FORMAT_VERSION: u32 = 1;
/// Older data describes a package that may no longer exist, so it is not shown.
const MAX_AGE: Duration = Duration::from_secs(3 * 60);
const POLL_INTERVAL: Duration = Duration::from_secs(3);
/// The mod rewrites the file every minute, so a half-written read is gone almost at once.
const RETRY_DELAY: Duration = Duration::from_millis(300);
const MAX_RETRIES: u32 = 3;
const MAX_LINES: usize = 2;
const FONT_SIZE: f32 = 12.;
const ICON_SIZE: f32 = 12.;

/// Last state that parsed successfully, shared by every window.
static LATEST: Mutex<Option<PacoteState>> = Mutex::new(None);

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PacoteState {
    #[serde(default)]
    versao: Option<u32>,
    ativo: bool,
    atualizado_em: DateTime<Utc>,
    faixa: Option<Banner>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub(super) struct Banner {
    linhas: Vec<String>,
    nivel: Level,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Level {
    Atencao,
    #[serde(other)]
    Info,
}

/// What one read of the file produced.
#[derive(Debug, PartialEq)]
pub(super) enum Reading {
    Parsed(PacoteState),
    Missing,
    Invalid,
}

pub(super) fn read_state(contents: std::io::Result<String>) -> Reading {
    match contents {
        Ok(contents) => match serde_json::from_str(&contents) {
            Ok(state) => Reading::Parsed(state),
            Err(_) => Reading::Invalid,
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Reading::Missing,
        Err(_) => Reading::Invalid,
    }
}

/// The banner to show for `state` at `now`, or `None` when the strip should stay absent.
pub(super) fn visible_banner(
    state: &PacoteState,
    now: DateTime<Utc>,
) -> Option<(Vec<&str>, Level)> {
    if !state.ativo
        || state
            .versao
            .is_some_and(|version| version != FORMAT_VERSION)
    {
        return None;
    }
    let age = now
        .signed_duration_since(state.atualizado_em)
        .to_std()
        .unwrap_or_default();
    if age > MAX_AGE {
        return None;
    }
    let banner = state.faixa.as_ref()?;
    let lines: Vec<&str> = banner
        .linhas
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .take(MAX_LINES)
        .collect();
    (!lines.is_empty()).then_some((lines, banner.nivel))
}

fn set_latest(state: Option<PacoteState>) {
    if let Ok(mut latest) = LATEST.lock() {
        *latest = state;
    }
}

/// Starts re-reading the state file every few seconds for as long as the workspace lives.
pub(super) fn start_polling(ctx: &mut ViewContext<Workspace>) {
    if cfg!(test) {
        return;
    }
    schedule_read(Duration::ZERO, 0, ctx);
}

impl Workspace {
    /// The strip for the current state, or `None` to stay silent.
    pub(super) fn render_pacote_pr_band(
        &self,
        appearance: &Appearance,
    ) -> Option<Box<dyn Element>> {
        let state = LATEST.lock().ok()?.clone()?;
        let (lines, level) = visible_banner(&state, Utc::now())?;
        Some(render_band(&lines, level, appearance))
    }
}

#[cfg(not(target_family = "wasm"))]
fn schedule_read(delay: Duration, attempt: u32, ctx: &mut ViewContext<Workspace>) {
    use warpui::r#async::Timer;

    let path = dirs::home_dir().map(|home| home.join(".claude/pacote-pr/estado.json"));
    ctx.spawn(
        async move {
            Timer::after(delay).await;
            match path {
                Some(path) => read_state(std::fs::read_to_string(path)),
                None => Reading::Missing,
            }
        },
        move |_, reading, ctx| {
            let mut next_delay = POLL_INTERVAL;
            let mut next_attempt = 0;
            match reading {
                Reading::Parsed(state) => set_latest(Some(state)),
                Reading::Missing => set_latest(None),
                // Keep the last good state: it expires on its own if the file stays broken.
                Reading::Invalid if attempt < MAX_RETRIES => {
                    next_delay = RETRY_DELAY;
                    next_attempt = attempt + 1;
                }
                Reading::Invalid => {}
            }
            ctx.notify();
            schedule_read(next_delay, next_attempt, ctx);
        },
    );
}

#[cfg(target_family = "wasm")]
fn schedule_read(_delay: Duration, _attempt: u32, _ctx: &mut ViewContext<Workspace>) {}

fn render_band(lines: &[&str], level: Level, appearance: &Appearance) -> Box<dyn Element> {
    let theme = appearance.theme();
    let quiet_text: ColorU = theme.sub_text_color(theme.background()).into();
    let (icon, text_color, background, weight) = match level {
        Level::Atencao => (
            Icon::AlertTriangle,
            ColorU::black(),
            WAITING_COLOR,
            Weight::Semibold,
        ),
        Level::Info => (
            Icon::Info,
            quiet_text,
            coloru_with_opacity(quiet_text, 12),
            Weight::Normal,
        ),
    };
    let mut text = Flex::column().with_main_axis_size(MainAxisSize::Min);
    for line in lines {
        text.add_child(
            Text::new_inline((*line).to_owned(), appearance.ui_font_family(), FONT_SIZE)
                .with_clip(ClipConfig::ellipsis())
                .with_color(text_color)
                .with_style(Properties::default().weight(weight))
                .finish(),
        );
    }
    let row = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(8.)
        .with_child(
            ConstrainedBox::new(icon.to_warpui_icon(ThemeFill::Solid(text_color)).finish())
                .with_width(ICON_SIZE)
                .with_height(ICON_SIZE)
                .finish(),
        )
        .with_child(Shrinkable::new(1., text.finish()).finish())
        .finish();
    Container::new(row)
        .with_background(ThemeFill::Solid(background))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.)))
        .with_horizontal_padding(12.)
        .with_vertical_padding(4.)
        .with_margin_left(12.)
        .with_margin_right(12.)
        .with_margin_top(6.)
        .finish()
}

#[cfg(test)]
#[path = "pacote_pr_band_tests.rs"]
mod tests;
