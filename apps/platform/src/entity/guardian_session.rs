//! A signed-in guardian's session, by the hash of its token.

use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "guardian_sessions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub token_hash: String,
    pub guardian_id: Uuid,
    pub created_at: DateTimeUtc,
    pub expires_at: DateTimeUtc,
    #[sea_orm(belongs_to, from = "guardian_id", to = "id", on_delete = "Cascade")]
    pub guardian: BelongsTo<super::guardian::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
