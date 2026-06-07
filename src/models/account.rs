use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Account {
    pub id: Uuid,
    pub name: Option<String>,
    pub date_entered: Option<NaiveDateTime>,
    pub date_modified: Option<NaiveDateTime>,
    pub modified_user_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
    pub description: Option<String>,
    pub deleted: Option<i8>,
    pub assigned_user_id: Option<Uuid>,
    pub account_type: Option<String>,
    pub industry: Option<String>,
    pub annual_revenue: Option<String>,
    pub phone_fax: Option<String>,
    pub billing_address_street: Option<String>,
    pub billing_address_city: Option<String>,
    pub billing_address_state: Option<String>,
    pub billing_address_postalcode: Option<String>,
    pub billing_address_country: Option<String>,
    pub rating: Option<String>,
    pub phone_office: Option<String>,
    pub phone_alternate: Option<String>,
    pub website: Option<String>,
    pub ownership: Option<String>,
    pub employees: Option<String>,
    pub ticker_symbol: Option<String>,
    pub shipping_address_street: Option<String>,
    pub shipping_address_city: Option<String>,
    pub shipping_address_state: Option<String>,
    pub shipping_address_postalcode: Option<String>,
    pub shipping_address_country: Option<String>,
    pub email1: Option<String>,
    pub email2: Option<String>,
    pub parent_id: Option<Uuid>,
}
