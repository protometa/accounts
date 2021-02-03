use super::entry::{Entry, EntryBody};
use super::money::Money;
use anyhow::Result;
use chrono::naive::NaiveDate;

#[derive(Debug)]
pub struct JournalEntry {
    date: NaiveDate,
    account: String,
    amount: JournalAmount,
}

impl JournalEntry {
    pub fn from_entry(entry: Entry) -> Vec<Result<Self>> {
        let date = entry.date();
        match entry.body() {
            EntryBody::PurchaseInvoice(invoice) => invoice
                .items()
                .iter()
                .map(|item| {
                    Ok(JournalEntry {
                        date,
                        account: item.account(),
                        amount: JournalAmount::Debit(item.amount()?),
                    })
                })
                .collect(), // TODO include Credit entry and entries from included payment

            EntryBody::PaymentSent(payment) => vec![
                Ok(JournalEntry {
                    date,
                    account: payment.account(),
                    amount: JournalAmount::Credit(payment.amount()),
                }),
                Ok(JournalEntry {
                    date,
                    account: "Accounts Payable".to_string(), // TODO include party
                    amount: JournalAmount::Debit(payment.amount()),
                }),
            ],

            EntryBody::SaleInvoice(invoice) => invoice
                .items()
                .iter()
                .map(|item| {
                    Ok(JournalEntry {
                        date,
                        account: item.account(),
                        amount: JournalAmount::Credit(item.amount()?),
                    })
                })
                .collect(), // TODO include Dedit entry and entries from included payment

            EntryBody::PaymentReceived(payment) => vec![
                Ok(JournalEntry {
                    date,
                    account: payment.account(),
                    amount: JournalAmount::Debit(payment.amount()),
                }),
                Ok(JournalEntry {
                    date,
                    account: "Accounts Recievable".to_string(), // TODO include party
                    amount: JournalAmount::Credit(payment.amount()),
                }),
            ],
        }
    }
}

#[derive(Debug)]
enum JournalAmount {
    Debit(Money),
    Credit(Money),
}
