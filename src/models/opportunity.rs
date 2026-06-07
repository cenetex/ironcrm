use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Opportunity {
    pub id: Uuid,
    pub name: Option<String>,
    pub date_entered: Option<NaiveDateTime>,
    pub date_modified: Option<NaiveDateTime>,
    pub modified_user_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
    pub description: Option<String>,
    pub deleted: Option<i8>,
    pub assigned_user_id: Option<Uuid>,
    pub opportunity_type: Option<String>,
    pub account_id: Option<Uuid>,
    pub campaign_id: Option<Uuid>,
    pub lead_source: Option<String>,
    pub amount: Option<f64>,
    pub amount_usdollar: Option<f64>,
    pub currency_id: Option<Uuid>,
    pub date_closed: Option<NaiveDateTime>,
    pub next_step: Option<String>,
    pub sales_stage: Option<String>,
    pub probability: Option<f64>,
}
