//! What the team reads to know how the tutor is doing: one row of numbers per
//! day. Everything here is a count, a rate or a duration. No name, no id of a
//! child and nothing anybody said is read or returned.

use axum::Json;
use axum::extract::{Query, State};
use serde::Deserialize;
use serde_json::{Map, Value, json};

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
    let days: Vec<Value> = rows.into_iter().map(|(row,)| row.0).collect();
    Ok(Json(json!({ "timezone_offset_minutes": offset, "days": days })))
}

#[cfg(test)]
mod tests {
    use super::*;

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
