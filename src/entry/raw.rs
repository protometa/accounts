use crate::money::Money;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::collections::HashMap;

#[derive(Debug, Deserialize, PartialEq, Clone, Serialize)]
#[serde(untagged)]
pub enum Lines {
    Simple(HashMap<String, Money>),
    Expanded(Vec<ExpandedLine>),
}

#[derive(Debug, Deserialize, PartialEq, Clone, Serialize)]
pub struct ExpandedLine {
    pub account: String,
    pub amount: Money,
}

/// Raw struct deserilized from yaml
#[skip_serializing_none]
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, Default)]
pub struct Entry {
    pub id: Option<String>, // if not specified will use filename
    pub r#type: Option<String>,
    pub date: String,
    pub memo: Option<String>,
    pub credits: Option<Lines>,
    pub debits: Option<Lines>,
    pub party: Option<String>,
    pub account: Option<String>,
    pub amount: Option<Money>,
    pub items: Option<Items>,
    pub extras: Option<Vec<Extra>>,
    pub payment: Option<Payment>,
    pub repeat: Option<String>,
    pub end: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq, Clone, Serialize)]
#[serde(untagged)]
pub enum Items {
    Simple(HashMap<String, Money>),
    Expanded(Vec<Item>),
}

impl Items {
    pub fn as_expanded(&self) -> Vec<Item> {
        match self {
            Self::Simple(lines) => lines
                .into_iter()
                .map(|(description, amount)| Item {
                    description: Some(description.to_owned()),
                    amount: Some(amount.to_owned()),
                    ..Default::default()
                })
                .collect(),
            Self::Expanded(items) => items.to_owned(),
        }
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, Default)]
pub struct Item {
    pub description: Option<String>,
    pub code: Option<String>,    // include if tracking
    pub account: Option<String>, // include if specific override to default above
    pub amount: Option<Money>,   // specify either ammount here or quantity and rate below
    pub quantity: Option<f64>,
    pub rate: Option<f64>,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct Extra {
    pub description: Option<String>,
    pub account: String,
    pub amount: Option<Money>,
    pub rate: Option<f64>,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct Payment {
    pub account: String,
    pub amount: Money,
}
