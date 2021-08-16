pub mod raw_entry;

use super::money::Money;
use anyhow::{Context, Error, Result};
use chrono::prelude::*;
use chrono_tz::UTC;
use raw_entry::RawEntry;
use rrule::{Frequenzy, Options, RRule};
use rust_decimal::Decimal;
use std::convert::{TryFrom, TryInto};
use std::iter::{self, Iterator};

/// This is a fully valid entry.
#[derive(Debug)]
pub struct Entry {
    id: String,
    date: EntryDate,
    body: EntryBody,
}

#[derive(Debug)]
enum EntryDate {
    SingleDate(NaiveDate),
    RRule(RRule),
}

impl EntryDate {
    fn iter(&self) -> Box<dyn Iterator<Item = NaiveDate> + '_> {
        match self {
            EntryDate::SingleDate(date) => Box::new(iter::once(date.clone())),
            EntryDate::RRule(rrule) => Box::new(rrule.into_iter().map(|d| d.date().naive_local())),
        }
    }
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
    pub fn dates(&self, until: NaiveDate) -> impl Iterator<Item = NaiveDate> + '_ {
        self.date.iter().take_while(move |d| *d <= until)
    }
    pub fn body(&self) -> EntryBody {
        self.body.clone()
    }
}

impl TryFrom<RawEntry> for Entry {
    type Error = Error;

    fn try_from(raw_entry: RawEntry) -> Result<Self> {
        let RawEntry {
            id,
            date,
            r#type,
            repeat,
            end,
            ..
        } = raw_entry.clone();
        let date: NaiveDate = date.parse()?;
        let end: Option<NaiveDate> = end.map(|s| s.parse()).transpose()?;
        Ok(Entry {
            id: id.clone().context("Id missing!")?,
            // `date` is single date unless `repeat` is specified then becomes rrule
            // rrule is parsed from optional `repeat` and `end` fields
            // treating string 'monthly' as generic monthly rrule
            date: repeat.map_or::<Result<_>, _>(Ok(EntryDate::SingleDate(date)), |rule_str| {
                let ed = match rule_str.to_uppercase().as_str() {
                    // if simply MONTHLY use basic monthy rrule
                    "MONTHLY" => end
                        .map_or(default_monthly_rrule(date), |end| {
                            default_monthly_rrule(date).until(
                                Local
                                    .ymd(end.year(), end.month(), end.day())
                                    .and_hms(0, 0, 0)
                                    .with_timezone(&Utc),
                            )
                        })
                        .build()
                        .map(RRule::new)?,
                    rule_str => rule_str.parse()?,
                };
                Ok(EntryDate::RRule(ed))
            })?,
            body: match r#type.as_ref() {
                "Payment Sent" => Ok(EntryBody::PaymentSent(raw_entry.try_into()?)),
                "Payment Received" => Ok(EntryBody::PaymentReceived(raw_entry.try_into()?)),
                "Purchase Invoice" => Ok(EntryBody::PurchaseInvoice(raw_entry.try_into()?)),
                "Sales Invoice" => Ok(EntryBody::SaleInvoice(raw_entry.try_into()?)),
                _ => Err(Error::msg(format!("{} not a valid entry type", r#type))),
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

fn default_monthly_rrule(date: NaiveDate) -> rrule::Options {
    Options::new()
        .dtstart(
            Local
                .ymd(date.year(), date.month(), date.day())
                .and_hms(0, 0, 0)
                .with_timezone(&UTC),
        )
        .freq(Frequenzy::Monthly)
        .bymonthday(vec![date.day().try_into().unwrap()]) // unwrap ok, always <= 31
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
