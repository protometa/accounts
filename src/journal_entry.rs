use self::JournalAmount::*;
use super::entry::{Entry, EntryBody};
use super::money::Money;
use anyhow::Result;
use chrono::naive::NaiveDate;
use std::convert::TryFrom;
use std::fmt;

pub type JournalAccount = String;

#[derive(Debug, Clone)]
pub enum JournalAmount {
    Debit(Money),
    Credit(Money),
}

impl fmt::Display for JournalAmount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Debit(debit) => write!(f, "{:>12} | {:12}", debit.to_string(), ""),
            Self::Credit(credit) => write!(f, "{:12} | {:>12}", "", credit.to_string()),
        }
    }
}

#[derive(Debug)]
pub struct JournalEntry(pub NaiveDate, pub JournalAccount, pub JournalAmount);

impl JournalEntry {
    pub fn from_entry(entry: Entry) -> Result<Vec<Self>> {
        let date = entry.date();
        match entry.body() {
            EntryBody::PurchaseInvoice(invoice) => {
                let mut entries = invoice
                    .items
                    .iter()
                    .map(|item| {
                        Ok(JournalEntry(
                            date,
                            item.account.clone(),
                            Debit(item.total()?),
                        ))
                    })
                    .collect::<Result<Vec<Self>>>()?; // TODO include inventory entries if tracking
                let credit_amount = Credit(
                    invoice
                        .items
                        .iter()
                        .fold(Money::try_from(0.0), |acc, item| Ok(acc? + item.total()?))?,
                );
                let credit_entry = match invoice.payment {
                    None => JournalEntry(date, String::from("Accounts Payable"), credit_amount),
                    Some(payment) => JournalEntry(date, payment.account.clone(), credit_amount),
                };
                entries.push(credit_entry);
                Ok(entries)
            }

            EntryBody::PaymentSent(payment) => Ok(vec![
                JournalEntry(
                    date,
                    payment.account.clone(),
                    Credit(payment.amount.clone()),
                ),
                JournalEntry(
                    date,
                    String::from("Accounts Payable"),
                    Debit(payment.amount.clone()),
                ),
            ]),

            EntryBody::SaleInvoice(invoice) => {
                let mut entries = invoice
                    .items
                    .iter()
                    .map(|item| {
                        Ok(JournalEntry(
                            date,
                            item.account.clone(),
                            Credit(item.total()?),
                        ))
                    })
                    .collect::<Result<Vec<Self>>>()?; // TODO include inventory entries if tracking
                let debit_amount = Debit(
                    invoice
                        .items
                        .iter()
                        .fold(Money::try_from(0.0), |acc, item| Ok(acc? + item.total()?))?,
                );
                let debit_entry = match invoice.payment {
                    None => JournalEntry(date, String::from("Accounts Receivable"), debit_amount),
                    Some(payment) => JournalEntry(date, payment.account.clone(), debit_amount),
                };
                entries.push(debit_entry);
                Ok(entries)
            }

            EntryBody::PaymentReceived(payment) => Ok(vec![
                JournalEntry(date, payment.account.clone(), Debit(payment.amount.clone())),
                JournalEntry(
                    date,
                    String::from("Accounts Receivable"),
                    Credit(payment.amount.clone()),
                ),
            ]),
        }
    }
}

impl fmt::Display for JournalEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self(date, account, amount) = self;
        write!(f, "| {} | {:25} | {} |", date, account.to_string(), amount)
    }
}
