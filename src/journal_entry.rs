use self::JournalAmount::*;
use super::entry::{Entry, EntryBody};
use super::money::Money;
use anyhow::Result;
use chrono::naive::NaiveDate;
use std::convert::TryFrom;
use std::fmt;

#[derive(Debug)]
pub struct JournalEntry(NaiveDate, String, JournalAmount);

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
                    "Accounts Payable".to_string(), // TODO include party
                    Debit(payment.amount.clone()),
                ),
            ]),

            EntryBody::SaleInvoice(invoice) => invoice
                .items
                .iter()
                .map(|item| {
                    Ok(JournalEntry(
                        date,
                        item.account.clone(),
                        Credit(item.total()?),
                    ))
                })
                .collect(), // TODO include Dedit entry, entries from included payment, and inventory if tracking

            EntryBody::PaymentReceived(payment) => Ok(vec![
                JournalEntry(date, payment.account, Debit(payment.amount.clone())),
                JournalEntry(
                    date,
                    "Accounts Recievable".to_string(), // TODO include party
                    Credit(payment.amount.clone()),
                ),
            ]),
        }
    }
}

impl fmt::Display for JournalEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self(date, account, amount) = self;
        write!(f, "| {} | {:25} | {} |", date, account, amount)
    }
}

#[derive(Debug)]
enum JournalAmount {
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
