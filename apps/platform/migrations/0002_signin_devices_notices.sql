-- Family sign-in, devices, notices to guardians and the event outbox.

-- families: sign-in ------------------------------------------------------------

alter table guardians
    add column language text not null default 'es' check (language in ('es', 'en')),
    add column notices_by_email boolean not null default true;

-- One pending sign-in code per email address, whether or not an account
-- exists, so asking for a code never reveals which addresses have one.
create table login_codes (
    email citext primary key,
    code_hash text not null,
    expires_at timestamptz not null,
    attempts integer not null default 0,
    -- codes sent since window_started_at, to limit how many one address gets
    sent_count integer not null default 1,
    window_started_at timestamptz not null default now()
);

-- A signed-in guardian. The token itself is never stored.
create table guardian_sessions (
    token_hash text primary key,
    guardian_id uuid not null references guardians (id) on delete cascade,
    created_at timestamptz not null default now(),
    expires_at timestamptz not null
);
create index guardian_sessions_guardian on guardian_sessions (guardian_id);

-- devices ----------------------------------------------------------------------

-- A device is known by its serial number and proves itself with the HMAC key
-- burned into it when it was made. The key never travels; the server keeps a
-- copy to check the device's answers. A MAC address identifies nothing here.
create table devices (
    id uuid primary key,
    serial text not null unique check (serial ~ '^[A-Za-z0-9._-]{4,32}$'),
    hmac_key bytea not null check (octet_length(hmac_key) = 32),
    model text not null check (length(model) between 1 and 60),
    family_id uuid references families (id) on delete set null,
    child_id uuid references children (id) on delete set null,
    claimed_at timestamptz,
    firmware_version text check (length(firmware_version) <= 40),
    last_seen_at timestamptz,
    -- the newest timestamp the device has signed, so a signed request cannot be replayed
    last_signed_at bigint not null default 0,
    created_at timestamptz not null default now()
);
create index devices_family on devices (family_id);

-- A device that is not linked to a family shows a code. Linking needs both
-- halves: a guardian who can read the code, and the device proving it holds
-- its key by signing the challenge.
create table device_claims (
    device_id uuid primary key references devices (id) on delete cascade,
    code text not null unique,
    challenge text not null,
    proved_at timestamptz,
    guardian_id uuid references guardians (id) on delete cascade,
    child_id uuid references children (id) on delete cascade,
    expires_at timestamptz not null
);

create table firmware (
    id uuid primary key,
    model text not null check (length(model) between 1 and 60),
    version text not null check (length(version) <= 40),
    major integer not null,
    minor integer not null,
    patch integer not null,
    url text not null check (url ~ '^https://'),
    sha256 text not null check (sha256 ~ '^[0-9a-f]{64}$'),
    created_at timestamptz not null default now(),
    unique (model, version)
);
create index firmware_latest on firmware (model, major desc, minor desc, patch desc);

alter table sessions add column device_id uuid references devices (id) on delete set null;

-- sessions: safety flags ---------------------------------------------------------

-- What a safety gate stopped, as fixed categories. Never the words.
create table session_flags (
    id bigint generated always as identity primary key,
    session_id uuid not null references sessions (id) on delete cascade,
    gate text not null check (gate in ('moderation_input', 'moderation_output', 'physical_safety', 'abuse_limit')),
    action text not null check (action in ('redirect', 'adult_help', 'clarify', 'support', 'unavailable', 'cooldown')),
    categories text[] not null,
    created_at timestamptz not null default now()
);
create index session_flags_session on session_flags (session_id);

-- notices ------------------------------------------------------------------------

-- What a family is told: a summary after a session, or that something needs
-- their attention. Written from fixed text and the review, never from a
-- child's words.
create table notices (
    id uuid primary key,
    family_id uuid not null references families (id) on delete cascade,
    child_id uuid references children (id) on delete cascade,
    session_id uuid,
    kind text not null check (kind in ('session_summary', 'attention')),
    title text not null check (length(title) <= 200),
    body text not null check (length(body) <= 2000),
    created_at timestamptz not null default now(),
    read_at timestamptz,
    emailed_at timestamptz,
    -- one notice of each kind per session, however often its event is delivered
    unique (session_id, kind)
);
create index notices_family on notices (family_id, created_at desc);

-- events -------------------------------------------------------------------------

-- An event is written in the same transaction as the change it describes and
-- delivered afterwards by the worker, at least once.
create table outbox (
    id uuid primary key,
    type text not null,
    subject text,
    data jsonb not null,
    time timestamptz not null default now(),
    attempts integer not null default 0,
    delivered_at timestamptz,
    last_error text
);
create index outbox_pending on outbox (time) where delivered_at is null;
