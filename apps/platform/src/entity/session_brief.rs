//! The supervisor's plan for a session.

use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "session_briefs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub session_id: Uuid,
    #[sea_orm(column_type = "JsonBinary")]
    pub brief: Json,
    pub created_at: DateTimeUtc,
    pub sent_at_start: bool,
    #[sea_orm(belongs_to, from = "session_id", to = "id", on_delete = "Cascade")]
    pub session: BelongsTo<super::session::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
