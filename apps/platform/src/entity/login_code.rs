//! The code emailed to an address to sign in, stored as a hash, with how many
//! were sent this hour and how many wrong tries it has had.

use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "login_codes")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false, column_type = "custom(\"citext\")")]
    pub email: String,
    pub code_hash: String,
    pub expires_at: DateTimeUtc,
    pub attempts: i32,
    pub sent_count: i32,
    pub window_started_at: DateTimeUtc,
}

impl ActiveModelBehavior for ActiveModel {}
