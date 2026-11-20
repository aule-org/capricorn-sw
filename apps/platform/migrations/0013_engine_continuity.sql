-- What the session engine carries from one session of a child to the next:
-- the level reached in each topic, a screening left half way, how long each
-- level took and which exercises were already used. Levels and ids, never
-- anything the student said.
alter table children
    add column engine_profile jsonb,
    -- when the session it comes from was created, so an older session that
    -- is reviewed late does not replace a newer one's
    add column engine_profile_at timestamptz;

-- What an engine session started from: that profile and the session length,
-- as they were when it was created. A session is rebuilt from this and its
-- inputs, so neither may change under it.
alter table sessions add column flow_start jsonb;

-- Exercises the engine graded are evidence for the learner model too.
alter table attempts drop constraint attempts_source_check;
alter table attempts add constraint attempts_source_check
    check (source in ('review', 'tutor', 'guardian', 'import', 'engine'));
