use super::raw;
use crate::money::Money;
use anyhow::{bail, Context, Error, Result};
use chrono::prelude::*;
use chrono_tz::UTC;
use rrule::{Frequency, RRuleProperties};
use rust_decimal::Decimal;
use std::convert::{TryFrom, TryInto};

#[derive(Debug, Clone)]
pub struct Invoice {
    pub party: String,
    pub items: Vec<InvoiceItem>,
    pub extras: Option<Vec<InvoiceExtra>>,
    pub payment: Option<InvoicePayment>,
}

pub fn default_monthly_rrule(date: NaiveDate) -> RRuleProperties {
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
        let account = account.context("Account required for Invoice")?;
        Ok(Self {
            party: party.context("Party required for Invoice")?,
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
                        amount: payment.amount,
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
                (None, None, Some(amount)) => InvoiceItemAmount::Total(amount),
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
                (Some(amount), None) => InvoiceExtraAmount::Total(amount),
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
