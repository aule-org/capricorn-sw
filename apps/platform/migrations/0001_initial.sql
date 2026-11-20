-- Aule platform, first schema. Each block belongs to one module, and a
-- module only reads and writes its own tables.

create extension if not exists vector;
create extension if not exists citext;

-- families ------------------------------------------------------------------

create table families (
    id uuid primary key,
    name text not null check (length(name) between 1 and 120),
    created_at timestamptz not null default now()
);

create table guardians (
    id uuid primary key,
    family_id uuid not null references families (id) on delete cascade,
    email citext not null unique,
    name text not null check (length(name) between 1 and 120),
    created_at timestamptz not null default now()
);

-- A child has no account. This row is what the tutor is allowed to know.
create table children (
    id uuid primary key,
    family_id uuid not null references families (id) on delete cascade,
    nickname text not null check (length(nickname) between 1 and 40),
    birth_year integer check (birth_year between 2000 and 2100),
    grade text check (length(grade) <= 60),
    language text not null default 'es' check (language in ('es', 'en')),
    interests text[] not null default '{}' check (cardinality(interests) <= 10),
    notes text check (length(notes) <= 1000),
    created_at timestamptz not null default now()
);
create index children_family on children (family_id);

-- consent -------------------------------------------------------------------

-- Append-only: a revocation is a new row with granted = false, so the proof of
-- what a guardian accepted and when is never overwritten. The current answer
-- for a purpose is its latest row. Rows outlive the child (child_id is kept as
-- a plain value) because the proof must survive the erasure it authorised.
create table consent_records (
    id uuid primary key,
    family_id uuid not null,
    child_id uuid not null,
    guardian_id uuid,
    purpose text not null check (purpose in ('voice', 'transcripts', 'memory')),
    granted boolean not null,
    policy_version text not null check (length(policy_version) between 1 and 40),
    locale text not null default 'es-PE' check (length(locale) <= 20),
    source text not null check (length(source) between 1 and 40),
    created_at timestamptz not null default now()
);
create index consent_current on consent_records (child_id, purpose, created_at desc);

create function consent_records_are_append_only() returns trigger language plpgsql as $$
begin
    raise exception 'consent_records is append-only';
end $$;
create trigger consent_records_no_change before update or delete on consent_records
    for each row execute function consent_records_are_append_only();

-- sessions ------------------------------------------------------------------

create table sessions (
    id uuid primary key,
    -- null for an operator session with no child profile
    child_id uuid references children (id) on delete cascade,
    audience text not null check (audience in ('child', 'operator')),
    room text not null unique,
    language text not null check (language in ('es', 'en')),
    status text not null default 'created' check (status in ('created', 'active', 'ended')),
    -- SHA-256 of the token the agent uses for this session only
    agent_token_hash text not null unique,
    keep_transcript boolean not null default false,
    use_memory boolean not null default false,
    created_at timestamptz not null default now(),
    started_at timestamptz,
    ended_at timestamptz,
    speaking_seconds integer check (speaking_seconds >= 0),
    reviewed_at timestamptz
);
create index sessions_child on sessions (child_id, created_at desc);
create index sessions_open on sessions (created_at) where status <> 'ended';

-- What was said. Deleted when the review finishes unless the guardian agreed
-- to keep transcripts, and then after a short fixed time (see the worker).
create table session_turns (
    session_id uuid not null references sessions (id) on delete cascade,
    seq integer not null check (seq >= 0),
    role text not null check (role in ('child', 'tutor')),
    content text not null check (length(content) <= 4000),
    at timestamptz not null default now(),
    primary key (session_id, seq)
);

-- learning ------------------------------------------------------------------

create table skills (
    id text primary key check (id ~ '^[a-z0-9][a-z0-9._-]{0,79}$'),
    subject text not null check (length(subject) between 1 and 60),
    name text not null check (length(name) between 1 and 160),
    grade text check (length(grade) <= 60),
    description text check (length(description) <= 600),
    prerequisites text[] not null default '{}'
);

-- Calibrated difficulty, in logits (0 = an average question). question_id ''
-- holds the difficulty of the skill itself, which a new question inherits.
create table item_difficulty (
    skill_id text not null,
    question_id text not null default '',
    value double precision not null default 0,
    answers integer not null default 0,
    primary key (skill_id, question_id)
);

-- The learner model is a replay of this table: fixing a row and reading again
-- recomputes everything.
create table attempts (
    id uuid primary key,
    child_id uuid not null references children (id) on delete cascade,
    session_id uuid references sessions (id) on delete set null,
    skill_id text not null,
    question_id text,
    correct boolean not null,
    credit double precision check (credit between 0 and 1),
    choice boolean not null default false,
    difficulty double precision,
    source text not null check (source in ('review', 'tutor', 'guardian', 'import')),
    at timestamptz not null,
    created_at timestamptz not null default now()
);
create index attempts_child on attempts (child_id, at);

-- A guardian or teacher saying "they already know this" (or the opposite).
create table corrections (
    id uuid primary key,
    child_id uuid not null references children (id) on delete cascade,
    skill_id text not null,
    level double precision not null check (level between 0 and 1),
    at timestamptz not null,
    created_at timestamptz not null default now()
);
create index corrections_child on corrections (child_id, at);

-- memory --------------------------------------------------------------------

-- What the tutor remembers about a child between sessions: short statements
-- derived from a session, never the transcript. 1024 dimensions is bge-m3.
create table memories (
    id uuid primary key,
    child_id uuid not null references children (id) on delete cascade,
    session_id uuid references sessions (id) on delete set null,
    kind text not null check (kind in ('fact', 'preference', 'struggle', 'success', 'summary')),
    content text not null check (length(content) between 1 and 600),
    embedding vector(1024),
    created_at timestamptz not null default now()
);
create index memories_child on memories (child_id, created_at desc);
create index memories_embedding on memories using hnsw (embedding vector_cosine_ops);

-- supervisor ----------------------------------------------------------------

create table session_briefs (
    session_id uuid primary key references sessions (id) on delete cascade,
    brief jsonb not null,
    created_at timestamptz not null default now()
);

-- Guidance the supervisor wrote while the session was running. after_seq is
-- the last turn it had read.
create table session_guidance (
    id bigint generated always as identity primary key,
    session_id uuid not null references sessions (id) on delete cascade,
    after_seq integer not null,
    guidance text not null check (length(guidance) <= 600),
    signals jsonb not null default '{}',
    created_at timestamptz not null default now()
);
create index session_guidance_session on session_guidance (session_id, id);

create table session_reviews (
    session_id uuid primary key references sessions (id) on delete cascade,
    child_id uuid references children (id) on delete cascade,
    guardian_summary text not null,
    next_step text,
    -- fixed categories only, never the child's words
    flags text[] not null default '{}',
    created_at timestamptz not null default now()
);
create index session_reviews_child on session_reviews (child_id, created_at desc);

-- worker --------------------------------------------------------------------

-- Jobs run in the worker process, claimed with FOR UPDATE SKIP LOCKED.
create table jobs (
    id bigint generated always as identity primary key,
    kind text not null,
    payload jsonb not null default '{}',
    run_at timestamptz not null default now(),
    attempts integer not null default 0,
    locked_until timestamptz,
    done_at timestamptz,
    last_error text
);
create index jobs_due on jobs (run_at) where done_at is null;
