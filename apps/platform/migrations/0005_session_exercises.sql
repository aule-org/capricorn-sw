-- Exercises the tutor stated in a session, checked by the platform. The
-- answer is worked out here and never sent to the tutor while the exercise
-- is open, so it cannot be given away.

create table session_exercises (
    id bigint generated always as identity primary key,
    session_id uuid not null references sessions (id) on delete cascade,
    skill_id text,
    expression text not null,
    answer text not null,
    form text not null default 'any' check (form in ('any', 'simplified', 'mixed')),
    -- asked to find out what the student can do: no hints, no right or wrong
    probe boolean not null,
    -- answers that counted (one nobody could read does not)
    attempts integer not null default 0,
    hints integer not null default 0,
    status text not null default 'open' check (status in ('open', 'correct', 'failed', 'dropped')),
    first_try boolean,
    -- a fixed name for the mistake, never the student's words
    misconception text,
    created_at timestamptz not null default now(),
    resolved_at timestamptz
);
create index session_exercises_session on session_exercises (session_id, id);
