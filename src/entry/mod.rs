pub mod raw_entry;

use super::money::Money;
use anyhow::{Context, Error, Result};
use chrono::naive::NaiveDate;
use raw_entry::RawEntry;
use rust_decimal::Decimal;
use std::convert::{TryFrom, TryInto};
use std::str::FromStr;

/// This is a fully valid entry.
#[derive(Debug)]
pub struct Entry {
    id: String,
    date: NaiveDate,
    body: EntryBody,
}

#[derive(Debug, Clone)]
pub enum EntryBody {
    PaymentSent(Payment),
    PaymentReceived(Payment),
    PurchaseInvoice(Invoice),
    SaleInvoice(Invoice),
}

impl Entry {
    pub fn id(&self) -> String {
        self.id.clone()
    }
    pub fn date(&self) -> NaiveDate {
        self.date.clone()
    }
    pub fn body(&self) -> EntryBody {
        self.body.clone()
    }
}

impl TryFrom<RawEntry> for Entry {
    type Error = Error;

    fn try_from(raw_entry: RawEntry) -> Result<Self> {
        Ok(Entry {
            id: raw_entry.id.clone().context("Id missing!")?,
            date: raw_entry.date.parse()?,
            body: match raw_entry.r#type.as_ref() {
                "Payment Sent" => Ok(EntryBody::PaymentSent(raw_entry.try_into()?)),
                "Payment Received" => Ok(EntryBody::PaymentReceived(raw_entry.try_into()?)),
                "Purchase Invoice" => Ok(EntryBody::PurchaseInvoice(raw_entry.try_into()?)),
                "Sales Invoice" => Ok(EntryBody::SaleInvoice(raw_entry.try_into()?)),
                _ => Err(Error::msg(format!(
                    "{} not a valid entry type",
                    raw_entry.r#type
                ))),
            }?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Payment {
    pub party: String,
    pub account: String,
    pub memo: Option<String>,
    pub amount: Money,
}

impl TryFrom<RawEntry> for Payment {
    type Error = Error;

    fn try_from(raw_entry: RawEntry) -> Result<Self> {
        let RawEntry {
            id,
            party,
            account,
            memo,
            amount,
            ..
        } = raw_entry;

        Ok(Self {
            party,
            account,
            memo,
            amount: amount
                .context(format!("Amount required for payment entry in {:?}", id))?
                .try_into()?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Invoice {
    pub party: String,
    pub items: Vec<InvoiceItem>,
    pub extras: Option<Vec<InvoiceExtra>>,
    pub payment: Option<InvoicePayment>,
    pub repeat: Option<RecurringPeriod>,
    pub end: Option<NaiveDate>,
}

impl Invoice {
    fn items_try_from_raw_items(
        raw_items: Vec<raw_entry::Item>,
        entry_account: String,
        id: String,
    ) -> Result<Vec<InvoiceItem>> {
        raw_items
            .into_iter()
            .map(|raw_item: raw_entry::Item| {
                let raw_entry::Item {
                    description,
                    code,
                    account,
                    amount,
                    quantity,
                    rate,
                } = raw_item;
                Ok(InvoiceItem {
                    description,
                    code,
                    account: account.unwrap_or(entry_account.clone()),
                    amount: match (quantity, rate, amount) {
                        (Some(quantity), Some(rate), None) => InvoiceItemAmount::ByRate {
                            quantity,
                            rate: rate.try_into()?,
                        },
                        (None, None, Some(amount)) => InvoiceItemAmount::Total(amount.try_into()?),
                        (_, _, _) => Err(Error::msg(format!(
                            "Invoice item must specify either amount \
                                exclusively or rate and quantity in {}",
                            id
                        )))?,
                    },
                })
            })
            .collect()
    }

    fn extras_try_from_raw_extras(
        raw_extras: Option<Vec<raw_entry::Extra>>,
        id: String,
    ) -> Result<Option<Vec<InvoiceExtra>>> {
        raw_extras
            .map(|extras| {
                extras
                    .into_iter()
                    .map(|raw_extra: raw_entry::Extra| {
                        let raw_entry::Extra {
                            description,
                            account,
                            amount,
                            rate,
                        } = raw_extra;
                        Ok(InvoiceExtra {
                            description,
                            account,
                            amount: match (amount, rate) {
                                (Some(amount), None) => {
                                    InvoiceExtraAmount::Total(amount.try_into()?)
                                }
                                (None, Some(rate)) => InvoiceExtraAmount::Rate(rate),
                                (_, _) => Err(Error::msg(format!(
                                    "Invoice extra must specify either amount or rate in {}",
                                    id
                                )))?,
                            },
                        })
                    })
                    .collect()
            })
            .transpose()
    }
}

impl TryFrom<RawEntry> for Invoice {
    type Error = Error;

    fn try_from(raw_entry: RawEntry) -> Result<Self> {
        let RawEntry {
            id,
            party,
            account,
            items,
            extras,
            payment,
            repeat,
            end,
            ..
        } = raw_entry;
        let id = id.context("Id missing!")?;
        Ok(Self {
            party,
            items: Self::items_try_from_raw_items(
                items.context(format!("Items not listed on Invoice {:?}", id))?,
                account,
                id.clone(),
            )?,
            extras: Self::extras_try_from_raw_extras(extras, id)?,
            payment: payment
                .map(|payment| -> Result<InvoicePayment> {
                    Ok(InvoicePayment {
                        account: payment.account,
                        amount: payment.amount.try_into()?,
                    })
                })
                .transpose()?,
            repeat: repeat.and_then(|period_string| match period_string.as_str() {
                "monthly" => Some(RecurringPeriod::Monthly),
                _ => None,
            }),
            end: end.map(|end| end.parse()).transpose()?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct InvoiceItem {
    pub description: Option<String>,
    pub code: Option<String>, // include if tracking item
    pub account: String,
    pub amount: InvoiceItemAmount,
}

impl InvoiceItem {
    pub fn total(&self) -> Result<Money> {
        match self.amount.clone() {
            InvoiceItemAmount::Total(amount) => Ok(amount),
            InvoiceItemAmount::ByRate {
                rate: Money(money),
                quantity,
            } => {
                let quantity: Decimal = quantity.try_into()?;
                let amount = money
                    .checked_mul(quantity)
                    .context("ammount * quantity overflow")?;
                Ok(Money(amount))
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum InvoiceItemAmount {
    Total(Money),
    ByRate { rate: Money, quantity: f64 },
}

#[derive(Debug, Clone)]
pub struct InvoiceExtra {
    description: Option<String>,
    account: String,
    amount: InvoiceExtraAmount,
}

#[derive(Debug, Clone)]
enum InvoiceExtraAmount {
    Total(Money),
    Rate(f64),
    // CumulativeRate(f64),
}

#[derive(Debug, Clone)]
pub struct InvoicePayment {
    pub account: String,
    pub amount: Money,
}

#[derive(Debug, Clone)]
pub enum RecurringPeriod {
    Monthly, //TODO: add more periods
}
