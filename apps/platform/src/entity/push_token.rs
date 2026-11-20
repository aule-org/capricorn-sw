//! A phone a guardian signed in on, for push notices.

use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "push_tokens")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub token: String,
    pub guardian_id: Uuid,
    pub platform: String,
    pub created_at: DateTimeUtc,
    pub last_seen_at: DateTimeUtc,
    #[sea_orm(belongs_to, from = "guardian_id", to = "id", on_delete = "Cascade")]
    pub guardian: BelongsTo<super::guardian::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
