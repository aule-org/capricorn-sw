//! An event waiting to be delivered, written in the same transaction as the
//! change it describes.

use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "outbox")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(column_name = "type")]
    pub event_type: String,
    pub subject: Option<String>,
    #[sea_orm(column_type = "JsonBinary")]
    pub data: Json,
    pub time: DateTimeUtc,
    pub attempts: i32,
    pub delivered_at: Option<DateTimeUtc>,
    pub last_error: Option<String>,
}

impl ActiveModelBehavior for ActiveModel {}
