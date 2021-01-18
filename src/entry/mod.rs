pub mod raw_entry;

use super::money::Money;
use anyhow::{Context, Error, Result};
use chrono::naive::NaiveDate;
use raw_entry::RawEntry;
use std::convert::{TryFrom, TryInto};

/// This is a fully valid entry.
#[derive(Debug)]
pub enum Entry {
    PaymentSent(Payment),
    PaymentReceived(Payment),
    PurchaseInvoice(Invoice),
    SaleInvoice(Invoice),
}

impl TryFrom<RawEntry> for Entry {
    type Error = Error;

    fn try_from(raw_entry: RawEntry) -> Result<Self> {
        match raw_entry.r#type.as_ref() {
            "Payment Sent" => Ok(Entry::PaymentSent(raw_entry.try_into()?)),
            "Payment Received" => Ok(Entry::PaymentReceived(raw_entry.try_into()?)),
            "Purchase Invoice" => Ok(Entry::PurchaseInvoice(raw_entry.try_into()?)),
            "Sales Invoice" => Ok(Entry::SaleInvoice(raw_entry.try_into()?)),
            _ => Err(Error::msg(format!(
                "{} not a valid entry type",
                raw_entry.r#type
            ))),
        }
    }
}

#[derive(Debug)]
pub struct Payment {
    id: String,
    date: NaiveDate,
    party: String,
    account: String,
    memo: Option<String>,
    amount: Money,
}

impl TryFrom<RawEntry> for Payment {
    type Error = Error;

    fn try_from(raw_entry: RawEntry) -> Result<Self> {
        let RawEntry {
            id,
            date,
            party,
            account,
            memo,
            amount,
            ..
        } = raw_entry;
        let id = id.context("Id missing!")?;
        Ok(Self {
            id: id.clone(),
            date: date.parse()?,
            party,
            account,
            memo,
            amount: amount
                .context(format!("Amount required for payment entry in {}", id))?
                .try_into()?,
        })
    }
}

#[derive(Debug)]
pub struct Invoice {
    id: String,
    date: NaiveDate,
    party: String,
    account: String, // default account for item expenses (items may include their own account to override)
    items: Vec<InvoiceItem>,
    extras: Option<Vec<InvoiceExtra>>,
    payment: Option<InvoicePayment>,
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
            date,
            party,
            account,
            items,
            extras,
            payment,
            ..
        } = raw_entry;
        let id = id.context("Id missing!")?;
        Ok(Self {
            id: id.clone(),
            date: date.parse()?,
            party,
            account: account.clone(),
            items: Self::items_try_from_raw_items(
                items.context(format!("Items not listed on Invoice {}", id))?,
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
        })
    }
}

#[derive(Debug)]
struct InvoiceItem {
    description: Option<String>,
    code: Option<String>, // include if tracking item
    account: String,
    amount: InvoiceItemAmount,
}

#[derive(Debug)]
enum InvoiceItemAmount {
    Total(Money),
    ByRate { rate: Money, quantity: f64 },
}

#[derive(Debug)]
struct InvoiceExtra {
    description: Option<String>,
    account: String,
    amount: InvoiceExtraAmount,
}

#[derive(Debug)]
enum InvoiceExtraAmount {
    Total(Money),
    Rate(f64),
    // CumulativeRate(f64),
}

#[derive(Debug)]
struct InvoicePayment {
    account: String,
    amount: Money,
}
