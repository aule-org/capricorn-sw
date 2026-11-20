//! A safety gate stopped a turn: which gate, what it did, fixed categories.
//! Never what was said.

use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "session_flags")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub session_id: Uuid,
    pub gate: String,
    pub action: String,
    pub categories: Vec<String>,
    pub created_at: DateTimeUtc,
    #[sea_orm(belongs_to, from = "session_id", to = "id", on_delete = "Cascade")]
    pub session: BelongsTo<super::session::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
