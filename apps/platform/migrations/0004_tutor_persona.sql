-- How the tutor presents itself to each child: the name the family gave it,
-- one of a few fixed manners, and an optional line from the guardian.

alter table children
    add column tutor_name text not null default 'Aule'
        check (char_length(tutor_name) between 1 and 20),
    add column tutor_persona text not null default 'calido'
        check (tutor_persona in ('calido', 'jugueton', 'aventurero', 'tranquilo')),
    add column tutor_persona_note text
        check (char_length(tutor_persona_note) between 1 and 200);
