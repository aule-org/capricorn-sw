//! Work for the worker process, committed with the change that caused it.

use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "jobs")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub kind: String,
    #[sea_orm(column_type = "JsonBinary")]
    pub payload: Json,
    pub run_at: DateTimeUtc,
    pub attempts: i32,
    pub locked_until: Option<DateTimeUtc>,
    pub done_at: Option<DateTimeUtc>,
    pub last_error: Option<String>,
}

impl ActiveModelBehavior for ActiveModel {}
