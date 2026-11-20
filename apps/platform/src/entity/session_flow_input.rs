//! One input the session engine applied, with its clock, so the engine can be
//! rebuilt in any process. What the student said: deleted with the turns.

use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "session_flow_inputs")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub session_id: Uuid,
    #[sea_orm(primary_key, auto_increment = false)]
    pub seq: i32,
    pub at_ms: i64,
    #[sea_orm(column_type = "JsonBinary")]
    pub input: Json,
    /// Lines the tutor still has to be handed, kept with the last input.
    #[sea_orm(column_type = "JsonBinary")]
    pub queued: Option<Json>,
    pub at: DateTimeUtc,
    #[sea_orm(belongs_to, from = "session_id", to = "id", on_delete = "Cascade")]
    pub session: BelongsTo<super::session::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
