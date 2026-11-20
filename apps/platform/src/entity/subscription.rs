//! A family's plan when it is not the trial: paid every month, or granted by
//! the team. The plan holds from `starts_at` until `ends_at`.

use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "subscriptions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub family_id: Uuid,
    /// `paid` or `granted`.
    pub plan: String,
    pub starts_at: DateTimeUtc,
    /// Paid: moved on by each payment. Granted: the day the team chose.
    pub ends_at: DateTimeUtc,
    /// Paid with Izipay: the card's token and the recurring charge made with it.
    pub payment_token: Option<String>,
    pub provider_subscription: Option<String>,
    pub cancelled_at: Option<DateTimeUtc>,
    /// Granted: who on the team granted it, and why.
    pub granted_by: Option<String>,
    pub note: Option<String>,
    pub created_at: DateTimeUtc,
    #[sea_orm(belongs_to, from = "family_id", to = "id", on_delete = "Cascade")]
    pub family: BelongsTo<super::family::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
