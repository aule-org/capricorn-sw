-- Sessions run by the session engine (`engine/`). The engine keeps its state
-- in memory and is not serializable, so what is stored is what went into it,
-- in order: a session is rebuilt by applying the same inputs again.

-- Who decides what happens in the session: null is the tutor with the
-- supervisor, as before; 'engine' is the session engine.
alter table sessions add column flow text check (flow in ('engine'));

-- The inputs hold what the student said, so they follow the rule of
-- session_turns: they go after the review unless the guardian agreed to keep
-- transcripts, and then only for as long as a transcript is kept.
create table session_flow_inputs (
    session_id uuid not null references sessions (id) on delete cascade,
    seq integer not null check (seq >= 0),
    -- the engine's clock when the input was applied: milliseconds since the
    -- first input, whose own row keeps the wall time in `at`
    at_ms bigint not null check (at_ms >= 0),
    input jsonb not null,
    -- lines the engine answered with that the tutor has not been handed yet:
    -- only for an answer typed on the page, until the agent's next report
    queued jsonb,
    at timestamptz not null default now(),
    primary key (session_id, seq)
);
