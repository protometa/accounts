use super::entry::{Entry, EntryBody};
use super::money::Money;
use anyhow::Result;
use chrono::naive::NaiveDate;
use std::convert::TryFrom;

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
                            JournalAmount::Debit(item.total()?),
                        ))
                    })
                    .collect::<Result<Vec<Self>>>()?; // TODO include inventory entries if tracking
                let credit_amount = JournalAmount::Credit(
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
                    JournalAmount::Credit(payment.amount.clone()),
                ),
                JournalEntry(
                    date,
                    "Accounts Payable".to_string(), // TODO include party
                    JournalAmount::Debit(payment.amount.clone()),
                ),
            ]),

            EntryBody::SaleInvoice(invoice) => invoice
                .items
                .iter()
                .map(|item| {
                    Ok(JournalEntry(
                        date,
                        item.account.clone(),
                        JournalAmount::Credit(item.total()?),
                    ))
                })
                .collect(), // TODO include Dedit entry, entries from included payment, and inventory if tracking

            EntryBody::PaymentReceived(payment) => Ok(vec![
                JournalEntry(
                    date,
                    payment.account,
                    JournalAmount::Debit(payment.amount.clone()),
                ),
                JournalEntry(
                    date,
                    "Accounts Recievable".to_string(), // TODO include party
                    JournalAmount::Credit(payment.amount.clone()),
                ),
            ]),
        }
    }
}

#[derive(Debug)]
enum JournalAmount {
    Debit(Money),
    Credit(Money),
}
