//! Advice the supervisor wrote while a session ran, for the tutor to collect.

use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "session_guidance")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub session_id: Uuid,
    pub after_seq: i32,
    pub guidance: String,
    #[sea_orm(column_type = "JsonBinary")]
    pub signals: Json,
    pub created_at: DateTimeUtc,
    #[sea_orm(belongs_to, from = "session_id", to = "id", on_delete = "Cascade")]
    pub session: BelongsTo<super::session::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
