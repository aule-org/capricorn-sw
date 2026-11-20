-- Algebra asks for an answer written expanded or as a product.
alter table session_exercises drop constraint session_exercises_form_check;
alter table session_exercises add constraint session_exercises_form_check
    check (form in ('any', 'simplified', 'mixed', 'expanded', 'factored'));
