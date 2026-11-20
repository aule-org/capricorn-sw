//! One answer of a guardian about one purpose. Append-only: the current answer
//! is the latest row. Rows outlive the child, as proof of what was agreed.
//! FHIR R4 `Consent`.

use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "consent_records")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub family_id: Uuid,
    pub child_id: Uuid,
    pub guardian_id: Option<Uuid>,
    pub purpose: String,
    pub granted: bool,
    pub policy_version: String,
    pub locale: String,
    pub source: String,
    pub created_at: DateTimeUtc,
}

impl ActiveModelBehavior for ActiveModel {}
