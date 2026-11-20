//! A skill of the curriculum, with the skills it builds on.

use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "skills")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub subject: String,
    pub name: String,
    pub grade: Option<String>,
    pub description: Option<String>,
    pub prerequisites: Vec<String>,
}

impl ActiveModelBehavior for ActiveModel {}
