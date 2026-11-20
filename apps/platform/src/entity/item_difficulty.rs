//! The calibrated difficulty of a skill (`question_id` empty) or of one of its
//! questions, in logits, and how many answers have moved it.

use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "item_difficulty")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub skill_id: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub question_id: String,
    pub value: f64,
    pub answers: i32,
}

impl ActiveModelBehavior for ActiveModel {}
