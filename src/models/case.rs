use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Case {
    pub id: Uuid,
    pub name: Option<String>,
    pub date_entered: Option<NaiveDateTime>,
    pub date_modified: Option<NaiveDateTime>,
    pub modified_user_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
    pub description: Option<String>,
    pub deleted: Option<i8>,
    pub assigned_user_id: Option<Uuid>,
    pub case_number: Option<i32>,
    pub r#type: Option<String>,
    pub status: Option<String>,
    pub priority: Option<String>,
    pub resolution: Option<String>,
    pub work_log: Option<String>,
    pub account_id: Option<Uuid>,
}
