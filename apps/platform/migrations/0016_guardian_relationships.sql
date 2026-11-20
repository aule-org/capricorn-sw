-- How a guardian is related to each child of their family, as FHIR R4's
-- RelatedPerson.relationship: a code from HL7 v3 RoleCode, in the
-- PatientRelationshipType value set. No row means the guardian has not said.
-- Who may see a child does not depend on it: every guardian of the family may.
create table guardian_children (
    guardian_id uuid not null references guardians (id) on delete cascade,
    child_id uuid not null references children (id) on delete cascade,
    relationship text not null check (relationship in (
        'MTH', 'FTH', 'STPMTH', 'STPFTH', 'GRMTH', 'GRFTH', 'AUNT', 'UNCLE', 'SIB', 'GUARD', 'FAMMEMB'
    )),
    updated_at timestamptz not null default now(),
    primary key (guardian_id, child_id)
);
create index guardian_children_child on guardian_children (child_id);
