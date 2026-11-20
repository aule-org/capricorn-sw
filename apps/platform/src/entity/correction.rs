//! "They already know this": a guardian or the team setting a skill's level.

use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "corrections")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub child_id: Uuid,
    pub skill_id: String,
    pub level: f64,
    pub at: DateTimeUtc,
    pub created_at: DateTimeUtc,
    #[sea_orm(belongs_to, from = "child_id", to = "id", on_delete = "Cascade")]
    pub child: BelongsTo<super::child::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
