//! An exercise the tutor posed in a session: what it asked, the answer worked
//! out here, and how the student got on. The answer never reaches the
//! student's screen.

use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "session_exercises")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub session_id: Uuid,
    pub skill_id: Option<String>,
    pub expression: String,
    pub answer: String,
    pub form: String,
    pub probe: bool,
    pub attempts: i32,
    pub hints: i32,
    pub status: String,
    pub first_try: Option<bool>,
    pub misconception: Option<String>,
    pub created_at: DateTimeUtc,
    pub resolved_at: Option<DateTimeUtc>,
    pub answer_said: bool,
    #[sea_orm(belongs_to, from = "session_id", to = "id", on_delete = "Cascade")]
    pub session: BelongsTo<super::session::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
