-- Push notifications to guardians' phones.

alter table guardians add column notices_by_push boolean not null default true;
alter table notices add column pushed_at timestamptz;

-- A phone's registration with the push service. A token belongs to whoever
-- signed in on that phone last.
create table push_tokens (
    token text primary key check (length(token) between 20 and 4096),
    guardian_id uuid not null references guardians (id) on delete cascade,
    platform text not null check (platform in ('android', 'ios', 'web')),
    created_at timestamptz not null default now(),
    last_seen_at timestamptz not null default now()
);
create index push_tokens_guardian on push_tokens (guardian_id);
