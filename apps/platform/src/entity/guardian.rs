//! An adult of a family, signed in by email. FHIR R4 `RelatedPerson`, one per
//! child they are related to.

use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "guardians")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub family_id: Uuid,
    #[sea_orm(column_type = "custom(\"citext\")", unique)]
    pub email: String,
    pub name: String,
    pub created_at: DateTimeUtc,
    pub language: String,
    pub notices_by_email: bool,
    pub notices_by_push: bool,
    #[sea_orm(belongs_to, from = "family_id", to = "id", on_delete = "Cascade")]
    pub family: BelongsTo<super::family::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
