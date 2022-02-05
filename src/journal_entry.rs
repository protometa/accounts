#![allow(clippy::new_without_default)]
use self::JournalAmount::*;
use super::account::Sign;
use super::entry::{Entry, EntryBody, Invoice};
use super::money::Money;
use anyhow::Result;
use chrono::prelude::*;
use num_traits::Zero;
use std::convert::TryFrom;
use std::fmt;
use std::ops::AddAssign;

pub type JournalAccount = String;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum JournalAmount {
    Debit(Money),
    Credit(Money),
}

impl Default for JournalAmount {
    fn default() -> Self {
        JournalAmount::Debit(Money::default())
    }
}

impl JournalAmount {
    pub fn new() -> Self {
        Debit(Money::zero())
    }
}

impl fmt::Display for JournalAmount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Debit(debit) => write!(f, "{:>12} |             ", debit.to_string()),
            Self::Credit(credit) => write!(f, "             | {:>12}", credit.to_string()),
        }
    }
}

impl AddAssign for JournalAmount {
    fn add_assign(&mut self, other: Self) {
        // treat credit amount as negative to add
        let relative_self = match self {
            Debit(money) => *money,
            Credit(money) => -*money,
        };
        let relative_other = match other {
            Debit(money) => money,
            Credit(money) => -money,
        };
        let sum = relative_self + relative_other;
        if sum >= Money::zero() {
            *self = Debit(sum)
        } else {
            *self = Credit(-sum)
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct JournalEntry(pub NaiveDate, pub JournalAccount, pub JournalAmount);

impl JournalEntry {
    pub fn from_entry(entry: Entry, until: Option<NaiveDate>) -> Result<Vec<Self>> {
        let until = until.unwrap_or({
            let today = Local::today();
            NaiveDate::from_ymd(today.year(), today.month(), today.day())
        });
        Ok(entry
            .dates(until)
            .map(|date| match entry.body() {
                EntryBody::PurchaseInvoice(invoice) => {
                    Self::entries_from_invoice(invoice, date, Sign::Debit)
                }

                EntryBody::PaymentSent(payment) => Ok(vec![
                    JournalEntry(date, payment.account, Credit(payment.amount)),
                    JournalEntry(
                        date,
                        String::from("Accounts Payable"),
                        Debit(payment.amount),
                    ),
                ]),

                EntryBody::SaleInvoice(invoice) => {
                    Self::entries_from_invoice(invoice, date, Sign::Credit)
                }

                EntryBody::PaymentReceived(payment) => Ok(vec![
                    JournalEntry(date, payment.account, Debit(payment.amount)),
                    JournalEntry(
                        date,
                        String::from("Accounts Receivable"),
                        Credit(payment.amount),
                    ),
                ]),
            })
            .collect::<Result<Vec<Vec<Self>>>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<Self>>())
    }

    fn entries_from_invoice(
        invoice: Invoice,
        date: NaiveDate,
        sign: Sign,
    ) -> Result<Vec<JournalEntry>> {
        let (amount_contructor, contra_amount_contructor): (
            fn(Money) -> JournalAmount,
            fn(Money) -> JournalAmount,
        ) = match sign {
            Sign::Debit => (Debit, Credit),
            Sign::Credit => (Credit, Debit),
        };
        let mut entries = invoice
            .items
            .iter()
            .map(|item| {
                Ok(JournalEntry(
                    date,
                    item.account.clone(),
                    amount_contructor(item.total()?),
                ))
            })
            .collect::<Result<Vec<Self>>>()?; // TODO include inventory entries if tracking
        let contra_amount = contra_amount_contructor(
            invoice
                .items
                .iter()
                .fold(Money::try_from(0.0), |acc, item| Ok(acc? + item.total()?))?,
        );
        let contra_account = match sign {
            Sign::Debit => String::from("Accounts Payable"),
            Sign::Credit => String::from("Accounts Receivable"),
        };
        let contra_entry = match invoice.payment {
            None => JournalEntry(date, contra_account, contra_amount),
            Some(payment) => JournalEntry(date, payment.account, contra_amount),
        };
        entries.push(contra_entry);
        Ok(entries)
    }
}

impl fmt::Display for JournalEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self(date, account, amount) = self;
        write!(f, "{} | {:25} | {}", date, account.to_string(), amount)
    }
}
