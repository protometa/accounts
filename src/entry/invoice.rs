use super::{
    journal::JournalAccount,
    raw::{self, Payment},
};
use crate::money::Money;
use anyhow::{Context, Error, Result, bail};
use chrono::prelude::*;
use num_traits::Zero;
use rrule::{Frequency, RRule, RRuleError, RRuleSet, Tz};
use rust_decimal::Decimal;
use std::convert::{TryFrom, TryInto};

#[derive(Debug, Clone)]
pub struct Invoice {
    pub party: String,
    pub account: Option<JournalAccount>,
    pub amount: Option<Money>,
    pub items: Vec<InvoiceItem>,
    pub extras: Option<Vec<InvoiceExtra>>,
    pub payment: Option<Payment>,
}

/// Converts `NaiveDate` to `DateTime<Tz>` that can be used with `rrule`.
fn naivedate_to_rrule_utc(date: &NaiveDate) -> DateTime<Tz> {
    rrule::Tz::UTC.from_utc_datetime(&date.and_time(NaiveTime::default()))
}

#[test]
fn naivedate_to_rrule_utc_test() -> Result<()> {
    let date = NaiveDate::from_ymd_opt(2020, 1, 20);
    let rrule_date = naivedate_to_rrule_utc(&date.unwrap());
    // assert time and zone are default
    assert_eq!(rrule_date.to_string(), "2020-01-20 00:00:00 UTC");
    let date = rrule_date.date_naive();
    // assert naive date from rrule date
    assert_eq!(date.to_string(), "2020-01-20");
    Ok(())
}

/// Creates simple rrule with frequncy, start, and optional end dates.
///
/// **WARNING:** monthly frequency skip months that do not contain
/// the starting month day!
pub fn simple_rrule(
    freq: Frequency,
    start: NaiveDate,
    end: Option<NaiveDate>,
) -> Result<RRuleSet, RRuleError> {
    let mut rrule = RRule::new(freq);
    if let Some(end) = end {
        rrule = rrule.until(naivedate_to_rrule_utc(&end));
    };
    rrule.build(naivedate_to_rrule_utc(&start))
}

#[test]
fn simple_rrule_monthly_test() -> Result<()> {
    let start = "2020-11-10".parse()?;
    let freq = "MONTHLY".parse()?;
    let mut rrule = simple_rrule(freq, start, None)?.into_iter();
    // starts with first date
    assert_eq!(rrule.next().unwrap().date_naive().to_string(), "2020-11-10");
    // advanced by frequency
    assert_eq!(rrule.next().unwrap().date_naive().to_string(), "2020-12-10");
    // rolls over
    assert_eq!(rrule.next().unwrap().date_naive().to_string(), "2021-01-10");
    Ok(())
}

#[test]
fn simple_rrule_monthly_31_test() -> Result<()> {
    let start = "2020-01-31".parse()?;
    let freq = "MONTHLY".parse()?;
    let mut rrule = simple_rrule(freq, start, None)?.into_iter();
    assert_eq!(rrule.next().unwrap().date_naive().to_string(), "2020-01-31");
    // WARNING Feb was skipped since it has no 31st day
    assert_eq!(rrule.next().unwrap().date_naive().to_string(), "2020-03-31");
    Ok(())
}

impl Invoice {
    pub fn party(&self) -> String {
        self.party.clone()
    }

    pub fn bill_lines(&self) -> Result<Vec<(String, Money)>> {
        if !self.items.is_empty() {
            self.items
                .iter()
                .map(|i| Ok((i.account.clone(), i.total()?)))
                .collect::<Result<Vec<_>>>()
            // TODO incorporate extras eventually
        } else {
            let amount = self
                .amount
                .context("Items empty and no amount in invoice")?;
            let account = self
                .account
                .clone()
                .context("Items empty and no account in invoice")?;
            Ok(vec![(account, amount)])
        }
    }

    // there may be multiple payments on an invoice in future
    pub fn payment_lines(&self) -> Result<Vec<(String, Money)>> {
        if let Some(payment) = self.payment.clone() {
            Ok(vec![(payment.account, payment.amount)])
        } else {
            Ok(vec![])
        }
    }

    pub fn total(&self) -> Result<Money> {
        let total = self
            .bill_lines()?
            .iter()
            .fold(Money::zero(), |t, &(_, m)| t + m);
        Ok(total)
    }

    // TODO impl inventory tracking methods
}

impl TryFrom<raw::InvoiceEntry> for Invoice {
    type Error = Error;

    fn try_from(
        raw::InvoiceEntry {
            party,
            account,
            items,
            extras,
            payment,
            amount,
            ..
        }: raw::InvoiceEntry,
    ) -> Result<Self> {
        if items.is_none() && amount.is_none() {
            bail!("Either items or amount (or both) required for Invoice")
        }
        let invoice = Self {
            party,
            account: account.clone(),
            amount: if items.is_none() { amount } else { None },
            items: items
                .iter() // iterate over Option to flatten and collect
                .flat_map(|items| {
                    items.as_expanded().into_iter().map(|mut raw_item| {
                        let item_account = raw_item.account.or(account.clone());
                        if item_account.is_none() {
                            bail!("If invoice does not contain default account, then all items must specify account");
                        }
                        raw_item.account = item_account;
                        raw_item.try_into()
                    })
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
                .map(|payment| -> Result<Payment> {
                    Ok(Payment {
                        account: payment.account,
                        amount: payment.amount,
                    })
                })
                .transpose()?,
        };
        let total = invoice.total()?; // this also serves to validate invoice
        if amount.is_some_and(|a| a != total) {
            bail!("Invoice ammount does not equal items total amount");
        }
        Ok(invoice)
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
                (Some(quantity), Some(rate), None) => InvoiceItemAmount::ByRate { quantity, rate },
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
