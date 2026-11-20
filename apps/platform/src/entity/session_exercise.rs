//! An exercise the tutor posed in a session. Only the columns the session
//! code reads are here; the exercise checker keeps the rest until it moves.

use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "session_exercises")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub session_id: Uuid,
    pub status: String,
    pub resolved_at: Option<DateTimeUtc>,
    #[sea_orm(belongs_to, from = "session_id", to = "id", on_delete = "Cascade")]
    pub session: BelongsTo<super::session::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
