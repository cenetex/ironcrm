use crate::models::{ModuleSchema, ModuleDefinition, field, field_default};

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct Case {
    pub id: Option<String>,
    pub name: Option<String>,
    pub date_entered: Option<String>,
    pub date_modified: Option<String>,
    pub modified_user_id: Option<String>,
    pub created_by: Option<String>,
    pub description: Option<String>,
    pub deleted: Option<i64>,
    pub assigned_user_id: Option<String>,
    pub case_number: Option<i64>,
    pub r#type: Option<String>,
    pub status: Option<String>,
    pub priority: Option<String>,
    pub resolution: Option<String>,
    pub work_log: Option<String>,
    pub account_id: Option<String>,
}

impl ModuleSchema for Case {
    fn definition() -> ModuleDefinition {
        ModuleDefinition {
            module: "cases".into(), label: "Case".into(), label_plural: "Cases".into(),
            icon: Some("K".into()), default_view: "table".into(),
            board_field: None, board_stages: None,
            list_columns: vec!["name","account_id","priority","status","type"].into_iter().map(String::from).collect(),
            detail_summary: vec!["priority","status","type"].into_iter().map(String::from).collect(),
            primary_fields: vec!["name","account_id","priority","status","type","resolution","work_log","description"].into_iter().map(String::from).collect(),
            fields: vec![
                field("id","TEXT",0), field("name","TEXT",1),
                field("date_entered","TEXT",2), field("date_modified","TEXT",3),
                field("modified_user_id","TEXT",4), field("created_by","TEXT",5),
                field("description","TEXT",6), field_default("deleted","INTEGER",7,"0"),
                field("assigned_user_id","TEXT",8), field("case_number","INTEGER",9),
                field("type","TEXT",10), field("status","TEXT",11),
                field("priority","TEXT",12), field("resolution","TEXT",13),
                field("work_log","TEXT",14), field("account_id","TEXT",15),
            ],
            scan_mapping: None,
            indexes: vec!["name".into(),"status".into(),"deleted".into()],
        }
    }
}
