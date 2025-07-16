mod invoice;
pub mod journal;
mod payment;
mod raw;

use crate::account::Sign;
use crate::money::Money;
use anyhow::{Context, Error, Result};
use chrono::prelude::*;
use invoice::{default_monthly_rrule, Invoice};
use journal::{JournalAmount, JournalEntry, JournalLine};
use payment::*;
use rrule::RRule;
use std::convert::{TryFrom, TryInto};
use std::iter::{self, Iterator};
use std::str::FromStr;
use JournalAmount::{Credit, Debit};

/// This is a fully valid entry.
#[derive(Debug, Clone)]
pub struct Entry {
    id: String,
    date: Date,
    memo: Option<String>,
    body: Body,
}

#[derive(Debug, Clone)]
enum Date {
    SingleDate(NaiveDate),
    RRule(Box<RRule>),
}

impl Date {
    fn iter(&self) -> Box<dyn Iterator<Item = NaiveDate> + '_> {
        match self {
            Date::SingleDate(date) => Box::new(iter::once(*date)),
            Date::RRule(rrule) => Box::new(rrule.into_iter().map(|d| d.date().naive_utc())),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Body {
    // one Body::Journal may represent many JournalEntry as Entry.date is possibly RRule
    Journal(Vec<JournalLine>),
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
    pub fn body(&self) -> Body {
        self.body.clone()
    }

    pub fn to_journal_entries(&self, until: Option<NaiveDate>) -> Result<Vec<JournalEntry>> {
        let until = until.unwrap_or({
            let today = Local::today();
            NaiveDate::from_ymd(today.year(), today.month(), today.day())
        });
        self.dates(until)
            .map(|date| match self.body() {
                Body::PurchaseInvoice(invoice) => Ok(JournalEntry::new(
                    &self.id,
                    &date,
                    self.memo.as_deref(),
                    &Self::lines_from_invoice(invoice, Sign::Debit)?,
                )),
                Body::PaymentSent(payment) => Ok(JournalEntry::new(
                    &self.id,
                    &date,
                    self.memo.as_deref(),
                    &[
                        JournalLine(payment.account, Credit(payment.amount)),
                        JournalLine(String::from("Accounts Payable"), Debit(payment.amount)),
                    ],
                )),
                Body::SaleInvoice(invoice) => Ok(JournalEntry::new(
                    &self.id,
                    &date,
                    self.memo.as_deref(),
                    &Self::lines_from_invoice(invoice, Sign::Credit)?,
                )),
                Body::PaymentReceived(payment) => Ok(JournalEntry::new(
                    &self.id,
                    &date,
                    self.memo.as_deref(),
                    &[
                        JournalLine(payment.account, Debit(payment.amount)),
                        JournalLine(String::from("Accounts Receivable"), Credit(payment.amount)),
                    ],
                )),
                Body::Journal(lines) => Ok(JournalEntry::new(
                    &self.id,
                    &date,
                    self.memo.as_deref(),
                    &lines,
                )),
            })
            .collect::<Result<Vec<JournalEntry>>>()
    }

    fn lines_from_invoice(invoice: Invoice, sign: Sign) -> Result<Vec<JournalLine>> {
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
                Ok(JournalLine(
                    item.account.clone(),
                    amount_contructor(item.total()?),
                ))
            })
            .collect::<Result<Vec<JournalLine>>>()?; // TODO include inventory entries if tracking
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
            None => JournalLine(contra_account, contra_amount),
            // TODO this doesn't appear to take into account payment amount separate from
            // contra_amount
            Some(payment) => JournalLine(payment.account, contra_amount),
        };
        entries.push(contra_entry);
        Ok(entries)
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
                Ok(Date::SingleDate(date)),
                |rule_str| {
                    let ed = match rule_str.to_uppercase().as_str() {
                        // if simply MONTHLY use basic monthy rrule
                        "MONTHLY" => RRule::new(end.map_or(default_monthly_rrule(date), |end| {
                            default_monthly_rrule(date)
                                .until(Utc.from_utc_datetime(&end.and_hms(0, 0, 0)))
                        }))?,
                        rule_str => rule_str.parse()?,
                    };
                    Ok(Date::RRule(Box::new(ed)))
                },
            )?,
            memo: raw_entry.memo.to_owned(),
            body: match raw_entry.r#type {
                Some(ref s) if s == "Payment Sent" => Ok(Body::PaymentSent(raw_entry.try_into()?)),
                Some(ref s) if s == "Payment Received" => {
                    Ok(Body::PaymentReceived(raw_entry.try_into()?))
                }
                Some(ref s) if s == "Purchase Invoice" => {
                    Ok(Body::PurchaseInvoice(raw_entry.try_into()?))
                }
                Some(ref s) if s == "Sales Invoice" => Ok(Body::SaleInvoice(raw_entry.try_into()?)),
                Some(ref s) => Err(Error::msg(format!("{} not a valid Entry type", s))),
                None => {
                    Ok(Body::Journal(
                        raw_entry
                            .debits
                            .into_iter()
                            .flatten()
                            .map(|(account, amount)| {
                                Ok(JournalLine(account, Debit(amount.parse()?)))
                            })
                            .chain(raw_entry.credits.into_iter().flatten().map(
                                |(account, amount)| {
                                    Ok(JournalLine(account, Credit(amount.parse()?)))
                                },
                            ))
                            .collect::<Result<Vec<_>>>()?,
                    ))
                }
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
            "{}|{}",
            raw_entry.date,
            raw_entry
                .r#type
                .clone()
                .unwrap_or("Journal Entry".to_string()),
            // TODO some hash or random uid part
        );
        raw_entry.id.get_or_insert(id.clone());
        let entry: Entry = raw_entry
            .try_into()
            .with_context(|| format!("Failed to convert Entry: {}", id))?;
        Ok(entry)
    }
}

#[cfg(test)]
mod entry_tests {
    use super::*;
    use indoc::indoc;
    use itertools::assert_equal;
    use std::fs::read_to_string;

    #[test]
    fn parse_journal_entry() -> Result<()> {
        let entry = read_to_string("./tests/fixtures/entries_flat/2020-01-02-Journal.yaml")?
            .parse::<Entry>()?;
        dbg!(&entry);
        assert!(matches!(entry.date, Date::SingleDate(s) if s.to_string() == "2020-01-01"));
        assert_eq!(entry.memo, Some("Initial Contribution".to_string()));
        assert!(
            matches!(entry.body.clone(), Body::Journal(lines) if lines.iter().eq([
                JournalLine(
                    "Bank".to_string(),
                    Debit(15000.00.try_into()?),
                ),
                JournalLine(
                    "Owner Contributions".to_string(),
                    Credit(15000.00.try_into()?),
                ),
            ].iter()))
        );
        Ok(())
    }

    #[test]
    fn to_journal_entries_basic() -> Result<()> {
        let entry: Entry = indoc! {"
            type: Payment Sent
            date: 2025-03-06
            party: ACME Electrical 
            memo: Operating Expenses
            account: Bank Checking
            amount: 60.50
        "}
        .parse()?;
        // let jes = entry.to_journal_entries(Some("2025-04-01".parse()?))?;
        let jes = entry.to_journal_entries(None)?;
        let entry = jes.first().context("No journal entries")?;
        dbg!(entry);
        assert_eq!(entry.date().to_string(), "2025-03-06");
        assert_eq!(entry.memo(), Some("Operating Expenses".to_string()));
        assert!(entry.lines().iter().eq([
            JournalLine("Bank Checking".to_string(), Credit(60.50.try_into()?),),
            JournalLine("Accounts Payable".to_string(), Debit(60.50.try_into()?),),
        ]
        .iter()));
        Ok(())
    }
}
