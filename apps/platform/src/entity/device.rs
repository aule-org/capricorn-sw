//! A device as it was made: its serial, the key burned into it, and the family
//! and child it is linked to. FHIR R4 `Device`.

use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "devices")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(unique)]
    pub serial: String,
    pub hmac_key: Vec<u8>,
    pub model: String,
    pub family_id: Option<Uuid>,
    pub child_id: Option<Uuid>,
    pub claimed_at: Option<DateTimeUtc>,
    pub firmware_version: Option<String>,
    pub last_seen_at: Option<DateTimeUtc>,
    pub last_signed_at: i64,
    pub created_at: DateTimeUtc,
}

impl ActiveModelBehavior for ActiveModel {}
