use crate::models::{ModuleSchema, ModuleDefinition, field, field_default};

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct Opportunity {
    pub id: Option<String>,
    pub name: Option<String>,
    pub date_entered: Option<String>,
    pub date_modified: Option<String>,
    pub modified_user_id: Option<String>,
    pub created_by: Option<String>,
    pub description: Option<String>,
    pub deleted: Option<i64>,
    pub assigned_user_id: Option<String>,
    pub opportunity_type: Option<String>,
    pub account_id: Option<String>,
    pub campaign_id: Option<String>,
    pub lead_source: Option<String>,
    pub amount: Option<f64>,
    pub amount_usdollar: Option<f64>,
    pub currency_id: Option<String>,
    pub date_closed: Option<String>,
    pub next_step: Option<String>,
    pub sales_stage: Option<String>,
    pub probability: Option<f64>,
}

impl ModuleSchema for Opportunity {
    fn definition() -> ModuleDefinition {
        ModuleDefinition {
            module: "opportunities".into(), label: "Opportunity".into(), label_plural: "Opportunities".into(),
            icon: Some("O".into()), default_view: "board".into(),
            board_field: Some("sales_stage".into()),
            board_stages: Some(vec![
                "Qualification","Needs Analysis","Value Proposition",
                "Proposal/Price Quote","Negotiation/Review","Closed Won","Closed Lost",
            ].into_iter().map(String::from).collect()),
            list_columns: vec!["name","account_id","sales_stage","amount","date_closed"].into_iter().map(String::from).collect(),
            detail_summary: vec!["sales_stage","amount","date_closed"].into_iter().map(String::from).collect(),
            primary_fields: vec!["name","account_id","sales_stage","amount","date_closed","next_step","probability","description"].into_iter().map(String::from).collect(),
            fields: vec![
                field("id","TEXT",0), field("name","TEXT",1),
                field("date_entered","TEXT",2), field("date_modified","TEXT",3),
                field("modified_user_id","TEXT",4), field("created_by","TEXT",5),
                field("description","TEXT",6), field_default("deleted","INTEGER",7,"0"),
                field("assigned_user_id","TEXT",8), field("opportunity_type","TEXT",9),
                field("account_id","TEXT",10), field("campaign_id","TEXT",11),
                field("lead_source","TEXT",12), field("amount","REAL",13),
                field("amount_usdollar","REAL",14), field("currency_id","TEXT",15),
                field("date_closed","TEXT",16), field("next_step","TEXT",17),
                field("sales_stage","TEXT",18), field("probability","REAL",19),
            ],
            scan_mapping: None,
            indexes: vec!["name".into(),"sales_stage".into(),"account_id".into(),"deleted".into()],
        }
    }
}
