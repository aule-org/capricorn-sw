-- Breaks in a long session. The platform offers one when a block of work is
-- over or the supervisor reads tiredness; the agent says when it was taken.

alter table sessions
    add column breaks integer not null default 0,
    add column last_break_at timestamptz,
    add column pause_offered_at timestamptz,
    add column tired_at timestamptz;
