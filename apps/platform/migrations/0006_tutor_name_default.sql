-- A database created before the tutor's default name changed keeps the old
-- default on the column; this brings it in line with 0004 as it reads now.

alter table children alter column tutor_name set default 'Aule';
