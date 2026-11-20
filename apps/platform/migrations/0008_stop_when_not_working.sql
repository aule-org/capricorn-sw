-- A student who is plainly not working gets one warning and then the session
-- closes, and the family is told. The count is of turns in a row; anything
-- else the student says starts it over.

alter table sessions
    add column not_trying_streak integer not null default 0,
    add column stop_step text check (stop_step in ('warn', 'end'));

alter table notices drop constraint notices_kind_check;
alter table notices
    add constraint notices_kind_check check (kind in ('session_summary', 'attention', 'session_stopped'));
