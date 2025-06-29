use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Raw struct deserilized from yaml
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: Option<String>, // if not specified will use filename
    pub r#type: Option<String>,
    pub date: String,
    pub memo: Option<String>,
    pub debits: Option<HashMap<String, String>>,
    pub credits: Option<HashMap<String, String>>,
    pub party: Option<String>,
    pub account: Option<String>,
    pub amount: Option<f64>,
    pub items: Option<Vec<Item>>,
    pub extras: Option<Vec<Extra>>,
    pub payment: Option<Payment>,
    pub repeat: Option<String>,
    pub end: Option<String>,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct Item {
    pub description: Option<String>,
    pub code: Option<String>,    // include if tracking
    pub account: Option<String>, // include if specific override to default above
    pub amount: Option<f64>,     // specify either ammount here or quantity and rate below
    pub quantity: Option<f64>,
    pub rate: Option<f64>,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct Extra {
    pub description: Option<String>,
    pub account: String,
    pub amount: Option<f64>,
    pub rate: Option<f64>,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct Payment {
    pub account: String,
    pub amount: f64,
}
