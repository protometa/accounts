use self::JournalAmount::*;
use super::account::Account;
use super::chart_of_accounts::ChartOfAccounts;
use super::entry::{Entry, EntryBody};
use super::money::Money;
use anyhow::Context;
use anyhow::Result;
use chrono::naive::NaiveDate;
use std::convert::TryFrom;
use std::fmt;

#[derive(Debug)]
pub struct JournalEntry(NaiveDate, Account, JournalAmount);

impl JournalEntry {
    pub fn from_entry(entry: Entry, accounts: &ChartOfAccounts) -> Result<Vec<Self>> {
        let date = entry.date();
        match entry.body() {
            EntryBody::PurchaseInvoice(invoice) => {
                let mut entries = invoice
                    .items
                    .iter()
                    .map(|item| {
                        Ok(JournalEntry(
                            date,
                            Account::new_expense_account(&item.account),
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
                    None => JournalEntry(
                        date,
                        Account::new_accounts_payable(&invoice.party),
                        credit_amount,
                    ),
                    Some(payment) => JournalEntry(
                        date,
                        accounts
                            .get_payment_account(&payment.account)
                            .context("No payment account found in Chart of Accounts")?,
                        credit_amount,
                    ),
                };
                entries.push(credit_entry);
                Ok(entries)
            }

            EntryBody::PaymentSent(payment) => Ok(vec![
                JournalEntry(
                    date,
                    accounts
                        .get_payment_account(&payment.account)
                        .context("No payment account found in Chart of Accounts")?,
                    Credit(payment.amount.clone()),
                ),
                JournalEntry(
                    date,
                    Account::new_accounts_payable(&payment.party),
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
                            Account::new_revenue_account(&item.account),
                            Debit(item.total()?),
                        ))
                    })
                    .collect::<Result<Vec<Self>>>()?; // TODO include inventory entries if tracking
                let debit_amount = Credit(
                    invoice
                        .items
                        .iter()
                        .fold(Money::try_from(0.0), |acc, item| Ok(acc? + item.total()?))?,
                );
                let debit_entry = match invoice.payment {
                    None => JournalEntry(
                        date,
                        Account::new_accounts_receivable(&invoice.party),
                        debit_amount,
                    ),
                    Some(payment) => JournalEntry(
                        date,
                        accounts
                            .get_payment_account(&payment.account)
                            .context("No payment account found in Chart of Accounts")?,
                        debit_amount,
                    ),
                };
                entries.push(debit_entry);
                Ok(entries)
            }

            EntryBody::PaymentReceived(payment) => Ok(vec![
                JournalEntry(
                    date,
                    accounts
                        .get_payment_account(&payment.account)
                        .context("No payment account found in Chart of Accounts")?,
                    Debit(payment.amount.clone()),
                ),
                JournalEntry(
                    date,
                    Account::new_accounts_receivable(&payment.party),
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
