use warpui::{App, EntityId};

use super::{CLIAgentResume, LaunchCommand};
use crate::terminal::CLIAgent;
use crate::terminal::cli_agent_sessions::{
    CLIAgentInputState, CLIAgentSession, CLIAgentSessionContext, CLIAgentSessionStatus,
    CLIAgentSessionsModel,
};

const SESSION_ID: &str = "3f2a9c1e-7b4d-4e8a-9f10-2c3d4e5f6a7b";

fn session(agent: CLIAgent, session_id: Option<&str>) -> CLIAgentSession {
    CLIAgentSession {
        agent,
        status: CLIAgentSessionStatus::Success,
        session_context: CLIAgentSessionContext {
            session_id: session_id.map(str::to_owned),
            query: Some("fix the upload bug".to_owned()),
            ..Default::default()
        },
        input_state: CLIAgentInputState::Closed,
        should_auto_toggle_input: false,
        listener: None,
        plugin_version: None,
        remote_host: None,
        draft_text: None,
        custom_command_prefix: None,
        received_rich_notification: true,
    }
}

fn resume_command(command: &str) -> Option<String> {
    CLIAgentResume::capture(&session(CLIAgent::Claude, Some(SESSION_ID)), command)?.command()
}

#[test]
fn replays_skip_permissions_flag() {
    assert_eq!(
        resume_command("claude --dangerously-skip-permissions").as_deref(),
        Some(format!("claude --dangerously-skip-permissions --resume {SESSION_ID}").as_str())
    );
}

#[test]
fn plain_launch_resumes_without_extra_permissions() {
    assert_eq!(
        resume_command("claude").as_deref(),
        Some(format!("claude --resume {SESSION_ID}").as_str())
    );
}

#[test]
fn keeps_alias_and_carried_options() {
    assert_eq!(
        resume_command("cc --model=opus --permission-mode plan").as_deref(),
        Some(format!("cc --model opus --permission-mode plan --resume {SESSION_ID}").as_str())
    );
}

#[test]
fn drops_previous_session_selectors() {
    for command in [
        "claude -c",
        "claude --continue --fork-session",
        "claude --resume 11111111-2222-3333-4444-555555555555",
        "claude -r",
        "claude --session-id=abc",
        "claude --session-id abc",
    ] {
        assert_eq!(
            resume_command(command).as_deref(),
            Some(format!("claude --resume {SESSION_ID}").as_str()),
            "{command}"
        );
    }
}

#[test]
fn drops_unknown_arguments_and_initial_prompt() {
    assert_eq!(
        resume_command("claude --add-dir ../other \"fix the bug\"").as_deref(),
        Some(format!("claude --resume {SESSION_ID}").as_str())
    );
}

#[test]
fn drops_unsafe_option_values() {
    assert_eq!(
        resume_command("claude --model \"a b\" --model $(whoami) --model --verbose").as_deref(),
        Some(format!("claude --resume {SESSION_ID}").as_str())
    );
}

#[test]
fn refuses_commands_that_cannot_be_replayed_safely() {
    for command in [
        "claude -p \"summarize\"",
        "claude --print",
        "ANTHROPIC_API_KEY=secret claude",
        "claude; rm -rf ~",
        "$(curl evil) --dangerously-skip-permissions",
        "`id`",
        "claude \"unterminated",
        "",
    ] {
        assert_eq!(resume_command(command), None, "{command}");
    }
}

#[test]
fn only_resumes_claude_sessions_with_a_uuid() {
    let command = "claude";
    assert!(CLIAgentResume::capture(&session(CLIAgent::Claude, None), command).is_none());
    assert!(
        CLIAgentResume::capture(&session(CLIAgent::Claude, Some("abc; rm -rf ~")), command)
            .is_none()
    );
    assert!(
        CLIAgentResume::capture(&session(CLIAgent::Codex, Some(SESSION_ID)), command).is_none()
    );
}

#[test]
fn does_not_resume_remote_sessions() {
    let mut remote = session(CLIAgent::Claude, Some(SESSION_ID));
    remote.remote_host = Some("me@server".to_owned());
    assert!(CLIAgentResume::capture(&remote, "claude").is_none());
}

#[test]
fn normalizes_session_id_and_keeps_title() {
    let resume = CLIAgentResume::capture(
        &session(CLIAgent::Claude, Some(&SESSION_ID.to_uppercase())),
        "claude",
    )
    .unwrap();
    assert_eq!(resume.session_id, SESSION_ID);
    assert_eq!(resume.title.as_deref(), Some("fix the upload bug"));
}

#[test]
fn revalidates_stored_values() {
    let mut resume =
        CLIAgentResume::capture(&session(CLIAgent::Claude, Some(SESSION_ID)), "claude").unwrap();
    resume.launch = LaunchCommand {
        program: "claude".to_owned(),
        args: vec!["--model".to_owned(), "x; rm -rf ~".to_owned()],
    };
    assert_eq!(resume.command(), None);

    resume.launch.args = vec!["--add-dir".to_owned(), "/".to_owned()];
    assert_eq!(resume.command(), None);

    resume.launch.args.clear();
    resume.session_id = "not-a-uuid".to_owned();
    assert_eq!(resume.command(), None);
}

#[test]
fn skips_conversation_whose_transcript_is_gone() {
    let mut resume =
        CLIAgentResume::capture(&session(CLIAgent::Claude, Some(SESSION_ID)), "claude").unwrap();
    resume.transcript_path = Some("/nonexistent/transcript.jsonl".into());
    assert_eq!(resume.command(), None);

    let transcript = tempfile::NamedTempFile::new().unwrap();
    resume.transcript_path = Some(transcript.path().to_path_buf());
    assert!(resume.command().is_some());
}

#[test]
fn round_trips_through_json() {
    let resume = CLIAgentResume::capture(
        &session(CLIAgent::Claude, Some(SESSION_ID)),
        "claude --dangerously-skip-permissions",
    )
    .unwrap();
    let json = serde_json::to_string(&resume).unwrap();
    assert_eq!(
        serde_json::from_str::<CLIAgentResume>(&json).unwrap(),
        resume
    );
}

fn saved_resume(session_id: &str) -> CLIAgentResume {
    CLIAgentResume {
        agent: CLIAgent::Claude,
        session_id: session_id.to_owned(),
        launch: LaunchCommand {
            program: "claude".to_owned(),
            args: vec![],
        },
        title: Some("fix the upload bug".to_owned()),
        transcript_path: None,
    }
}

#[test]
fn resumed_session_starts_with_saved_title() {
    App::test((), |mut app| async move {
        let model = app.add_singleton_model(|_| CLIAgentSessionsModel::new());
        let resumed_view = EntityId::new();
        let other_view = EntityId::new();
        let mut detected_session = session(CLIAgent::Claude, None);
        detected_session.session_context.query = None;

        model.update(&mut app, |sessions, ctx| {
            sessions.expect_resumed_session(resumed_view, saved_resume(SESSION_ID));
            sessions.set_session(resumed_view, detected_session.clone(), ctx);
            sessions.set_session(other_view, detected_session.clone(), ctx);
        });
        model.read(&app, |sessions, _| {
            let prompt = |view| {
                sessions
                    .session(view)
                    .unwrap()
                    .session_context
                    .latest_user_prompt()
            };
            assert_eq!(prompt(resumed_view).as_deref(), Some("fix the upload bug"));
            assert_eq!(prompt(other_view), None);
        });

        // Once the resumed agent exits, a later agent in the same pane starts untitled.
        model.update(&mut app, |sessions, ctx| {
            sessions.remove_session(resumed_view, ctx);
            sessions.set_session(resumed_view, detected_session, ctx);
        });
        model.read(&app, |sessions, _| {
            assert_eq!(
                sessions
                    .session(resumed_view)
                    .unwrap()
                    .session_context
                    .latest_user_prompt(),
                None
            );
        });
    });
}

#[test]
fn resumed_conversation_survives_saves_until_the_agent_exits() {
    App::test((), |mut app| async move {
        let model = app.add_singleton_model(|_| CLIAgentSessionsModel::new());
        let view = EntityId::new();
        let resume = saved_resume(SESSION_ID);
        let command = || "claude --resume 3f2a9c1e-7b4d-4e8a-9f10-2c3d4e5f6a7b".to_owned();

        // Before the agent has started, and while it has not reported its conversation yet.
        model.update(&mut app, |sessions, ctx| {
            sessions.expect_resumed_session(view, resume.clone());
            assert_eq!(sessions.resume_for(view, command), Some(resume.clone()));
            sessions.set_session(view, session(CLIAgent::Claude, None), ctx);
            assert_eq!(sessions.resume_for(view, command), Some(resume.clone()));
        });

        // Once the agent reports a conversation, that one wins, e.g. after `/clear`.
        let newer_id = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
        model.update(&mut app, |sessions, ctx| {
            sessions.set_session(view, session(CLIAgent::Claude, Some(newer_id)), ctx);
            assert_eq!(
                sessions
                    .resume_for(view, command)
                    .map(|resume| resume.session_id),
                Some(newer_id.to_owned())
            );
        });

        model.update(&mut app, |sessions, ctx| {
            sessions.remove_session(view, ctx);
            assert_eq!(sessions.resume_for(view, command), None);
        });
    });
}
