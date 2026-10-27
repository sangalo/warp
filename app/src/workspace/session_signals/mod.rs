//! Signals that keep parallel agent sessions from stepping on each other.
//!
//! Running many agent sessions at once fails in a few recurring ways: a message is typed into the
//! wrong session, two sessions share one checkout and one commits the other's work, two sessions
//! edit the same file, or a session sits waiting for an answer nobody noticed. This model gathers
//! what is needed to show each of those on the tabs themselves, with colors that keep one meaning:
//! an orange link icon for a shared checkout, red for a file edited by two sessions, yellow for a
//! session waiting on the user.

pub mod scanner;
pub mod server;
pub mod sounds;
pub mod subagents;
pub mod subagents_ui;
pub mod ui;
pub mod working_sweep;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use blocking::unblock;
use chrono::{DateTime, Local};
use instant::Instant;
use warpui::r#async::Timer;
use warpui::{AppContext, Entity, EntityId, ModelContext, SingletonEntity, ViewHandle};

use self::scanner::{ResourceKind, ResourceState, ScanResult};
use self::server::{ServerPaneObservation, ServerTrack};
use self::subagents::{Subagent, SubagentSource};
use crate::ai::agent_management::AgentNotificationsModel;
use crate::code::buffer_location::LocalOrRemotePath;
use crate::pane_group::PaneGroup;
use crate::terminal::cli_agent_sessions::{
    CLIAgentSessionStatus, CLIAgentSessionsModel, CLIAgentSessionsModelEvent,
};

const SCAN_INTERVAL: Duration = Duration::from_secs(5);
/// How often tabs waiting on the user are redrawn while their border pulses.
const PULSE_FRAME_INTERVAL: Duration = Duration::from_millis(120);
/// How often server tabs are checked for new requests.
const SERVER_TICK_INTERVAL: Duration = Duration::from_secs(1);

/// Directory where the Claude Code hook records which files each session edits.
fn edit_ledger_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".claude").join("warp-edits"))
}

/// Directory where Claude Code keeps each project's session transcripts.
fn claude_projects_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".claude").join("projects"))
}

/// Why a session is waiting on the user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitReason {
    /// The agent asked for permission or a question to be answered.
    Blocked,
    /// The agent finished its turn and waits for the next instruction.
    Finished,
}

/// What a tab's agent is doing, for the line under the tab's name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentActivity {
    /// Running a turn, for this long.
    Working(Duration),
    /// Done with its turn, with the result already seen, for this long.
    Idle(Duration),
}

/// What a tab should show about its sessions.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TabSignals {
    /// Another tab runs an agent in the same checkout.
    pub shares_checkout: bool,
    /// Files this tab's agent edited that another open agent session also edited.
    pub contested_files: Vec<PathBuf>,
    pub waiting: Option<(WaitReason, Duration)>,
    /// Shared resources held by processes started from this tab.
    pub held_resources: Vec<ResourceKind>,
    /// Subagents launched by this tab's agent sessions, running ones first.
    pub subagents: Vec<Subagent>,
    pub server: Option<ServerBadge>,
}

/// The port of a dev server running in a tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServerBadge {
    pub port: u16,
    pub is_down: bool,
}

/// One agent session in a tab, as seen when computing [`TabSignals`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TabSessionFacts {
    pub session_uuids: Vec<String>,
    /// Root of the checkout the tab's agent runs in.
    pub agent_checkout: Option<PathBuf>,
    pub agent_session_id: Option<String>,
    /// Every agent session in the tab, when it is split into several.
    pub agent_session_ids: Vec<String>,
    pub waiting: Option<(WaitReason, Duration)>,
    pub server: Option<ServerBadge>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionSignalsEvent {
    Changed,
    /// Time to check server tabs for new requests, with [`SessionSignalsModel::observe_servers`].
    ServerTick,
}

pub struct SessionSignalsModel {
    /// When each terminal's agent session entered its current status.
    status_since: HashMap<EntityId, Instant>,
    /// Terminals whose agent has started a turn since the session began. A session is registered
    /// as in progress before any prompt, including when Warp resumes it on launch.
    turn_started: HashSet<EntityId>,
    scan: ScanResult,
    /// When each resource was taken by its current holder.
    resource_since: HashMap<ResourceKind, (ResourceState, DateTime<Local>)>,
    is_pulse_scheduled: bool,
    /// Dev servers listening from a pane, keyed by the pane's session UUID.
    servers: HashMap<String, ServerTrack>,
    is_server_tick_scheduled: bool,
}

impl Entity for SessionSignalsModel {
    type Event = SessionSignalsEvent;
}

impl SingletonEntity for SessionSignalsModel {}

impl SessionSignalsModel {
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        ctx.subscribe_to_model(&CLIAgentSessionsModel::handle(ctx), |me, _, event, ctx| {
            match event {
                CLIAgentSessionsModelEvent::Started {
                    terminal_view_id, ..
                } => {
                    me.status_since.insert(*terminal_view_id, Instant::now());
                    me.turn_started.remove(terminal_view_id);
                }
                CLIAgentSessionsModelEvent::StatusChanged {
                    terminal_view_id,
                    status,
                    ..
                } => {
                    me.status_since.insert(*terminal_view_id, Instant::now());
                    if matches!(status, CLIAgentSessionStatus::InProgress) {
                        me.turn_started.insert(*terminal_view_id);
                    }
                    sounds::play_for_status(status, ctx);
                }
                CLIAgentSessionsModelEvent::Ended {
                    terminal_view_id, ..
                } => {
                    me.status_since.remove(terminal_view_id);
                    me.turn_started.remove(terminal_view_id);
                }
                CLIAgentSessionsModelEvent::InputSessionChanged { .. }
                | CLIAgentSessionsModelEvent::SessionUpdated { .. } => return,
            }
            ctx.emit(SessionSignalsEvent::Changed);
            me.schedule_pulse(ctx);
        });
        let model = Self {
            status_since: HashMap::new(),
            turn_started: HashSet::new(),
            scan: ScanResult::default(),
            resource_since: HashMap::new(),
            is_pulse_scheduled: false,
            servers: HashMap::new(),
            is_server_tick_scheduled: false,
        };
        model.schedule_scan(Duration::ZERO, ctx);
        model
    }

    fn schedule_scan(&self, delay: Duration, ctx: &mut ModelContext<Self>) {
        let _ = ctx.spawn(Timer::after(delay), |me, _, ctx| me.scan(ctx));
    }

    fn scan(&mut self, ctx: &mut ModelContext<Self>) {
        let Some(ledger_dir) = edit_ledger_dir() else {
            return;
        };
        let subagent_sources = self.subagent_sources(ctx);
        let projects_dir = claude_projects_dir();
        let _ = ctx.spawn(
            unblock(move || {
                let mut scan = scanner::scan(&ledger_dir);
                if let Some(projects_dir) = projects_dir {
                    scan.subagents =
                        subagents::scan(&subagent_sources, &projects_dir, SystemTime::now());
                }
                scan
            }),
            |me, scan, ctx| {
                me.record_resource_holders(&scan);
                me.track_server_ports(&scan, Instant::now());
                me.scan = scan;
                // Emitted even when nothing changed so displayed wait times keep counting up.
                ctx.emit(SessionSignalsEvent::Changed);
                me.schedule_scan(SCAN_INTERVAL, ctx);
                me.schedule_pulse(ctx);
                me.schedule_server_tick(ctx);
            },
        );
    }

    /// Where to look for the subagents of every open local agent session.
    fn subagent_sources(&self, app: &AppContext) -> Vec<SubagentSource> {
        let sessions = CLIAgentSessionsModel::as_ref(app);
        self.status_since
            .keys()
            .filter_map(|terminal_view_id| {
                let session = sessions.session(*terminal_view_id)?;
                if session.is_remote() {
                    return None;
                }
                let context = &session.session_context;
                Some(SubagentSource {
                    session_id: context.session_id.clone()?,
                    transcript_path: context.transcript_path.as_ref().map(PathBuf::from),
                })
            })
            .collect()
    }

    /// Redraws tabs for as long as any waits on the user, has a subagent running or a server dot
    /// ringing, so their border can pulse and their subagent counter and dot animate.
    fn schedule_pulse(&mut self, ctx: &mut ModelContext<Self>) {
        if self.is_pulse_scheduled
            || !(self.any_session_blocked(ctx)
                || self.any_subagent_running()
                || self.any_server_ringing())
        {
            return;
        }
        self.is_pulse_scheduled = true;
        let _ = ctx.spawn(Timer::after(PULSE_FRAME_INTERVAL), |me, _, ctx| {
            me.is_pulse_scheduled = false;
            ctx.emit(SessionSignalsEvent::Changed);
            me.schedule_pulse(ctx);
        });
    }

    /// Whether the agent in `terminal_view_id` has run a turn since its session began.
    pub fn has_started_turn(&self, terminal_view_id: EntityId) -> bool {
        self.turn_started.contains(&terminal_view_id)
    }

    fn any_session_blocked(&self, app: &AppContext) -> bool {
        let sessions = CLIAgentSessionsModel::as_ref(app);
        self.status_since.keys().any(|terminal_view_id| {
            sessions.session(*terminal_view_id).is_some_and(|session| {
                matches!(session.status, CLIAgentSessionStatus::Blocked { .. })
            })
        })
    }

    fn schedule_server_tick(&mut self, ctx: &mut ModelContext<Self>) {
        if self.is_server_tick_scheduled || self.servers.is_empty() {
            return;
        }
        self.is_server_tick_scheduled = true;
        let _ = ctx.spawn(Timer::after(SERVER_TICK_INTERVAL), |me, _, ctx| {
            me.is_server_tick_scheduled = false;
            ctx.emit(SessionSignalsEvent::ServerTick);
            me.schedule_server_tick(ctx);
        });
    }

    /// Starts tracking panes that hold a watched port, and marks as down the tracked ones whose
    /// port went free.
    fn track_server_ports(&mut self, scan: &ScanResult, now: Instant) {
        let holders: HashMap<&String, u16> = scan
            .resources
            .iter()
            .filter_map(|(kind, state)| match (kind, state) {
                (
                    ResourceKind::Port(port),
                    ResourceState::InUse {
                        pane: Some(pane), ..
                    },
                ) => Some((pane, *port)),
                (ResourceKind::Port(_), ResourceState::InUse { pane: None, .. })
                | (_, ResourceState::Free)
                | (ResourceKind::Redis | ResourceKind::Simulator | ResourceKind::Playwright, _) => {
                    None
                }
            })
            .collect();
        for (pane, port) in &holders {
            let track = self
                .servers
                .entry((*pane).clone())
                .or_insert_with(|| ServerTrack::new(*port, now));
            track.port = *port;
            track.down_since = None;
        }
        for (pane, track) in &mut self.servers {
            if !holders.contains_key(pane) {
                track.down_since.get_or_insert(now);
            }
        }
        self.servers.retain(|_, track| !track.is_abandoned(now));
    }

    /// Panes tracked as running a server, to observe on [`SessionSignalsEvent::ServerTick`].
    pub fn server_panes(&self) -> impl Iterator<Item = &String> {
        self.servers.keys()
    }

    /// The server running in `pane`, unless the pane runs a coding agent, whose own line wins.
    pub fn server(&self, pane: &str) -> Option<&ServerTrack> {
        self.servers.get(pane).filter(|track| !track.is_agent_pane)
    }

    /// Records what each server pane looks like now, dropping the ones that stopped running a
    /// server.
    pub fn observe_servers(
        &mut self,
        observations: Vec<ServerPaneObservation>,
        ctx: &mut ModelContext<Self>,
    ) {
        let now = Instant::now();
        let before = self.servers.clone();
        for observation in observations {
            let pane = observation.pane.clone();
            if let Some(track) = self.servers.get_mut(&pane)
                && !track.observe(observation, now)
            {
                self.servers.remove(&pane);
            }
        }
        if self.servers != before {
            ctx.emit(SessionSignalsEvent::Changed);
            self.schedule_pulse(ctx);
        }
    }

    fn any_server_ringing(&self) -> bool {
        let now = Instant::now();
        self.servers
            .values()
            .any(|track| track.blink_progress(now).is_some())
    }

    fn any_subagent_running(&self) -> bool {
        self.scan
            .subagents
            .values()
            .flatten()
            .any(Subagent::is_running)
    }

    fn record_resource_holders(&mut self, scan: &ScanResult) {
        let now = Local::now();
        self.resource_since = scan
            .resources
            .iter()
            .filter(|(_, state)| *state != ResourceState::Free)
            .map(|(kind, state)| {
                let since = match self.resource_since.get(kind) {
                    Some((previous, since)) if previous == state => *since,
                    Some(_) | None => now,
                };
                (*kind, (state.clone(), since))
            })
            .collect();
    }

    pub fn resources(&self) -> &[(ResourceKind, ResourceState)] {
        &self.scan.resources
    }

    /// When `kind` was taken by its current holder, if it is in use. Resources already in use when
    /// Warp started count from the first scan.
    pub fn resource_since(&self, kind: ResourceKind) -> Option<DateTime<Local>> {
        self.resource_since.get(&kind).map(|(_, since)| *since)
    }

    pub fn idle_playwright_servers(&self) -> usize {
        self.scan.idle_playwright_servers
    }

    /// How long the agent in `terminal_view_id` has been waiting on the user, if it is.
    pub fn waiting(
        &self,
        terminal_view_id: EntityId,
        app: &AppContext,
    ) -> Option<(WaitReason, Duration)> {
        let session = CLIAgentSessionsModel::as_ref(app).session(terminal_view_id)?;
        let reason = match session.status {
            CLIAgentSessionStatus::Blocked { .. } => WaitReason::Blocked,
            // A finished turn stays finished for hours, so it only counts until the user has seen
            // the result; otherwise every idle session would be listed as waiting.
            CLIAgentSessionStatus::Success
                if AgentNotificationsModel::as_ref(app)
                    .notifications()
                    .has_unread_for_terminal_view(terminal_view_id) =>
            {
                WaitReason::Finished
            }
            CLIAgentSessionStatus::Success => return None,
            CLIAgentSessionStatus::InProgress
            | CLIAgentSessionStatus::Failed { .. }
            | CLIAgentSessionStatus::Cancelled => return None,
        };
        let since = self.status_since.get(&terminal_view_id)?;
        Some((reason, since.elapsed()))
    }

    /// What the agent in `terminal_view_id` is doing. `None` while it waits on the user, which the
    /// tab's badges already show, or when it failed.
    pub fn activity(&self, terminal_view_id: EntityId, app: &AppContext) -> Option<AgentActivity> {
        let session = CLIAgentSessionsModel::as_ref(app).session(terminal_view_id)?;
        let since = self.status_since.get(&terminal_view_id)?.elapsed();
        match session.status {
            CLIAgentSessionStatus::InProgress if self.turn_started.contains(&terminal_view_id) => {
                Some(AgentActivity::Working(since))
            }
            CLIAgentSessionStatus::InProgress => Some(AgentActivity::Idle(since)),
            CLIAgentSessionStatus::Success | CLIAgentSessionStatus::Cancelled
                if self.waiting(terminal_view_id, app).is_none() =>
            {
                Some(AgentActivity::Idle(since))
            }
            CLIAgentSessionStatus::Success
            | CLIAgentSessionStatus::Cancelled
            | CLIAgentSessionStatus::Blocked { .. }
            | CLIAgentSessionStatus::Failed { .. } => None,
        }
    }

    /// Gathers what [`compute_tab_signals`] needs about the agent sessions in `pane_group`.
    pub fn tab_facts(
        &self,
        pane_group: &ViewHandle<PaneGroup>,
        app: &AppContext,
    ) -> TabSessionFacts {
        let mut facts = TabSessionFacts::default();
        let sessions = CLIAgentSessionsModel::as_ref(app);
        for (session_uuid, terminal_view) in
            pane_group.as_ref(app).terminal_views_by_session_uuid(app)
        {
            if let Some(track) = self.server(&session_uuid) {
                facts.server.get_or_insert(ServerBadge {
                    port: track.port,
                    is_down: track.down_since.is_some(),
                });
            }
            facts.session_uuids.push(session_uuid);
            let terminal_view_id = terminal_view.id();
            let Some(session) = sessions.session(terminal_view_id) else {
                continue;
            };
            if session.is_remote() {
                continue;
            }
            if facts.agent_checkout.is_none() {
                facts.agent_checkout = match terminal_view.as_ref(app).current_repo_path() {
                    Some(LocalOrRemotePath::Local(path)) => Some(path.clone()),
                    Some(LocalOrRemotePath::Remote(_)) | None => None,
                };
            }
            if let Some(session_id) = &session.session_context.session_id {
                if facts.agent_session_id.is_none() {
                    facts.agent_session_id = Some(session_id.clone());
                }
                facts.agent_session_ids.push(session_id.clone());
            }
            if let Some(waiting) = self.waiting(terminal_view_id, app) {
                facts.waiting = Some(match facts.waiting {
                    Some(existing) if existing.1 >= waiting.1 => existing,
                    _ => waiting,
                });
            }
        }
        facts
    }

    /// Signals for each tab, in the same order as `tabs`.
    pub fn tab_signals(&self, tabs: &[TabSessionFacts]) -> Vec<TabSignals> {
        compute_tab_signals(tabs, &self.scan)
    }
}

/// Derives each tab's signals from its sessions and the latest machine scan.
pub fn compute_tab_signals(tabs: &[TabSessionFacts], scan: &ScanResult) -> Vec<TabSignals> {
    let mut agents_per_checkout: HashMap<&PathBuf, usize> = HashMap::new();
    for checkout in tabs.iter().filter_map(|tab| tab.agent_checkout.as_ref()) {
        *agents_per_checkout.entry(checkout).or_default() += 1;
    }

    let open_session_ids: HashSet<&String> = tabs
        .iter()
        .filter_map(|tab| tab.agent_session_id.as_ref())
        .collect();
    let mut editors_per_file: HashMap<&PathBuf, usize> = HashMap::new();
    for (session_id, files) in &scan.edited_files {
        if open_session_ids.contains(session_id) {
            for file in files {
                *editors_per_file.entry(file).or_default() += 1;
            }
        }
    }

    tabs.iter()
        .map(|tab| {
            let mut contested_files: Vec<PathBuf> = tab
                .agent_session_id
                .as_ref()
                .and_then(|session_id| scan.edited_files.get(session_id))
                .into_iter()
                .flatten()
                .filter(|file| editors_per_file.get(file).is_some_and(|count| *count > 1))
                .cloned()
                .collect();
            contested_files.sort();
            let mut subagents: Vec<Subagent> = tab
                .agent_session_ids
                .iter()
                .filter_map(|session_id| scan.subagents.get(session_id))
                .flatten()
                .cloned()
                .collect();
            subagents::sort_subagents(&mut subagents);
            TabSignals {
                shares_checkout: tab
                    .agent_checkout
                    .as_ref()
                    .is_some_and(|checkout| agents_per_checkout[checkout] > 1),
                contested_files,
                waiting: tab.waiting,
                held_resources: scan
                    .resources
                    .iter()
                    .filter_map(|(kind, state)| match state {
                        ResourceState::InUse {
                            pane: Some(pane), ..
                        } if tab.session_uuids.contains(pane) => Some(*kind),
                        ResourceState::InUse { .. } | ResourceState::Free => None,
                    })
                    .collect(),
                subagents,
                server: tab.server,
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
