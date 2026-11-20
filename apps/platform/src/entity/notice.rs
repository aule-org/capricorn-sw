//! Something the family is told: a session's review, a stop, a safety note.
//! Shown in the app, and sent by email and push as the guardian chose.

use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "notices")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub family_id: Uuid,
    pub child_id: Option<Uuid>,
    #[sea_orm(unique_key = "session_kind")]
    pub session_id: Option<Uuid>,
    #[sea_orm(unique_key = "session_kind")]
    pub kind: String,
    pub title: String,
    pub body: String,
    pub created_at: DateTimeUtc,
    pub read_at: Option<DateTimeUtc>,
    pub emailed_at: Option<DateTimeUtc>,
    pub pushed_at: Option<DateTimeUtc>,
    #[sea_orm(belongs_to, from = "family_id", to = "id", on_delete = "Cascade")]
    pub family: BelongsTo<super::family::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
