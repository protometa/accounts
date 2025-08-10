mod invoice;
pub mod journal;
mod payment;
pub mod raw;

use crate::money::Money;
use anyhow::{Context, Error, Result};
use chrono::prelude::*;
use invoice::{default_monthly_rrule, Invoice};
use journal::{JournalAmount, JournalEntry, JournalLine, JournalLines};
use payment::*;
use raw::{ExpandedLine, Lines};
use rrule::RRule;
use std::collections::HashMap;
use std::convert::{TryFrom, TryInto};
use std::iter::{self, Iterator};
use std::ops::AddAssign;
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

    fn start(&self) -> NaiveDate {
        match self {
            Date::SingleDate(date) => *date,
            Date::RRule(rrule) => {
                // RRule zones are all treated as utc
                rrule.get_properties().dt_start.date().naive_utc()
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum Body {
    // one Body::Journal may represent many JournalEntry as Entry.date is possibly RRule
    Journal(JournalLines),
    PaymentSent(Payment),
    PaymentReceived(Payment),
    PurchaseInvoice(Invoice),
    SaleInvoice(Invoice),
}

impl Entry {
    pub fn id(&self) -> String {
        self.id.clone()
    }

    /// Returns simple date or first date if recurring
    pub fn date(&self) -> NaiveDate {
        self.date.start()
    }

    /// Returns iterator of entry dates up to and including `util`
    pub fn dates(&self, until: NaiveDate) -> impl Iterator<Item = NaiveDate> + '_ {
        self.date.iter().take_while(move |d| *d <= until)
    }

    pub fn memo(&self) -> Option<String> {
        self.memo.clone()
    }

    /// Absolute amount of entry (not as debit or credit)
    pub fn abs_amount(&self) -> Result<Money> {
        // get absolute amount from all lines of journal entry
        let (total_debit, total_credit) = self.lines()?.iter().fold(
            (Money::default(), Money::default()),
            |(mut debit, mut credit), JournalLine(_, amount)| {
                match amount {
                    Debit(money) => debit += *money,
                    Credit(money) => credit += *money,
                };
                (debit, credit)
            },
        );
        assert!(
            total_debit == total_credit,
            "Journal entry total debits and credits were not equal!"
        ); // this should never fail
        Ok(total_debit)
    }

    /// Get debit or credit amount from for given account
    pub fn amount_of_account(&self, account: &str) -> Option<JournalAmount> {
        self.lines().ok().and_then(|lines| {
            lines
                .iter()
                .filter(|JournalLine(a, _)| a == account)
                .map(|l| l.1)
                .reduce(|mut a: JournalAmount, b| {
                    a.add_assign(b);
                    a
                })
        })
    }

    /// Get party if entry is an invoice or payment type
    pub fn party(&self) -> Option<String> {
        match &self.body {
            Body::PaymentSent(p) | Body::PaymentReceived(p) => Some(p.party.clone()),
            Body::PurchaseInvoice(i) | Body::SaleInvoice(i) => Some(i.party().clone()),
            _ => None,
        }
    }

    /// Get journal lines
    pub fn lines(&self) -> Result<JournalLines> {
        Ok(self.to_journal_entry()?.lines())
    }

    /// Get all journal entries of possibly recurring entry
    // TODO why doesn't this return an iterator?
    pub fn to_journal_entries(&self, until: Option<NaiveDate>) -> Result<Vec<JournalEntry>> {
        let until = until.unwrap_or({
            let today = Local::today();
            NaiveDate::from_ymd(today.year(), today.month(), today.day())
        });
        self.dates(until)
            .map(|date| self.to_journal_entry_for_date(date))
            .collect::<Result<Vec<JournalEntry>>>()
    }

    pub fn to_journal_entry(&self) -> Result<JournalEntry> {
        self.to_journal_entry_for_date(self.date())
    }

    /// Used internally to generate a journal entry from simple date
    /// or many from recurring dates
    fn to_journal_entry_for_date(&self, date: NaiveDate) -> Result<JournalEntry> {
        match self.body.clone() {
            Body::PurchaseInvoice(invoice) => {
                let bill_lines = invoice
                    .bill_lines()?
                    .into_iter()
                    .map(|l| JournalLine(l.0, Debit(l.1)));
                let payment_lines = invoice
                    .payment_lines()?
                    .into_iter()
                    .map(|l| JournalLine(l.0, Credit(l.1)));
                let lines = bill_lines.chain(payment_lines).collect::<Vec<_>>();

                JournalEntry::new(
                    &self.id,
                    &date,
                    self.memo.as_deref(),
                    &lines,
                    Some("Accounts Payable".to_string()),
                )
            }
            Body::PaymentSent(payment) => JournalEntry::new(
                &self.id,
                &date,
                self.memo.as_deref(),
                &[
                    JournalLine(payment.account, Credit(payment.amount)),
                    JournalLine("Accounts Payable".to_string(), Debit(payment.amount)),
                ],
                None,
            ),
            Body::SaleInvoice(invoice) => {
                let bill_lines = invoice
                    .bill_lines()?
                    .into_iter()
                    .map(|l| JournalLine(l.0, Credit(l.1)));
                let payment_lines = invoice
                    .payment_lines()?
                    .into_iter()
                    .map(|l| JournalLine(l.0, Debit(l.1)));
                let lines = bill_lines.chain(payment_lines).collect::<Vec<_>>();

                JournalEntry::new(
                    &self.id,
                    &date,
                    self.memo.as_deref(),
                    &lines,
                    Some("Accounts Receivable".to_string()),
                )
            }
            Body::PaymentReceived(payment) => JournalEntry::new(
                &self.id,
                &date,
                self.memo.as_deref(),
                &[
                    JournalLine(payment.account, Debit(payment.amount)),
                    JournalLine("Accounts Receivable".to_string(), Credit(payment.amount)),
                ],
                None,
            ),
            Body::Journal(lines) => {
                JournalEntry::new(&self.id, &date, self.memo.as_deref(), &lines, None)
            }
        }
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
            body: match raw_entry.r#type.as_deref() {
                Some("Payment Sent") => Ok(Body::PaymentSent(raw_entry.try_into()?)),
                Some("Payment Received") => Ok(Body::PaymentReceived(raw_entry.try_into()?)),
                Some("Purchase Invoice") => Ok(Body::PurchaseInvoice(raw_entry.try_into()?)),
                Some("Sales Invoice") => Ok(Body::SaleInvoice(raw_entry.try_into()?)),
                Some("Journal Entry") | None => {
                    // TODO refactor this out to reusable function
                    let debit_lines: Box<dyn Iterator<Item = Result<JournalLine>>> =
                        match raw_entry.debits {
                            Some(Lines::Simple(hashmap)) => {
                                Box::new(hashmap.into_iter().map(|(account, amount)| {
                                    Ok(JournalLine(account.to_owned(), Debit(amount)))
                                }))
                            }
                            Some(Lines::Expanded(expanded)) => Box::new(expanded.into_iter().map(
                                |ExpandedLine { account, amount }| {
                                    Ok(JournalLine(account.to_owned(), Debit(amount)))
                                },
                            )),
                            None => Box::new(std::iter::empty()),
                        };
                    let credit_lines: Box<dyn Iterator<Item = Result<JournalLine>>> =
                        match raw_entry.credits {
                            Some(Lines::Simple(hashmap)) => {
                                Box::new(hashmap.into_iter().map(|(account, amount)| {
                                    Ok(JournalLine(account.to_owned(), Credit(amount)))
                                }))
                            }
                            Some(Lines::Expanded(expanded)) => Box::new(expanded.into_iter().map(
                                |ExpandedLine { account, amount }| {
                                    Ok(JournalLine(account.to_owned(), Credit(amount)))
                                },
                            )),
                            None => Box::new(std::iter::empty()),
                        };
                    let lines = credit_lines
                        .chain(debit_lines)
                        .collect::<Result<Vec<_>>>()?;
                    Ok(Body::Journal(JournalLines::new(lines, None)?))
                }
                Some(s) => Err(Error::msg(format!("{} not a valid Entry type", s))),
            }?,
        })
    }
}

// impl TryInto<raw::Entry> for Entry {
impl From<Entry> for raw::Entry {
    // type Error = Error;

    // fn try_into(self) -> std::result::Result<raw::Entry, Self::Error> {
    fn from(val: Entry) -> Self {
        // let id = Some(val.id);
        let date = val.date().to_string();
        let memo = val.memo();

        let raw_body = match val.body {
            Body::Journal(lines) => {
                let debits: HashMap<String, Money> = lines
                    .iter()
                    .filter_map(|l| l.1.as_debit().map(|m| (l.0.clone(), m)))
                    .collect();
                let credits: HashMap<String, Money> = lines
                    .iter()
                    .filter_map(|l| l.1.as_credit().map(|m| (l.0.clone(), m)))
                    .collect();

                // TODO check to see if this is a case where expanded lines should be used
                raw::Entry {
                    r#type: None,
                    debits: Some(Lines::Simple(debits)),
                    credits: Some(Lines::Simple(credits)),
                    ..Default::default()
                }
            }
            Body::PaymentSent(payment) => raw::Entry {
                r#type: Some("Payment Sent".to_string()),
                party: Some(payment.party),
                account: Some(payment.account),
                amount: Some(payment.amount),
                ..Default::default()
            },
            Body::PaymentReceived(payment) => raw::Entry {
                r#type: Some("Payment Received".to_string()),
                party: Some(payment.party),
                account: Some(payment.account),
                amount: Some(payment.amount),
                ..Default::default()
            },
            // TODO handle all the other types
            _ => {
                todo!()
            }
        };

        // Ok(raw::Entry {
        raw::Entry {
            date,
            memo,
            ..raw_body
        }
        // })
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

    

    #[test]
    fn parse_journal_entry() -> Result<()> {
        let entry: Entry = indoc! {"
            ---
            date: 2020-01-01
            memo: Initial Contribution
            debits:
              Bank: 500
            credits:
              Owner Contributions: 500
        "}
        .parse()?;

        dbg!(&entry);

        assert_eq!(entry.date(), "2020-01-01".parse()?);
        assert_eq!(entry.memo(), Some("Initial Contribution".to_string()));

        assert_eq!(
            entry.amount_of_account("Bank").unwrap(),
            JournalAmount::debit(500.00)?
        );
        assert_eq!(
            entry.amount_of_account("Owner Contributions").unwrap(),
            JournalAmount::credit(500.00)?
        );

        Ok(())
    }

    #[test]
    fn parse_journal_entry_expanded_accounts() -> Result<()> {
        // allows expanded journal line format for more advanced entries
        // split into to separate deposits which will match bank txs
        let entry: Entry = indoc! {"
            ---
            date: 2020-01-02
            memo: Initial Contribution
            credits:
              Owner Contributions: $15,000.00  
            debits:
              - account: Bank
                amount: $10000.00
              - account: Bank
                amount: $50,00.00
        "}
        .parse()?;

        dbg!(&entry);

        assert_eq!(entry.date(), "2020-01-02".parse()?);
        assert_eq!(entry.memo(), Some("Initial Contribution".to_string()));

        assert_eq!(
            entry.amount_of_account("Bank").unwrap(),
            JournalAmount::debit(15000.00)?
        );
        assert_eq!(
            entry.amount_of_account("Owner Contributions").unwrap(),
            JournalAmount::credit(15000.00)?
        );

        // contains two lines for "Owner Contributions"
        assert_eq!(entry.lines()?.iter().filter(|l| l.0 == "Bank").count(), 2);

        Ok(())
    }

    #[test]
    fn parse_payment_entry() -> Result<()> {
        let entry: Entry = indoc! {"
            type: Payment Sent
            date: 2025-03-06
            party: ACME Electrical 
            memo: Operating Expenses
            account: Bank Checking
            amount: 60.50
        "}
        .parse()?;

        dbg!(&entry);
        assert_eq!(entry.date(), "2025-03-06".parse()?);
        assert_eq!(entry.memo(), Some("Operating Expenses".to_string()));

        assert_eq!(
            entry.amount_of_account("Bank Checking").unwrap(),
            JournalAmount::credit(60.50)?
        );
        assert_eq!(
            entry.amount_of_account("Accounts Payable").unwrap(),
            JournalAmount::debit(60.50)?
        );

        Ok(())
    }

    #[test]
    fn parse_invoice_entry() -> Result<()> {
        let entry: Entry = indoc! {"
            type: Purchase Invoice
            date: 2020-01-01
            party: ACME Business Services
            account: Operating Expenses
            items:
              - description: Business Services
                amount: 100
        "}
        .parse()?;

        dbg!(&entry);
        assert_eq!(entry.date(), "2020-01-01".parse()?);
        assert_eq!(entry.memo(), None);
        assert_eq!(entry.party(), Some("ACME Business Services".to_string()));

        assert_eq!(
            entry.amount_of_account("Accounts Payable").unwrap(),
            JournalAmount::credit(100.00)?
        );
        assert_eq!(
            entry.amount_of_account("Operating Expenses").unwrap(),
            JournalAmount::debit(100.00)?
        );

        Ok(())
    }

    #[test]
    fn parse_invoice_simple_items() -> Result<()> {
        // if items is map, then treat as description -> amount
        let entry: Entry = indoc! {"
            type: Purchase Invoice
            date: 2021-01-01
            party: ACME Business Services
            account: Operating Expenses
            items:
              Paperclips: 0.05
        "}
        .parse()?;

        dbg!(&entry);
        assert_eq!(entry.date(), "2021-01-01".parse()?);
        assert_eq!(entry.memo(), None);

        assert_eq!(
            entry.amount_of_account("Accounts Payable").unwrap(),
            JournalAmount::credit(0.05)?
        );
        assert_eq!(
            entry.amount_of_account("Operating Expenses").unwrap(),
            JournalAmount::debit(0.05)?
        );

        Ok(())
    }

    #[test]
    fn parse_invoice_no_items() -> Result<()> {
        // if not items, use given total amount
        let entry: Entry = indoc! {"
            type: Purchase Invoice
            date: 2021-01-01
            party: ACME Business Services
            account: Operating Expenses
            amount: 0.05
        "}
        .parse()?;

        dbg!(&entry);
        assert_eq!(entry.date(), "2021-01-01".parse()?);
        assert_eq!(entry.memo(), None);

        assert_eq!(
            entry.amount_of_account("Accounts Payable").unwrap(),
            JournalAmount::credit(0.05)?
        );
        assert_eq!(
            entry.amount_of_account("Operating Expenses").unwrap(),
            JournalAmount::debit(0.05)?
        );

        Ok(())
    }

    // TODO decide variations
    #[test]
    #[ignore]
    fn parse_invoice_condensed() -> Result<()> {
        // Not sure if I like this concept
        let entry: Entry = indoc! {"
            type: Purchase Invoice
            date: 2021-01-01
            party: ACME Business Services
            items:
              Operating Expenses: 0.05
            payments:
              Bank: 0.05
        "}
        .parse()?;

        dbg!(&entry);
        assert_eq!(entry.date(), "2021-01-01".parse()?);
        assert_eq!(entry.memo(), None);

        assert_eq!(
            entry.amount_of_account("Bank").unwrap(),
            JournalAmount::credit(0.05)?
        );
        assert_eq!(
            entry.amount_of_account("Operating Expenses").unwrap(),
            JournalAmount::debit(0.05)?
        );

        Ok(())
    }

    #[test]
    #[ignore]
    fn parse_syntax_error() -> Result<()> {
        // TODO this produces a very obscure error message and I'm not sure why it's missing context
        let entry: Entry = indoc! {"
            type: Payment Sent
            date: 2025-03-08
            party: ACME Electrical
            account: {bank_account}
            amount: 200.00
        "}
        .parse()?;
        dbg!(&entry);
        Ok(())
    }
}
