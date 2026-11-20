//! Something the tutor remembers about a child between sessions, with its
//! embedding (bge-m3, 1024 dimensions) for finding it by meaning.

use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "memories")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub child_id: Uuid,
    pub session_id: Option<Uuid>,
    pub kind: String,
    pub content: String,
    #[sea_orm(column_type = "Vector(Some(1024))")]
    pub embedding: Option<PgVector>,
    pub created_at: DateTimeUtc,
    #[sea_orm(belongs_to, from = "child_id", to = "id", on_delete = "Cascade")]
    pub child: BelongsTo<super::child::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
