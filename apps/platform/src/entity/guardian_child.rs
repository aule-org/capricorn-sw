//! What a guardian is to a child: FHIR R4 `RelatedPerson.relationship`, a code
//! of HL7 v3 RoleCode. No row: the guardian has not said.

use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "guardian_children")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub guardian_id: Uuid,
    #[sea_orm(primary_key, auto_increment = false)]
    pub child_id: Uuid,
    pub relationship: String,
    pub updated_at: DateTimeUtc,
    #[sea_orm(belongs_to, from = "guardian_id", to = "id", on_delete = "Cascade")]
    pub guardian: BelongsTo<super::guardian::Entity>,
    #[sea_orm(belongs_to, from = "child_id", to = "id", on_delete = "Cascade")]
    pub child: BelongsTo<super::child::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
