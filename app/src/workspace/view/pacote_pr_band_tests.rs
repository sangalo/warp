use chrono::{Duration, TimeZone};

use super::*;

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 8, 12, 0, 0).unwrap()
}

fn state(json: &str) -> PacoteState {
    match read_state(Ok(json.to_owned())) {
        Reading::Parsed(state) => state,
        other => panic!("expected a parsed state, got {other:?}"),
    }
}

fn with_banner(updated_at: DateTime<Utc>, active: bool) -> PacoteState {
    state(&format!(
        r#"{{"versao":1,"ativo":{active},"atualizadoEm":"{}","commitsAFrente":5,"pr":null,
            "testes":null,"avisos":[],
            "faixa":{{"linhas":["PR #1 sem testes","  ","abra o PR"],"nivel":"atencao"}}}}"#,
        updated_at.to_rfc3339()
    ))
}

#[test]
fn shows_fresh_banner_with_at_most_two_non_blank_lines() {
    let state = with_banner(now() - Duration::seconds(30), true);
    let (lines, level) = visible_banner(&state, now()).unwrap();
    assert_eq!(lines, vec!["PR #1 sem testes", "abra o PR"]);
    assert_eq!(level, Level::Atencao);
}

#[test]
fn stays_silent_when_inactive() {
    assert_eq!(visible_banner(&with_banner(now(), false), now()), None);
}

#[test]
fn stays_silent_when_stale() {
    let at_limit = with_banner(now() - Duration::minutes(3), true);
    assert!(visible_banner(&at_limit, now()).is_some());
    let stale = with_banner(now() - Duration::minutes(3) - Duration::seconds(1), true);
    assert_eq!(visible_banner(&stale, now()), None);
}

#[test]
fn stays_silent_without_banner() {
    let state =
        state(r#"{"versao":1,"ativo":true,"atualizadoEm":"2026-11-08T12:00:00Z","faixa":null}"#);
    assert_eq!(visible_banner(&state, now()), None);
}

#[test]
fn stays_silent_for_unknown_format_version() {
    let state = state(
        r#"{"versao":2,"ativo":true,"atualizadoEm":"2026-11-08T12:00:00Z",
            "faixa":{"linhas":["x"],"nivel":"info"}}"#,
    );
    assert_eq!(visible_banner(&state, now()), None);
}

#[test]
fn unknown_level_is_treated_as_info() {
    let state = state(
        r#"{"versao":1,"ativo":true,"atualizadoEm":"2026-11-08T12:00:00Z",
            "faixa":{"linhas":["x"],"nivel":"outro"}}"#,
    );
    assert_eq!(visible_banner(&state, now()).unwrap().1, Level::Info);
}

#[test]
fn half_written_file_is_invalid_not_missing() {
    let half_written = r#"{"versao":1,"ativo":tr"#.to_owned();
    assert_eq!(read_state(Ok(half_written)), Reading::Invalid);
    assert_eq!(read_state(Ok(String::new())), Reading::Invalid);
}

#[test]
fn absent_file_is_missing() {
    let error = std::io::Error::from(std::io::ErrorKind::NotFound);
    assert_eq!(read_state(Err(error)), Reading::Missing);
}
