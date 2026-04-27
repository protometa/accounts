use crate::money::Money;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::collections::HashMap;

#[derive(Debug, Deserialize, PartialEq, Clone, Serialize, Default, JsonSchema)]
#[serde(untagged)]
pub enum Lines {
    #[default]
    Empty,
    Simple(HashMap<String, Money>),
    Expanded(Vec<ExpandedLine>),
}

#[derive(Debug, Deserialize, PartialEq, Clone, Serialize, JsonSchema)]
pub struct ExpandedLine {
    pub account: String,
    pub amount: Money,
}

#[derive(Debug, Default, Deserialize, PartialEq, Clone, Serialize, JsonSchema)]
pub enum JournalEntryType {
    #[default]
    #[serde(rename = "Journal Entry")]
    JournalEntry,
}

#[derive(Debug, Deserialize, PartialEq, Clone, Serialize, Default, JsonSchema)]
pub enum InvoiceEntryType {
    #[default]
    #[serde(rename = "Purchase Invoice")]
    PurchaseInvoice,
    #[serde(rename = "Sales Invoice")]
    SalesInvoice,
}

#[derive(Debug, Deserialize, PartialEq, Clone, Serialize, Default, JsonSchema)]
pub enum PaymentEntryType {
    #[default]
    #[serde(rename = "Payment Sent")]
    PaymentSent,
    #[serde(rename = "Payment Received")]
    PaymentReceived,
}

/// Raw struct deserilized from yaml
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(untagged)]
pub enum Entry {
    JournalEntry(JournalEntry),
    PaymentEntry(PaymentEntry),
    InvoiceEntry(InvoiceEntry),
}

/// Raw struct deserilized from yaml
#[skip_serializing_none]
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, Default, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JournalEntry {
    pub id: Option<String>, // if not specified will use filename TODO needs more work
    pub date: String,
    pub r#type: Option<JournalEntryType>,
    pub memo: Option<String>,
    pub debits: Lines,
    pub credits: Lines,
}

/// Raw struct deserilized from yaml
#[skip_serializing_none]
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, Default, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PaymentEntry {
    pub id: Option<String>, // if not specified will use filename TODO needs more work
    pub date: String,
    pub r#type: PaymentEntryType,
    pub memo: Option<String>,
    pub party: String,
    pub account: String,
    pub amount: Money,
}

/// Raw struct deserilized from yaml
#[skip_serializing_none]
#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, Default, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InvoiceEntry {
    pub id: Option<String>, // if not specified will use filename TODO needs more work
    pub date: String,
    pub r#type: InvoiceEntryType,
    pub memo: Option<String>,
    pub party: String,
    pub account: String,
    pub amount: Option<Money>,
    pub items: Option<Items>,
    pub extras: Option<Vec<Extra>>,
    pub payment: Option<Payment>,
    pub repeat: Option<String>,
    pub end: Option<String>,
}

impl Entry {
    // TODO should be able to used derived methods somehow
    pub fn type_str(&self) -> String {
        match self {
            Self::JournalEntry(_) => "Journal Entry".to_string(),
            Self::PaymentEntry(x) => match x.r#type {
                PaymentEntryType::PaymentSent => "Payment Sent".to_string(),
                PaymentEntryType::PaymentReceived => "Payment Received".to_string(),
            },
            Self::InvoiceEntry(x) => match x.r#type {
                InvoiceEntryType::PurchaseInvoice => "Purchase Invoice".to_string(),
                InvoiceEntryType::SalesInvoice => "Sales Invoice".to_string(),
            },
        }
    }
    pub fn id(&self) -> Option<String> {
        match self {
            Self::JournalEntry(x) => x.id.clone(),
            Self::PaymentEntry(x) => x.id.clone(),
            Self::InvoiceEntry(x) => x.id.clone(),
        }
    }
    pub fn set_id(&mut self, id: impl Into<String>) -> &Self {
        match self {
            Self::JournalEntry(x) => x.id.insert(id.into()),
            Self::PaymentEntry(x) => x.id.insert(id.into()),
            Self::InvoiceEntry(x) => x.id.insert(id.into()),
        };
        self
    }
    pub fn date(&self) -> String {
        match self {
            Self::JournalEntry(x) => x.date.clone(),
            Self::PaymentEntry(x) => x.date.clone(),
            Self::InvoiceEntry(x) => x.date.clone(),
        }
    }
    pub fn memo(&self) -> Option<String> {
        match self {
            Self::JournalEntry(x) => x.memo.clone(),
            Self::PaymentEntry(x) => x.memo.clone(),
            Self::InvoiceEntry(x) => x.memo.clone(),
        }
    }
    // TODO handle this intrinsically when parsed as invoice
    pub fn end(&self) -> Option<String> {
        match self {
            Self::JournalEntry(_) => None,
            Self::PaymentEntry(_) => None,
            Self::InvoiceEntry(x) => x.end.clone(),
        }
    }
    // TODO handle this intrinsically when parsed as invoice
    pub fn repeat(&self) -> Option<String> {
        match self {
            Self::JournalEntry(_) => None,
            Self::PaymentEntry(_) => None,
            Self::InvoiceEntry(x) => x.repeat.clone(),
        }
    }
}

#[derive(Debug, Deserialize, PartialEq, Clone, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Items {
    Simple(HashMap<String, Money>),
    Expanded(Vec<Item>),
}

impl Items {
    pub fn as_expanded(&self) -> Vec<Item> {
        match self {
            Self::Simple(lines) => lines
                .iter()
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

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct Item {
    pub description: Option<String>,
    pub code: Option<String>,    // include if tracking
    pub account: Option<String>, // include if specific override to default above
    pub amount: Option<Money>,   // specify either ammount here or quantity and rate below
    pub quantity: Option<f64>,
    pub rate: Option<Money>,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Extra {
    pub description: Option<String>,
    pub account: String,
    pub amount: Option<Money>,
    pub rate: Option<f64>,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Payment {
    pub account: String,
    pub amount: Money,
}
