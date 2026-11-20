//! What the team reads to know how the tutor is doing: one row of numbers per
//! day. Everything here is a count, a rate or a duration. No name, no id of a
//! child and nothing anybody said is read or returned.

use axum::Json;
use axum::extract::{Query, State};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sqlx::PgPool;

use crate::auth::Operator;
use crate::error::ApiResult;
use crate::state::AppState;

const MAX_METRICS: usize = 40;
const MAX_NAME_CHARS: usize = 40;

/// The agent's own measurements, kept only when they are what they claim to
/// be: a small object of short lower-case names with finite numbers.
pub fn numbers_only(metrics: &Value) -> Option<Value> {
    let object = metrics.as_object()?;
    let kept: Map<String, Value> = object
        .iter()
        .filter(|(name, value)| {
            !name.is_empty()
                && name.chars().count() <= MAX_NAME_CHARS
                && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                && value.as_f64().is_some_and(f64::is_finite)
        })
        .take(MAX_METRICS)
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();
    (!kept.is_empty()).then_some(Value::Object(kept))
}

// What a session costs ----------------------------------------------------------

// List prices in US dollars as published on 2026-11-20. They change: check
// them against the invoices.
// - Anthropic, per million tokens: Sonnet 5.5 $2 in, $10 out; Haiku 5.5 $0.10
//   in, $0.50 out. Cache reads at a tenth of the input price, cache writes
//   (five minutes) at 1.25 times it.
// - Deepgram Nova-3 streaming: $0.0077 a minute of audio sent (list price; a
//   promotion was at $0.0048).
// - AssemblyAI TTS-1: $15 per million characters.
// The tutor's model is the agent's default, `claude-sonnet-5-5`: the agent
// reports its tokens, not its model.
const TUTOR_MODEL: &str = "claude-sonnet-5-5";
const CACHE_READ: f64 = 0.1;
const CACHE_WRITE: f64 = 1.25;
const SPEECH_TO_TEXT_PER_MINUTE: f64 = 0.0077;
const VOICE_PER_MILLION_CHARACTERS: f64 = 15.0;

/// Input and output price per million tokens. A model not known here is
/// priced as the dearer one, so the estimate errs high.
fn model_price(model: &str) -> (f64, f64) {
    if model.contains("haiku") { (0.10, 0.50) } else { (2.0, 10.0) }
}

/// Tokens of one model, split the way they are billed.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tokens {
    pub input: f64,
    pub cache_read: f64,
    pub cache_write: f64,
    pub output: f64,
}

pub fn tokens_cost(model: &str, tokens: Tokens) -> f64 {
    let (input, output) = model_price(model);
    (tokens.input * input
        + tokens.cache_read * input * CACHE_READ
        + tokens.cache_write * input * CACHE_WRITE
        + tokens.output * output)
        / 1e6
}

/// Dollars, to the cent. A sum of nothing is -0.0 in floating point; it is shown as 0.
fn cents(value: f64) -> Value {
    let rounded = (value * 100.0).round() / 100.0;
    json!(if rounded == 0.0 { 0.0 } else { rounded })
}

/// What the sessions of each day cost, by service, from what the agent
/// reported at their end and what the platform's own model calls used.
async fn costs(pool: &PgPool, days: i32, offset: i32) -> Result<Map<String, Value>, sqlx::Error> {
    type Agent = (String, i64, f64, f64, f64, f64, f64, f64, f64);
    // The agent counts every prompt token, cached or not; the uncached part
    // is what is left after reads and writes.
    let agents: Vec<Agent> = sqlx::query_as(
        "select ((created_at + make_interval(mins => $2))::date)::text,
                count(*),
                coalesce(sum((agent_metrics->>'minutes')::float8), 0),
                coalesce(sum((agent_metrics->>'input_tokens')::float8
                             - coalesce((agent_metrics->>'cached_input_tokens')::float8, 0)
                             - coalesce((agent_metrics->>'cache_write_tokens')::float8, 0)), 0),
                coalesce(sum((agent_metrics->>'cached_input_tokens')::float8), 0),
                coalesce(sum((agent_metrics->>'cache_write_tokens')::float8), 0),
                coalesce(sum((agent_metrics->>'output_tokens')::float8), 0),
                -- Older sessions did not report these: their length and the tutor's words stand in.
                coalesce(sum(coalesce((agent_metrics->>'stt_seconds')::float8,
                                      (agent_metrics->>'minutes')::float8 * 60)), 0),
                coalesce(sum(coalesce((agent_metrics->>'tts_chars')::float8, tutor_chars)), 0)
         from sessions
         where agent_metrics ? 'input_tokens' and created_at > now() - make_interval(days => $1 + 1)
         group by 1",
    )
    .bind(days)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    let supervisors: Vec<(String, String, f64, f64, f64, f64)> = sqlx::query_as(
        "select ((s.created_at + make_interval(mins => $2))::date)::text, u.model,
                sum(u.input_tokens)::float8, sum(u.cache_read_tokens)::float8,
                sum(u.cache_write_tokens)::float8, sum(u.output_tokens)::float8
         from session_model_usage u join sessions s on s.id = u.session_id
         where s.created_at > now() - make_interval(days => $1 + 1)
         group by 1, 2",
    )
    .bind(days)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    let mut by_day = Map::new();
    for (day, sessions, minutes, input, cache_read, cache_write, output, heard, spoken) in agents {
        let tutor = tokens_cost(TUTOR_MODEL, Tokens { input, cache_read, cache_write, output });
        let speech_to_text = heard / 60.0 * SPEECH_TO_TEXT_PER_MINUTE;
        let voice = spoken / 1e6 * VOICE_PER_MILLION_CHARACTERS;
        let supervisor: f64 = supervisors
            .iter()
            .filter(|row| row.0 == day)
            .map(|(_, model, input, cache_read, cache_write, output)| {
                let tokens = Tokens {
                    input: *input,
                    cache_read: *cache_read,
                    cache_write: *cache_write,
                    output: *output,
                };
                tokens_cost(model, tokens)
            })
            .sum();
        let total = tutor + speech_to_text + voice + supervisor;
        by_day.insert(
            day,
            json!({
                "sessions": sessions,
                "minutes": (minutes * 10.0).round() / 10.0,
                "tutor": cents(tutor),
                "speech_to_text": cents(speech_to_text),
                "voice": cents(voice),
                "supervisor": cents(supervisor),
                "total": cents(total),
                "per_hour": if minutes > 0.0 { cents(total / minutes * 60.0) } else { Value::Null },
            }),
        );
    }
    Ok(by_day)
}

#[derive(Deserialize)]
pub struct DailyQuery {
    #[serde(default)]
    days: Option<i32>,
}

/// One object per day, newest first, in the server's configured local time.
pub async fn daily(
    _: Operator,
    State(state): State<AppState>,
    Query(query): Query<DailyQuery>,
) -> ApiResult<Json<Value>> {
    let days = query.days.unwrap_or(14).clamp(1, 90);
    let offset = state.config.timezone_offset_minutes;
    let rows: Vec<(sqlx::types::Json<Value>,)> = sqlx::query_as(
        "with bounds as (
           select d::date as day
           from generate_series(
             ((now() + make_interval(mins => $2))::date - ($1 - 1)),
             (now() + make_interval(mins => $2))::date,
             interval '1 day') d
         ),
         s as (
           select *, (created_at + make_interval(mins => $2))::date as day,
                  extract(epoch from (ended_at - coalesce(started_at, created_at))) as seconds
           from sessions
           where created_at > now() - make_interval(days => $1 + 1)
         ),
         e as (
           select x.*, s.day from session_exercises x join s on s.id = x.session_id
         ),
         f as (
           select g.gate, g.action, s.day, g.session_id from session_flags g join s on s.id = g.session_id
         )
         select jsonb_build_object(
           'day', b.day,
           'sessions', jsonb_build_object(
             'started', (select count(*) from s where s.day = b.day and started_at is not null),
             'as_child', (select count(*) from s where s.day = b.day and audience = 'child' and started_at is not null),
             'never_started', (select count(*) from s where s.day = b.day and started_at is null),
             'under_a_minute', (select count(*) from s where s.day = b.day and seconds < 60),
             'median_minutes', (select round((percentile_cont(0.5) within group (order by seconds) / 60)::numeric, 1)
                                from s where s.day = b.day and seconds is not null),
             'median_seconds_to_start', (select round(percentile_cont(0.5) within group (
                                           order by extract(epoch from (started_at - created_at)))::numeric, 1)
                                         from s where s.day = b.day and started_at is not null),
             'students', (select count(distinct child_id) from s where s.day = b.day and child_id is not null)
           ),
           'talk', jsonb_build_object(
             'child_turns', (select sum(child_turns) from s where s.day = b.day),
             'tutor_turns', (select sum(tutor_turns) from s where s.day = b.day),
             'tutor_share_of_words', (select round((sum(tutor_chars)::numeric
                                        / nullif(sum(tutor_chars) + sum(child_chars), 0)), 2)
                                      from s where s.day = b.day)
           ),
           'exercises', jsonb_build_object(
             'posed', (select count(*) from e where e.day = b.day),
             'not_checkable', (select coalesce(sum(unchecked_exercises), 0) from s where s.day = b.day),
             'correct', (select count(*) from e where e.day = b.day and status = 'correct'),
             'failed', (select count(*) from e where e.day = b.day and status = 'failed'),
             'left_open', (select count(*) from e where e.day = b.day and status = 'dropped'),
             'first_try_rate', (select round(avg(case when first_try then 1 else 0 end)::numeric, 2)
                                from e where e.day = b.day and not probe and status in ('correct', 'failed')),
             'hints_per_exercise', (select round(avg(hints)::numeric, 2)
                                    from e where e.day = b.day and not probe and status in ('correct', 'failed')),
             'answer_given_away', (select count(*) from e where e.day = b.day and answer_said),
             'sessions_with_one', (select count(distinct session_id) from e where e.day = b.day)
           ),
           'safety', jsonb_build_object(
             'turns_stopped', (select count(*) from f where f.day = b.day),
             'sessions_with_a_stop', (select count(distinct session_id) from f where f.day = b.day),
             'by_gate_and_action', (select coalesce(jsonb_object_agg(k, n), '{}'::jsonb) from (
                                      select gate || ':' || action as k, count(*) as n
                                      from f where f.day = b.day group by 1) t)
           ),
           'not_working', jsonb_build_object(
             'warned', (select count(*) from s where s.day = b.day and stop_step is not null),
             'closed', (select count(*) from s where s.day = b.day and stop_step = 'end')
           ),
           'breaks', jsonb_build_object(
             'taken', (select coalesce(sum(breaks), 0) from s where s.day = b.day),
             'sessions_with_one', (select count(*) from s where s.day = b.day and breaks > 0)
           ),
           'agent', (select coalesce(jsonb_object_agg(name, value), '{}'::jsonb) from (
                       select m.key as name, round(avg((m.value)::text::numeric), 2) as value
                       from s, jsonb_each(s.agent_metrics) m
                       where s.day = b.day and jsonb_typeof(m.value) = 'number'
                       group by m.key) t),
           'family', jsonb_build_object(
             'reviews', (select count(*) from session_reviews r join s on s.id = r.session_id where s.day = b.day),
             'notices', (select count(*) from notices n
                         where (n.created_at + make_interval(mins => $2))::date = b.day),
             'notices_emailed', (select count(*) from notices n
                                 where (n.created_at + make_interval(mins => $2))::date = b.day
                                   and n.emailed_at is not null)
           )
         )
         from bounds b order by b.day desc",
    )
    .bind(days)
    .bind(offset)
    .fetch_all(&state.pool)
    .await?;
    let costs = costs(&state.pool, days, offset).await?;
    let days: Vec<Value> = rows
        .into_iter()
        .map(|(row,)| {
            let mut day = row.0;
            let cost = day["day"].as_str().and_then(|name| costs.get(name)).cloned();
            day["cost_usd"] = cost.unwrap_or(Value::Null);
            day
        })
        .collect();
    Ok(Json(json!({ "timezone_offset_minutes": offset, "days": days })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_priced_the_way_they_are_billed() {
        let million =
            |input, cache_read, cache_write, output| Tokens { input, cache_read, cache_write, output };
        // A million of each, on the tutor's model: 2 + 0.2 + 2.5 + 10.
        assert!((tokens_cost("claude-sonnet-5-5", million(1e6, 1e6, 1e6, 1e6)) - 14.7).abs() < 1e-9);
        assert!((tokens_cost("claude-haiku-5-5", million(1e6, 0.0, 0.0, 1e6)) - 0.6).abs() < 1e-9);
        // An unknown model is priced high, not free.
        assert!(tokens_cost("some-new-model", million(1e6, 0.0, 0.0, 0.0)) >= 2.0);
        assert_eq!(cents(0.126), json!(0.13));
        assert_eq!(cents(std::iter::empty::<f64>().sum()).to_string(), "0.0");
    }

    #[test]
    fn only_named_numbers_are_kept_from_what_the_agent_reports() {
        let kept = numbers_only(&json!({
            "first_word_ms": 2600,
            "stt_confidence": 0.91,
            "note": "the student said hello",
            "Bad Name": 3,
            "nested": { "a": 1 },
            "nothing": null
        }))
        .unwrap();
        assert_eq!(kept, json!({ "first_word_ms": 2600, "stt_confidence": 0.91 }));
        assert!(numbers_only(&json!({ "note": "words" })).is_none());
        assert!(numbers_only(&json!([1, 2, 3])).is_none());
    }
}
