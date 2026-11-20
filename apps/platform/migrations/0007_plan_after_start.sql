-- The tutor no longer waits for the plan before it says hello. A plan that
-- was ready at the start travels with the start; one finished later travels
-- with the turns, and this says which.

alter table session_briefs add column sent_at_start boolean not null default false;
