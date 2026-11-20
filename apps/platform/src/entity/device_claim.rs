//! A device waiting to be linked: the code it shows, the challenge it signs,
//! and the guardian who typed the code.

use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "device_claims")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub device_id: Uuid,
    #[sea_orm(unique)]
    pub code: String,
    pub challenge: String,
    pub proved_at: Option<DateTimeUtc>,
    pub guardian_id: Option<Uuid>,
    pub child_id: Option<Uuid>,
    pub expires_at: DateTimeUtc,
    #[sea_orm(belongs_to, from = "device_id", to = "id", on_delete = "Cascade")]
    pub device: BelongsTo<super::device::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
