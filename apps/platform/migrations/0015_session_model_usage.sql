-- What the platform's own model calls used, per session and model: the plan
-- before a session, the guidance during it and the review after it. Counts
-- only. With what the agent reports at the end of a session, it gives what
-- the session cost (see metrics::daily).
create table session_model_usage (
    session_id uuid not null references sessions (id) on delete cascade,
    model text not null check (length(model) between 1 and 80),
    calls integer not null default 0,
    input_tokens bigint not null default 0,
    cache_read_tokens bigint not null default 0,
    cache_write_tokens bigint not null default 0,
    output_tokens bigint not null default 0,
    primary key (session_id, model)
);
