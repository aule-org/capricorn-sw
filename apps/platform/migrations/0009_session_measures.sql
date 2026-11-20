-- Numbers kept about a session after its words are gone: how much each side
-- spoke, how many exercises could not be checked, and what the agent measured
-- itself (timings, counts). Counts and timings only, never text.

alter table sessions
    add column child_turns integer,
    add column tutor_turns integer,
    add column child_chars integer,
    add column tutor_chars integer,
    add column unchecked_exercises integer not null default 0,
    add column agent_metrics jsonb;
