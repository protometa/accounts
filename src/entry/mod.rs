mod raw;

use super::money::Money;
use anyhow::{bail, Context, Error, Result};
use chrono::prelude::*;
use chrono_tz::UTC;
use rrule::{Frequency, RRule, RRuleProperties};
use rust_decimal::Decimal;
use std::convert::{TryFrom, TryInto};
use std::iter::{self, Iterator};
use std::str::FromStr;

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
    RRule(Box<RRule>),
}

impl EntryDate {
    fn iter(&self) -> Box<dyn Iterator<Item = NaiveDate> + '_> {
        match self {
            EntryDate::SingleDate(date) => Box::new(iter::once(*date)),
            EntryDate::RRule(rrule) => Box::new(rrule.into_iter().map(|d| d.date().naive_utc())),
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

impl TryFrom<raw::Entry> for Entry {
    type Error = Error;

    fn try_from(raw_entry: raw::Entry) -> Result<Self> {
        let date: NaiveDate = raw_entry.date.parse()?;
        let end: Option<NaiveDate> = raw_entry.end.clone().map(|s| s.parse()).transpose()?;
        Ok(Entry {
            id: raw_entry.id.clone().context("Id missing!")?,
            // `date` is single date unless `repeat` is specified then becomes rrule
            // rrule is parsed from optional `repeat` and `end` fields
            // treating string 'monthly' as generic monthly rrule
            date: raw_entry.repeat.clone().map_or::<Result<_>, _>(
                Ok(EntryDate::SingleDate(date)),
                |rule_str| {
                    let ed = match rule_str.to_uppercase().as_str() {
                        // if simply MONTHLY use basic monthy rrule
                        "MONTHLY" => RRule::new(end.map_or(default_monthly_rrule(date), |end| {
                            default_monthly_rrule(date)
                                .until(Utc.from_utc_datetime(&end.and_hms(0, 0, 0)))
                        }))?,
                        rule_str => rule_str.parse()?,
                    };
                    Ok(EntryDate::RRule(Box::new(ed)))
                },
            )?,
            body: match raw_entry.r#type.as_ref() {
                "Payment Sent" => Ok(EntryBody::PaymentSent(raw_entry.try_into()?)),
                "Payment Received" => Ok(EntryBody::PaymentReceived(raw_entry.try_into()?)),
                "Purchase Invoice" => Ok(EntryBody::PurchaseInvoice(raw_entry.try_into()?)),
                "Sales Invoice" => Ok(EntryBody::SaleInvoice(raw_entry.try_into()?)),
                _ => Err(Error::msg(format!(
                    "{} not a valid Entry type",
                    raw_entry.r#type
                ))),
            }?,
        })
    }
}

impl FromStr for Entry {
    type Err = Error;
    fn from_str(doc: &str) -> Result<Self> {
        let mut raw_entry: raw::Entry = serde_yaml::from_str(doc)
            .with_context(|| format!("Failed to deserialize Entry:\n{}", doc))?;
        let id = format!(
            "{}|{}|{}|{}",
            raw_entry.date,
            raw_entry.r#type,
            raw_entry.party,
            raw_entry.account // TODO some random uid part
        );
        raw_entry.id.get_or_insert(id.clone());
        let entry: Entry = raw_entry
            .try_into()
            .with_context(|| format!("Failed to convert Entry: {}", id))?;
        Ok(entry)
    }
}

#[derive(Debug, Clone)]
pub struct Payment {
    pub party: String,
    pub account: String,
    pub memo: Option<String>,
    pub amount: Money,
}

impl TryFrom<raw::Entry> for Payment {
    type Error = Error;

    fn try_from(
        raw::Entry {
            party,
            account,
            memo,
            amount,
            ..
        }: raw::Entry,
    ) -> Result<Self> {
        Ok(Self {
            party,
            account,
            memo,
            amount: amount
                .context("Amount required for Payment Entry")?
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

fn default_monthly_rrule(date: NaiveDate) -> RRuleProperties {
    RRuleProperties::new(
        Frequency::Monthly,
        UTC.from_utc_datetime(&date.and_hms(0, 0, 0)),
    )
    .by_month_day(vec![date.day().try_into().unwrap()]) // unwrap ok, always <= 31
}

impl TryFrom<raw::Entry> for Invoice {
    type Error = Error;

    fn try_from(
        raw::Entry {
            party,
            account,
            items,
            extras,
            payment,
            ..
        }: raw::Entry,
    ) -> Result<Self> {
        Ok(Self {
            party,
            items: items
                .context("Items not listed on Invoice")?
                .into_iter()
                .map(|mut raw_item| {
                    raw_item.account.get_or_insert(account.clone());
                    raw_item.try_into()
                })
                .collect::<Result<Vec<InvoiceItem>>>()?,
            extras: extras
                .map(|extras| {
                    extras
                        .into_iter()
                        .map(|raw_extra| raw_extra.try_into())
                        .collect()
                })
                .transpose()?,
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

impl TryFrom<raw::Item> for InvoiceItem {
    type Error = Error;

    fn try_from(
        raw::Item {
            description,
            code,
            account,
            amount,
            quantity,
            rate,
        }: raw::Item,
    ) -> Result<Self> {
        Ok(InvoiceItem {
            description,
            code,
            account: account.context("No account for Item!")?,
            amount: match (quantity, rate, amount) {
                (Some(quantity), Some(rate), None) => InvoiceItemAmount::ByRate {
                    quantity,
                    rate: rate.try_into()?,
                },
                (None, None, Some(amount)) => InvoiceItemAmount::Total(amount.try_into()?),
                _ => bail!(
                    "Invoice Item must specify either amount \
                    exclusively or rate and quantity"
                ),
            },
        })
    }
}

impl TryFrom<raw::Extra> for InvoiceExtra {
    type Error = Error;

    fn try_from(
        raw::Extra {
            description,
            account,
            amount,
            rate,
        }: raw::Extra,
    ) -> Result<Self> {
        Ok(InvoiceExtra {
            description,
            account,
            amount: match (amount, rate) {
                (Some(amount), None) => InvoiceExtraAmount::Total(amount.try_into()?),
                (None, Some(rate)) => InvoiceExtraAmount::Rate(rate),
                (_, _) => bail!("Invoice Extra must specify either amount or rate"),
            },
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
