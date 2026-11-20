//! One answer of a child to a skill or question. The learner model is a replay
//! of these.

use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "attempts")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub child_id: Uuid,
    pub session_id: Option<Uuid>,
    pub skill_id: String,
    pub question_id: Option<String>,
    pub correct: bool,
    pub credit: Option<f64>,
    pub choice: bool,
    pub difficulty: Option<f64>,
    pub source: String,
    pub at: DateTimeUtc,
    pub created_at: DateTimeUtc,
    #[sea_orm(belongs_to, from = "child_id", to = "id", on_delete = "Cascade")]
    pub child: BelongsTo<super::child::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
