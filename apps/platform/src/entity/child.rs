//! A child: what the tutor is allowed to know, and what the session engine
//! keeps of them between sessions. FHIR R4 `Patient`.

use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "children")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub family_id: Uuid,
    pub nickname: String,
    pub birth_year: Option<i32>,
    pub grade: Option<String>,
    pub language: String,
    pub interests: Vec<String>,
    pub subjects: Vec<String>,
    pub notes: Option<String>,
    pub tutor_name: String,
    pub tutor_persona: String,
    pub tutor_persona_note: Option<String>,
    pub created_at: DateTimeUtc,
    /// The engine's own record of the child, from the latest session it ran.
    #[sea_orm(column_type = "JsonBinary")]
    pub engine_profile: Option<Json>,
    pub engine_profile_at: Option<DateTimeUtc>,
    #[sea_orm(belongs_to, from = "family_id", to = "id", on_delete = "Cascade")]
    pub family: BelongsTo<super::family::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
