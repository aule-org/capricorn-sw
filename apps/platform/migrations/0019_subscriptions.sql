-- A family's plan when it is not the trial (which is counted from its first
-- session and needs no row): paid every month, or granted by the team.
create table subscriptions (
    id uuid primary key,
    family_id uuid not null references families (id) on delete cascade,
    plan text not null check (plan in ('paid', 'granted')),
    starts_at timestamptz not null default now(),
    ends_at timestamptz not null,
    payment_token text check (length(payment_token) <= 200),
    provider_subscription text check (length(provider_subscription) <= 200),
    cancelled_at timestamptz,
    granted_by text check (length(granted_by) <= 254),
    note text check (length(note) <= 500),
    created_at timestamptz not null default now(),
    check (ends_at > starts_at)
);
create index subscriptions_family_ends on subscriptions (family_id, ends_at desc);
