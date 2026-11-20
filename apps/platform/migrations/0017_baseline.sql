-- The whole schema as migrations 0001 to 0016 left it, collapsed into one file
-- on 2026-11-20 (written by pg_dump from a database those migrations built).
-- A new database runs this and what follows. A database that already ran
-- 0001 to 0016 is marked as having run this one instead, at start, without
-- changing anything (see `migrate` in src/lib.rs).
--
-- Tables, columns and keys are now added by changing an entity in src/entity/
-- and generating the next migration from it (README: "Changing the schema").

CREATE EXTENSION IF NOT EXISTS citext WITH SCHEMA public;

CREATE EXTENSION IF NOT EXISTS vector WITH SCHEMA public;

CREATE FUNCTION public.consent_records_are_append_only() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
begin
    raise exception 'consent_records is append-only';
end $$;

CREATE TABLE public.attempts (
    id uuid NOT NULL,
    child_id uuid NOT NULL,
    session_id uuid,
    skill_id text NOT NULL,
    question_id text,
    correct boolean NOT NULL,
    credit double precision,
    choice boolean DEFAULT false NOT NULL,
    difficulty double precision,
    source text NOT NULL,
    at timestamp with time zone NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT attempts_credit_check CHECK (((credit >= (0)::double precision) AND (credit <= (1)::double precision))),
    CONSTRAINT attempts_source_check CHECK ((source = ANY (ARRAY['review'::text, 'tutor'::text, 'guardian'::text, 'import'::text, 'engine'::text])))
);

CREATE TABLE public.children (
    id uuid NOT NULL,
    family_id uuid NOT NULL,
    nickname text NOT NULL,
    birth_year integer,
    grade text,
    language text DEFAULT 'es'::text NOT NULL,
    interests text[] DEFAULT '{}'::text[] NOT NULL,
    notes text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    tutor_name text DEFAULT 'Aule'::text NOT NULL,
    tutor_persona text DEFAULT 'calido'::text NOT NULL,
    tutor_persona_note text,
    subjects text[] DEFAULT '{}'::text[] NOT NULL,
    engine_profile jsonb,
    engine_profile_at timestamp with time zone,
    CONSTRAINT children_birth_year_check CHECK (((birth_year >= 2000) AND (birth_year <= 2100))),
    CONSTRAINT children_grade_check CHECK ((length(grade) <= 60)),
    CONSTRAINT children_interests_check CHECK ((cardinality(interests) <= 10)),
    CONSTRAINT children_language_check CHECK ((language = ANY (ARRAY['es'::text, 'en'::text]))),
    CONSTRAINT children_nickname_check CHECK (((length(nickname) >= 1) AND (length(nickname) <= 40))),
    CONSTRAINT children_notes_check CHECK ((length(notes) <= 1000)),
    CONSTRAINT children_subjects_check CHECK ((cardinality(subjects) <= 5)),
    CONSTRAINT children_tutor_name_check CHECK (((char_length(tutor_name) >= 1) AND (char_length(tutor_name) <= 20))),
    CONSTRAINT children_tutor_persona_check CHECK ((tutor_persona = ANY (ARRAY['calido'::text, 'jugueton'::text, 'aventurero'::text, 'tranquilo'::text]))),
    CONSTRAINT children_tutor_persona_note_check CHECK (((char_length(tutor_persona_note) >= 1) AND (char_length(tutor_persona_note) <= 200)))
);

CREATE TABLE public.consent_records (
    id uuid NOT NULL,
    family_id uuid NOT NULL,
    child_id uuid NOT NULL,
    guardian_id uuid,
    purpose text NOT NULL,
    granted boolean NOT NULL,
    policy_version text NOT NULL,
    locale text DEFAULT 'es-PE'::text NOT NULL,
    source text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT consent_records_locale_check CHECK ((length(locale) <= 20)),
    CONSTRAINT consent_records_policy_version_check CHECK (((length(policy_version) >= 1) AND (length(policy_version) <= 40))),
    CONSTRAINT consent_records_purpose_check CHECK ((purpose = ANY (ARRAY['voice'::text, 'transcripts'::text, 'memory'::text]))),
    CONSTRAINT consent_records_source_check CHECK (((length(source) >= 1) AND (length(source) <= 40)))
);

CREATE TABLE public.corrections (
    id uuid NOT NULL,
    child_id uuid NOT NULL,
    skill_id text NOT NULL,
    level double precision NOT NULL,
    at timestamp with time zone NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT corrections_level_check CHECK (((level >= (0)::double precision) AND (level <= (1)::double precision)))
);

CREATE TABLE public.device_claims (
    device_id uuid NOT NULL,
    code text NOT NULL,
    challenge text NOT NULL,
    proved_at timestamp with time zone,
    guardian_id uuid,
    child_id uuid,
    expires_at timestamp with time zone NOT NULL
);

CREATE TABLE public.devices (
    id uuid NOT NULL,
    serial text NOT NULL,
    hmac_key bytea NOT NULL,
    model text NOT NULL,
    family_id uuid,
    child_id uuid,
    claimed_at timestamp with time zone,
    firmware_version text,
    last_seen_at timestamp with time zone,
    last_signed_at bigint DEFAULT 0 NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT devices_firmware_version_check CHECK ((length(firmware_version) <= 40)),
    CONSTRAINT devices_hmac_key_check CHECK ((octet_length(hmac_key) = 32)),
    CONSTRAINT devices_model_check CHECK (((length(model) >= 1) AND (length(model) <= 60))),
    CONSTRAINT devices_serial_check CHECK ((serial ~ '^[A-Za-z0-9._-]{4,32}$'::text))
);

CREATE TABLE public.families (
    id uuid NOT NULL,
    name text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT families_name_check CHECK (((length(name) >= 1) AND (length(name) <= 120)))
);

CREATE TABLE public.firmware (
    id uuid NOT NULL,
    model text NOT NULL,
    version text NOT NULL,
    major integer NOT NULL,
    minor integer NOT NULL,
    patch integer NOT NULL,
    url text NOT NULL,
    sha256 text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT firmware_model_check CHECK (((length(model) >= 1) AND (length(model) <= 60))),
    CONSTRAINT firmware_sha256_check CHECK ((sha256 ~ '^[0-9a-f]{64}$'::text)),
    CONSTRAINT firmware_url_check CHECK ((url ~ '^https://'::text)),
    CONSTRAINT firmware_version_check CHECK ((length(version) <= 40))
);

CREATE TABLE public.guardian_children (
    guardian_id uuid NOT NULL,
    child_id uuid NOT NULL,
    relationship text NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT guardian_children_relationship_check CHECK ((relationship = ANY (ARRAY['MTH'::text, 'FTH'::text, 'STPMTH'::text, 'STPFTH'::text, 'GRMTH'::text, 'GRFTH'::text, 'AUNT'::text, 'UNCLE'::text, 'SIB'::text, 'GUARD'::text, 'FAMMEMB'::text])))
);

CREATE TABLE public.guardian_sessions (
    token_hash text NOT NULL,
    guardian_id uuid NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    expires_at timestamp with time zone NOT NULL
);

CREATE TABLE public.guardians (
    id uuid NOT NULL,
    family_id uuid NOT NULL,
    email public.citext NOT NULL,
    name text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    language text DEFAULT 'es'::text NOT NULL,
    notices_by_email boolean DEFAULT true NOT NULL,
    notices_by_push boolean DEFAULT true NOT NULL,
    CONSTRAINT guardians_language_check CHECK ((language = ANY (ARRAY['es'::text, 'en'::text]))),
    CONSTRAINT guardians_name_check CHECK (((length(name) >= 1) AND (length(name) <= 120)))
);

CREATE TABLE public.item_difficulty (
    skill_id text NOT NULL,
    question_id text DEFAULT ''::text NOT NULL,
    value double precision DEFAULT 0 NOT NULL,
    answers integer DEFAULT 0 NOT NULL
);

CREATE TABLE public.jobs (
    id bigint NOT NULL,
    kind text NOT NULL,
    payload jsonb DEFAULT '{}'::jsonb NOT NULL,
    run_at timestamp with time zone DEFAULT now() NOT NULL,
    attempts integer DEFAULT 0 NOT NULL,
    locked_until timestamp with time zone,
    done_at timestamp with time zone,
    last_error text
);

ALTER TABLE public.jobs ALTER COLUMN id ADD GENERATED ALWAYS AS IDENTITY (
    SEQUENCE NAME public.jobs_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);

CREATE TABLE public.login_codes (
    email public.citext NOT NULL,
    code_hash text NOT NULL,
    expires_at timestamp with time zone NOT NULL,
    attempts integer DEFAULT 0 NOT NULL,
    sent_count integer DEFAULT 1 NOT NULL,
    window_started_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE public.memories (
    id uuid NOT NULL,
    child_id uuid NOT NULL,
    session_id uuid,
    kind text NOT NULL,
    content text NOT NULL,
    embedding public.vector(1024),
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT memories_content_check CHECK (((length(content) >= 1) AND (length(content) <= 600))),
    CONSTRAINT memories_kind_check CHECK ((kind = ANY (ARRAY['fact'::text, 'preference'::text, 'struggle'::text, 'success'::text, 'summary'::text])))
);

CREATE TABLE public.notices (
    id uuid NOT NULL,
    family_id uuid NOT NULL,
    child_id uuid,
    session_id uuid,
    kind text NOT NULL,
    title text NOT NULL,
    body text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    read_at timestamp with time zone,
    emailed_at timestamp with time zone,
    pushed_at timestamp with time zone,
    CONSTRAINT notices_body_check CHECK ((length(body) <= 2000)),
    CONSTRAINT notices_kind_check CHECK ((kind = ANY (ARRAY['session_summary'::text, 'attention'::text, 'session_stopped'::text]))),
    CONSTRAINT notices_title_check CHECK ((length(title) <= 200))
);

CREATE TABLE public.outbox (
    id uuid NOT NULL,
    type text NOT NULL,
    subject text,
    data jsonb NOT NULL,
    "time" timestamp with time zone DEFAULT now() NOT NULL,
    attempts integer DEFAULT 0 NOT NULL,
    delivered_at timestamp with time zone,
    last_error text
);

CREATE TABLE public.push_tokens (
    token text NOT NULL,
    guardian_id uuid NOT NULL,
    platform text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    last_seen_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT push_tokens_platform_check CHECK ((platform = ANY (ARRAY['android'::text, 'ios'::text, 'web'::text]))),
    CONSTRAINT push_tokens_token_check CHECK (((length(token) >= 20) AND (length(token) <= 4096)))
);

CREATE TABLE public.session_briefs (
    session_id uuid NOT NULL,
    brief jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    sent_at_start boolean DEFAULT false NOT NULL
);

CREATE TABLE public.session_exercises (
    id bigint NOT NULL,
    session_id uuid NOT NULL,
    skill_id text,
    expression text NOT NULL,
    answer text NOT NULL,
    form text DEFAULT 'any'::text NOT NULL,
    probe boolean NOT NULL,
    attempts integer DEFAULT 0 NOT NULL,
    hints integer DEFAULT 0 NOT NULL,
    status text DEFAULT 'open'::text NOT NULL,
    first_try boolean,
    misconception text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    resolved_at timestamp with time zone,
    answer_said boolean DEFAULT false NOT NULL,
    CONSTRAINT session_exercises_form_check CHECK ((form = ANY (ARRAY['any'::text, 'simplified'::text, 'mixed'::text, 'expanded'::text, 'factored'::text]))),
    CONSTRAINT session_exercises_status_check CHECK ((status = ANY (ARRAY['open'::text, 'correct'::text, 'failed'::text, 'dropped'::text])))
);

ALTER TABLE public.session_exercises ALTER COLUMN id ADD GENERATED ALWAYS AS IDENTITY (
    SEQUENCE NAME public.session_exercises_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);

CREATE TABLE public.session_flags (
    id bigint NOT NULL,
    session_id uuid NOT NULL,
    gate text NOT NULL,
    action text NOT NULL,
    categories text[] NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT session_flags_action_check CHECK ((action = ANY (ARRAY['redirect'::text, 'adult_help'::text, 'clarify'::text, 'support'::text, 'unavailable'::text, 'cooldown'::text]))),
    CONSTRAINT session_flags_gate_check CHECK ((gate = ANY (ARRAY['moderation_input'::text, 'moderation_output'::text, 'physical_safety'::text, 'abuse_limit'::text])))
);

ALTER TABLE public.session_flags ALTER COLUMN id ADD GENERATED ALWAYS AS IDENTITY (
    SEQUENCE NAME public.session_flags_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);

CREATE TABLE public.session_flow_inputs (
    session_id uuid NOT NULL,
    seq integer NOT NULL,
    at_ms bigint NOT NULL,
    input jsonb NOT NULL,
    queued jsonb,
    at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT session_flow_inputs_at_ms_check CHECK ((at_ms >= 0)),
    CONSTRAINT session_flow_inputs_seq_check CHECK ((seq >= 0))
);

CREATE TABLE public.session_guidance (
    id bigint NOT NULL,
    session_id uuid NOT NULL,
    after_seq integer NOT NULL,
    guidance text NOT NULL,
    signals jsonb DEFAULT '{}'::jsonb NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT session_guidance_guidance_check CHECK ((length(guidance) <= 600))
);

ALTER TABLE public.session_guidance ALTER COLUMN id ADD GENERATED ALWAYS AS IDENTITY (
    SEQUENCE NAME public.session_guidance_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);

CREATE TABLE public.session_model_usage (
    session_id uuid NOT NULL,
    model text NOT NULL,
    calls integer DEFAULT 0 NOT NULL,
    input_tokens bigint DEFAULT 0 NOT NULL,
    cache_read_tokens bigint DEFAULT 0 NOT NULL,
    cache_write_tokens bigint DEFAULT 0 NOT NULL,
    output_tokens bigint DEFAULT 0 NOT NULL,
    CONSTRAINT session_model_usage_model_check CHECK (((length(model) >= 1) AND (length(model) <= 80)))
);

CREATE TABLE public.session_reviews (
    session_id uuid NOT NULL,
    child_id uuid,
    guardian_summary text NOT NULL,
    next_step text,
    flags text[] DEFAULT '{}'::text[] NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL
);

CREATE TABLE public.session_turns (
    session_id uuid NOT NULL,
    seq integer NOT NULL,
    role text NOT NULL,
    content text NOT NULL,
    at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT session_turns_content_check CHECK ((length(content) <= 4000)),
    CONSTRAINT session_turns_role_check CHECK ((role = ANY (ARRAY['child'::text, 'tutor'::text]))),
    CONSTRAINT session_turns_seq_check CHECK ((seq >= 0))
);

CREATE TABLE public.sessions (
    id uuid NOT NULL,
    child_id uuid,
    audience text NOT NULL,
    room text NOT NULL,
    language text NOT NULL,
    status text DEFAULT 'created'::text NOT NULL,
    agent_token_hash text NOT NULL,
    keep_transcript boolean DEFAULT false NOT NULL,
    use_memory boolean DEFAULT false NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    started_at timestamp with time zone,
    ended_at timestamp with time zone,
    speaking_seconds integer,
    reviewed_at timestamp with time zone,
    device_id uuid,
    not_trying_streak integer DEFAULT 0 NOT NULL,
    stop_step text,
    child_turns integer,
    tutor_turns integer,
    child_chars integer,
    tutor_chars integer,
    unchecked_exercises integer DEFAULT 0 NOT NULL,
    agent_metrics jsonb,
    breaks integer DEFAULT 0 NOT NULL,
    last_break_at timestamp with time zone,
    pause_offered_at timestamp with time zone,
    tired_at timestamp with time zone,
    objective jsonb,
    flow text,
    flow_start jsonb,
    CONSTRAINT sessions_audience_check CHECK ((audience = ANY (ARRAY['child'::text, 'operator'::text]))),
    CONSTRAINT sessions_flow_check CHECK ((flow = 'engine'::text)),
    CONSTRAINT sessions_language_check CHECK ((language = ANY (ARRAY['es'::text, 'en'::text]))),
    CONSTRAINT sessions_speaking_seconds_check CHECK ((speaking_seconds >= 0)),
    CONSTRAINT sessions_status_check CHECK ((status = ANY (ARRAY['created'::text, 'active'::text, 'ended'::text]))),
    CONSTRAINT sessions_stop_step_check CHECK ((stop_step = ANY (ARRAY['warn'::text, 'end'::text])))
);

CREATE TABLE public.skills (
    id text NOT NULL,
    subject text NOT NULL,
    name text NOT NULL,
    grade text,
    description text,
    prerequisites text[] DEFAULT '{}'::text[] NOT NULL,
    CONSTRAINT skills_description_check CHECK ((length(description) <= 600)),
    CONSTRAINT skills_grade_check CHECK ((length(grade) <= 60)),
    CONSTRAINT skills_id_check CHECK ((id ~ '^[a-z0-9][a-z0-9._-]{0,79}$'::text)),
    CONSTRAINT skills_name_check CHECK (((length(name) >= 1) AND (length(name) <= 160))),
    CONSTRAINT skills_subject_check CHECK (((length(subject) >= 1) AND (length(subject) <= 60)))
);

ALTER TABLE ONLY public.attempts
    ADD CONSTRAINT attempts_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.children
    ADD CONSTRAINT children_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.consent_records
    ADD CONSTRAINT consent_records_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.corrections
    ADD CONSTRAINT corrections_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.device_claims
    ADD CONSTRAINT device_claims_code_key UNIQUE (code);

ALTER TABLE ONLY public.device_claims
    ADD CONSTRAINT device_claims_pkey PRIMARY KEY (device_id);

ALTER TABLE ONLY public.devices
    ADD CONSTRAINT devices_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.devices
    ADD CONSTRAINT devices_serial_key UNIQUE (serial);

ALTER TABLE ONLY public.families
    ADD CONSTRAINT families_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.firmware
    ADD CONSTRAINT firmware_model_version_key UNIQUE (model, version);

ALTER TABLE ONLY public.firmware
    ADD CONSTRAINT firmware_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.guardian_children
    ADD CONSTRAINT guardian_children_pkey PRIMARY KEY (guardian_id, child_id);

ALTER TABLE ONLY public.guardian_sessions
    ADD CONSTRAINT guardian_sessions_pkey PRIMARY KEY (token_hash);

ALTER TABLE ONLY public.guardians
    ADD CONSTRAINT guardians_email_key UNIQUE (email);

ALTER TABLE ONLY public.guardians
    ADD CONSTRAINT guardians_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.item_difficulty
    ADD CONSTRAINT item_difficulty_pkey PRIMARY KEY (skill_id, question_id);

ALTER TABLE ONLY public.jobs
    ADD CONSTRAINT jobs_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.login_codes
    ADD CONSTRAINT login_codes_pkey PRIMARY KEY (email);

ALTER TABLE ONLY public.memories
    ADD CONSTRAINT memories_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.notices
    ADD CONSTRAINT notices_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.notices
    ADD CONSTRAINT notices_session_id_kind_key UNIQUE (session_id, kind);

ALTER TABLE ONLY public.outbox
    ADD CONSTRAINT outbox_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.push_tokens
    ADD CONSTRAINT push_tokens_pkey PRIMARY KEY (token);

ALTER TABLE ONLY public.session_briefs
    ADD CONSTRAINT session_briefs_pkey PRIMARY KEY (session_id);

ALTER TABLE ONLY public.session_exercises
    ADD CONSTRAINT session_exercises_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.session_flags
    ADD CONSTRAINT session_flags_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.session_flow_inputs
    ADD CONSTRAINT session_flow_inputs_pkey PRIMARY KEY (session_id, seq);

ALTER TABLE ONLY public.session_guidance
    ADD CONSTRAINT session_guidance_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.session_model_usage
    ADD CONSTRAINT session_model_usage_pkey PRIMARY KEY (session_id, model);

ALTER TABLE ONLY public.session_reviews
    ADD CONSTRAINT session_reviews_pkey PRIMARY KEY (session_id);

ALTER TABLE ONLY public.session_turns
    ADD CONSTRAINT session_turns_pkey PRIMARY KEY (session_id, seq);

ALTER TABLE ONLY public.sessions
    ADD CONSTRAINT sessions_agent_token_hash_key UNIQUE (agent_token_hash);

ALTER TABLE ONLY public.sessions
    ADD CONSTRAINT sessions_pkey PRIMARY KEY (id);

ALTER TABLE ONLY public.sessions
    ADD CONSTRAINT sessions_room_key UNIQUE (room);

ALTER TABLE ONLY public.skills
    ADD CONSTRAINT skills_pkey PRIMARY KEY (id);

CREATE INDEX attempts_child ON public.attempts USING btree (child_id, at);

CREATE INDEX children_family ON public.children USING btree (family_id);

CREATE INDEX consent_current ON public.consent_records USING btree (child_id, purpose, created_at DESC);

CREATE INDEX corrections_child ON public.corrections USING btree (child_id, at);

CREATE INDEX devices_family ON public.devices USING btree (family_id);

CREATE INDEX firmware_latest ON public.firmware USING btree (model, major DESC, minor DESC, patch DESC);

CREATE INDEX guardian_children_child ON public.guardian_children USING btree (child_id);

CREATE INDEX guardian_sessions_guardian ON public.guardian_sessions USING btree (guardian_id);

CREATE INDEX jobs_due ON public.jobs USING btree (run_at) WHERE (done_at IS NULL);

CREATE INDEX memories_child ON public.memories USING btree (child_id, created_at DESC);

CREATE INDEX memories_embedding ON public.memories USING hnsw (embedding public.vector_cosine_ops);

CREATE INDEX notices_family ON public.notices USING btree (family_id, created_at DESC);

CREATE INDEX outbox_pending ON public.outbox USING btree ("time") WHERE (delivered_at IS NULL);

CREATE INDEX push_tokens_guardian ON public.push_tokens USING btree (guardian_id);

CREATE INDEX session_exercises_session ON public.session_exercises USING btree (session_id, id);

CREATE INDEX session_flags_session ON public.session_flags USING btree (session_id);

CREATE INDEX session_guidance_session ON public.session_guidance USING btree (session_id, id);

CREATE INDEX session_reviews_child ON public.session_reviews USING btree (child_id, created_at DESC);

CREATE INDEX sessions_child ON public.sessions USING btree (child_id, created_at DESC);

CREATE INDEX sessions_open ON public.sessions USING btree (created_at) WHERE (status <> 'ended'::text);

CREATE TRIGGER consent_records_no_change BEFORE DELETE OR UPDATE ON public.consent_records FOR EACH ROW EXECUTE FUNCTION public.consent_records_are_append_only();

ALTER TABLE ONLY public.attempts
    ADD CONSTRAINT attempts_child_id_fkey FOREIGN KEY (child_id) REFERENCES public.children(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.attempts
    ADD CONSTRAINT attempts_session_id_fkey FOREIGN KEY (session_id) REFERENCES public.sessions(id) ON DELETE SET NULL;

ALTER TABLE ONLY public.children
    ADD CONSTRAINT children_family_id_fkey FOREIGN KEY (family_id) REFERENCES public.families(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.corrections
    ADD CONSTRAINT corrections_child_id_fkey FOREIGN KEY (child_id) REFERENCES public.children(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.device_claims
    ADD CONSTRAINT device_claims_child_id_fkey FOREIGN KEY (child_id) REFERENCES public.children(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.device_claims
    ADD CONSTRAINT device_claims_device_id_fkey FOREIGN KEY (device_id) REFERENCES public.devices(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.device_claims
    ADD CONSTRAINT device_claims_guardian_id_fkey FOREIGN KEY (guardian_id) REFERENCES public.guardians(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.devices
    ADD CONSTRAINT devices_child_id_fkey FOREIGN KEY (child_id) REFERENCES public.children(id) ON DELETE SET NULL;

ALTER TABLE ONLY public.devices
    ADD CONSTRAINT devices_family_id_fkey FOREIGN KEY (family_id) REFERENCES public.families(id) ON DELETE SET NULL;

ALTER TABLE ONLY public.guardian_children
    ADD CONSTRAINT guardian_children_child_id_fkey FOREIGN KEY (child_id) REFERENCES public.children(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.guardian_children
    ADD CONSTRAINT guardian_children_guardian_id_fkey FOREIGN KEY (guardian_id) REFERENCES public.guardians(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.guardian_sessions
    ADD CONSTRAINT guardian_sessions_guardian_id_fkey FOREIGN KEY (guardian_id) REFERENCES public.guardians(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.guardians
    ADD CONSTRAINT guardians_family_id_fkey FOREIGN KEY (family_id) REFERENCES public.families(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.memories
    ADD CONSTRAINT memories_child_id_fkey FOREIGN KEY (child_id) REFERENCES public.children(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.memories
    ADD CONSTRAINT memories_session_id_fkey FOREIGN KEY (session_id) REFERENCES public.sessions(id) ON DELETE SET NULL;

ALTER TABLE ONLY public.notices
    ADD CONSTRAINT notices_child_id_fkey FOREIGN KEY (child_id) REFERENCES public.children(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.notices
    ADD CONSTRAINT notices_family_id_fkey FOREIGN KEY (family_id) REFERENCES public.families(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.push_tokens
    ADD CONSTRAINT push_tokens_guardian_id_fkey FOREIGN KEY (guardian_id) REFERENCES public.guardians(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.session_briefs
    ADD CONSTRAINT session_briefs_session_id_fkey FOREIGN KEY (session_id) REFERENCES public.sessions(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.session_exercises
    ADD CONSTRAINT session_exercises_session_id_fkey FOREIGN KEY (session_id) REFERENCES public.sessions(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.session_flags
    ADD CONSTRAINT session_flags_session_id_fkey FOREIGN KEY (session_id) REFERENCES public.sessions(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.session_flow_inputs
    ADD CONSTRAINT session_flow_inputs_session_id_fkey FOREIGN KEY (session_id) REFERENCES public.sessions(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.session_guidance
    ADD CONSTRAINT session_guidance_session_id_fkey FOREIGN KEY (session_id) REFERENCES public.sessions(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.session_model_usage
    ADD CONSTRAINT session_model_usage_session_id_fkey FOREIGN KEY (session_id) REFERENCES public.sessions(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.session_reviews
    ADD CONSTRAINT session_reviews_child_id_fkey FOREIGN KEY (child_id) REFERENCES public.children(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.session_reviews
    ADD CONSTRAINT session_reviews_session_id_fkey FOREIGN KEY (session_id) REFERENCES public.sessions(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.session_turns
    ADD CONSTRAINT session_turns_session_id_fkey FOREIGN KEY (session_id) REFERENCES public.sessions(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.sessions
    ADD CONSTRAINT sessions_child_id_fkey FOREIGN KEY (child_id) REFERENCES public.children(id) ON DELETE CASCADE;

ALTER TABLE ONLY public.sessions
    ADD CONSTRAINT sessions_device_id_fkey FOREIGN KEY (device_id) REFERENCES public.devices(id) ON DELETE SET NULL;
