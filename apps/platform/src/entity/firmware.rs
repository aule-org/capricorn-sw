//! A published firmware version for one device model. Never replaced.

use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "firmware")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(unique_key = "model_version")]
    pub model: String,
    #[sea_orm(unique_key = "model_version")]
    pub version: String,
    pub major: i32,
    pub minor: i32,
    pub patch: i32,
    pub url: String,
    pub sha256: String,
    pub created_at: DateTimeUtc,
}

impl ActiveModelBehavior for ActiveModel {}
