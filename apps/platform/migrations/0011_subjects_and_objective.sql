-- What the family wants: the subjects to reinforce for each student, and, for
-- one session, what it is for (homework, an exam to prepare...). The topic of
-- an objective is the guardian's own words.

alter table children
    add column subjects text[] not null default '{}' check (cardinality(subjects) <= 5);

alter table sessions add column objective jsonb;
