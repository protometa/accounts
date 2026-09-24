mod invoice;
pub mod journal;
mod payment;
pub mod raw;

use crate::money::Money;
use JournalAmount::{Credit, Debit};
use anyhow::{Context, Error, Result, anyhow, bail};
use chrono::prelude::*;
use invoice::InvoiceItemAmount::ByRate;
use invoice::{Invoice, simple_rrule};
use itertools::Itertools;
use journal::{JournalAccount, JournalAmount, JournalEntry, JournalLine, JournalLines};
use payment::*;
use raw::{ExpandedLine, InvoiceEntryType, Lines, PaymentEntryType};
use rrule::RRuleSet;
use std::collections::HashMap;
use std::convert::{TryFrom, TryInto};
use std::iter::{self, Iterator};
use std::ops::AddAssign;
use std::str::FromStr;

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
    Single(NaiveDate),
    Recurring(Box<RRuleSet>),
}

impl Date {
    // TODO impl IntoIterator
    fn iter(self) -> Box<dyn Iterator<Item = NaiveDate> + Send> {
        match self {
            Date::Single(date) => Box::new(iter::once(date)),
            Date::Recurring(recur) => Box::new(recur.clone().into_iter().map(|d| d.date_naive())),
        }
    }

    fn start(&self) -> NaiveDate {
        match self {
            Date::Single(date) => *date,
            Date::Recurring(recur) => recur.get_dt_start().date_naive(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Body {
    // one Body::Journal may represent many JournalEntry as Entry.date is possibly recurring
    Journal(JournalLines),
    PaymentSent(Payment),
    PaymentReceived(Payment),
    PurchaseInvoice(Invoice),
    SaleInvoice(Invoice),
}

pub type JournalEntryIterator = Box<dyn Iterator<Item = Result<JournalEntry>> + Send>;

impl Entry {
    pub fn id(&self) -> String {
        self.id.clone()
    }

    /// Returns simple date or first date if recurring
    pub fn date(&self) -> NaiveDate {
        self.date.start()
    }

    /// Returns iterator of entry dates up to and including `util`
    pub fn dates(self, until: NaiveDate) -> impl Iterator<Item = NaiveDate> {
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

    /// get journal lines with entry party in place of given account
    /// e.g. for accounts payable/receivable
    pub fn journal_lines_with_party(
        &self,
        until: Option<NaiveDate>,
        account: JournalAccount,
    ) -> Result<Vec<JournalLine>> {
        let party = self.party();
        if let Some(party) = party {
            self.clone()
                .into_journal_entries(until)
                .map(|j| {
                    j.map(|j| {
                        j.lines()
                            .iter()
                            .filter_map(|l| {
                                if l.0 == account {
                                    Some(JournalLine(party.to_owned(), l.1))
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .flatten_ok()
                .collect()
        } else {
            Ok(Vec::default())
        }
    }

    /// Consume and transform possibly recurring entry into iterator of journal entries
    pub fn into_journal_entries(self, until: Option<NaiveDate>) -> JournalEntryIterator {
        let until = until.unwrap_or(Local::now().date_naive());

        Box::new(
            self.clone()
                .dates(until)
                .map(move |date| self.to_journal_entry_for_date(date)),
        )
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
                    self.party().as_deref(),
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
                self.party().as_deref(),
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
                    self.party().as_deref(),
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
                self.party().as_deref(),
            ),
            Body::Journal(lines) => {
                JournalEntry::new(&self.id, &date, self.memo.as_deref(), &lines, None, None)
            }
        }
    }
}

impl TryFrom<raw::Entry> for Entry {
    type Error = Error;

    fn try_from(raw_entry: raw::Entry) -> Result<Self> {
        let date: NaiveDate = raw_entry.date().parse()?;
        // TODO handle this intrinsically when parsed as invoice
        let end: Option<NaiveDate> = raw_entry.end().map(|s| s.parse()).transpose()?;
        Ok(Entry {
            // TODO make better IDs
            // id: raw_entry.id.clone().context("Id missing!")?,
            id: raw_entry.id().unwrap_or_default(),

            // `date` is single date unless `repeat` is specified then becomes recurring
            // Recurrence is parsed from optional `repeat` and `end` fields
            // treating frequency strings like 'monthly' as simple rules
            // TODO handle this intrinsically when parsed as invoice
            date: match raw_entry.repeat() {
                Some(rule_str) => {
                    // if rule string can be parsed as a simple frequency
                    let rrule = if let Ok(freq) = rule_str.to_uppercase().parse() {
                        simple_rrule(freq, date, end)?
                    } else {
                        rule_str
                            .parse()
                            .map_err(|_| anyhow!("Failed to parse rrule"))?
                    };
                    Date::Recurring(Box::new(rrule))
                }
                None => Date::Single(date),
            },
            memo: raw_entry.memo(),
            body: match raw_entry {
                raw::Entry::PaymentEntry(raw_entry) => match raw_entry.r#type {
                    PaymentEntryType::PaymentSent => {
                        anyhow::Ok(Body::PaymentSent(raw_entry.try_into()?))
                    }
                    PaymentEntryType::PaymentReceived => {
                        Ok(Body::PaymentReceived(raw_entry.try_into()?))
                    }
                },
                raw::Entry::InvoiceEntry(raw_entry) => match raw_entry.r#type {
                    InvoiceEntryType::PurchaseInvoice => {
                        Ok(Body::PurchaseInvoice(raw_entry.try_into()?))
                    }
                    InvoiceEntryType::SalesInvoice => Ok(Body::SaleInvoice(raw_entry.try_into()?)),
                },
                raw::Entry::JournalEntry(raw_entry) => {
                    // TODO refactor this out to reusable function
                    let debit_lines: Box<dyn Iterator<Item = Result<JournalLine>>> =
                        match raw_entry.debits {
                            Lines::Simple(hashmap) => {
                                Box::new(hashmap.into_iter().map(|(account, amount)| {
                                    Ok(JournalLine(account.to_owned(), Debit(amount)))
                                }))
                            }
                            Lines::Expanded(expanded) => Box::new(expanded.into_iter().map(
                                |ExpandedLine { account, amount }| {
                                    Ok(JournalLine(account.to_owned(), Debit(amount)))
                                },
                            )),
                            Lines::Empty => bail!("Debit lines cannot be empty"),
                        };
                    let credit_lines: Box<dyn Iterator<Item = Result<JournalLine>>> =
                        match raw_entry.credits {
                            Lines::Simple(hashmap) => {
                                Box::new(hashmap.into_iter().map(|(account, amount)| {
                                    Ok(JournalLine(account.to_owned(), Credit(amount)))
                                }))
                            }
                            Lines::Expanded(expanded) => Box::new(expanded.into_iter().map(
                                |ExpandedLine { account, amount }| {
                                    Ok(JournalLine(account.to_owned(), Credit(amount)))
                                },
                            )),
                            Lines::Empty => bail!("Credit lines cannot be empty"),
                        };
                    let lines = credit_lines
                        .chain(debit_lines)
                        .collect::<Result<Vec<_>>>()?;
                    Ok(Body::Journal(JournalLines::new(lines, None)?))
                }
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

        match val.body.clone() {
            Body::Journal(lines) => {
                let debits: HashMap<String, Money> = lines
                    .iter()
                    .filter_map(|l| l.1.as_abs_debit().map(|m| (l.0.clone(), m)))
                    .collect();
                let credits: HashMap<String, Money> = lines
                    .iter()
                    .filter_map(|l| l.1.as_abs_credit().map(|m| (l.0.clone(), m)))
                    .collect();

                // TODO check to see if this is a case where expanded lines should be used
                raw::Entry::JournalEntry(raw::JournalEntry {
                    date,
                    r#type: None,
                    debits: Lines::Simple(debits),
                    credits: Lines::Simple(credits),
                    memo,
                    ..Default::default()
                })
            }
            Body::PaymentSent(payment) | Body::PaymentReceived(payment) => {
                let r#type = match val.body {
                    Body::PaymentSent(_) => raw::PaymentEntryType::PaymentSent,
                    Body::PaymentReceived(_) => raw::PaymentEntryType::PaymentReceived,
                    _ => unreachable!(),
                };
                raw::Entry::PaymentEntry(raw::PaymentEntry {
                    date,
                    r#type,
                    party: payment.party,
                    account: payment.account,
                    amount: payment.amount,
                    memo,
                    ..Default::default()
                })
            }
            Body::PurchaseInvoice(invoice) | Body::SaleInvoice(invoice) => {
                let items = if invoice.items.is_empty() {
                    None
                } else {
                    Some(raw::Items::Expanded(
                        invoice
                            .items
                            .into_iter()
                            .map(|item| {
                                let (amount, quantity, rate) = match item.amount {
                                    invoice::InvoiceItemAmount::Total(total) => {
                                        (Some(total), None, None)
                                    }
                                    ByRate { rate, quantity } => (None, Some(quantity), Some(rate)),
                                };
                                raw::Item {
                                    description: item.description,
                                    code: item.code,
                                    account: Some(item.account),
                                    amount,
                                    quantity,
                                    rate,
                                }
                            })
                            .collect(),
                    ))
                };
                let r#type = match val.body {
                    Body::SaleInvoice(_) => raw::InvoiceEntryType::SalesInvoice,
                    Body::PurchaseInvoice(_) => raw::InvoiceEntryType::PurchaseInvoice,
                    _ => unreachable!(),
                };
                raw::Entry::InvoiceEntry(raw::InvoiceEntry {
                    date,
                    r#type,
                    party: invoice.party,
                    account: invoice.account,
                    memo,
                    amount: invoice.amount,
                    items,
                    // TODO include extras
                    payment: invoice.payment,
                    ..Default::default()
                })
            }
        }
    }
}

impl FromStr for Entry {
    type Err = Error;
    fn from_str(doc: &str) -> Result<Self> {
        let mut raw_entry: raw::Entry = serde_yaml::from_str(doc)
            .with_context(|| format!("Failed to deserialize Entry:\n{doc}"))?;
        // TODO some hash or random uid part in id
        let id = format!("{}|{}", raw_entry.date(), raw_entry.clone().type_str());
        raw_entry.set_id(id.clone());
        let entry: Entry = raw_entry
            .try_into()
            .with_context(|| format!("Failed to convert Entry: {id}"))?;
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

        assert_eq!(entry.id(), "2020-01-01|Journal Entry");
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
    fn parse_invoice_no_default_account_with_total_amount() -> Result<()> {
        // default account field is optional if all items specify account
        // total amount field is checked against items
        let entry: Entry = indoc! {"
            type: Purchase Invoice
            date: 2021-01-01
            memo: Business Loan
            party: ACME Credit Union
            items:
              - account: Loan
                amount: $100.00
              - account: Interest
                amount: $10.00
            amount: $110.00
        "}
        .parse()?;

        dbg!(&entry);
        assert_eq!(
            entry.amount_of_account("Accounts Payable").unwrap(),
            JournalAmount::credit(110.00)?
        );
        assert_eq!(
            entry.amount_of_account("Loan").unwrap(),
            JournalAmount::debit(100.00)?
        );
        assert_eq!(
            entry.amount_of_account("Interest").unwrap(),
            JournalAmount::debit(10.00)?
        );

        Ok(())
    }

    #[test]
    fn parse_invoice_total_amount_error() -> Result<()> {
        // total amount provided but not correct
        let entry: Result<Entry> = indoc! {"
            type: Purchase Invoice
            date: 2021-01-01
            party: ACME Credit Union
            items:
              - account: Loan
                amount: $100.00
              - account: Interest
                amount: $10.00
            amount: $100.00
        "}
        .parse();

        dbg!(&entry);
        assert!(
            matches!(entry, Err(e) if dbg!(e.source().unwrap().to_string()).contains("Invoice ammount does not equal items total amount"))
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

    #[test]
    fn parse_unknown_fields_error() -> Result<()> {
        // if not items, use given total amount
        let entry: Result<Entry> = indoc! {"
            ---
            date: 2020-01-01
            type: Purchase Invoice
            party: ACME Construction
            account: Home Improvements
            memo: \"Invoice #1234\"
            amount: 808
            payments: # typo, this field is not plural
              account: Cash
              ammount: 808
        "}
        .parse();

        // TODO improve error message
        dbg!(&entry);
        assert!(
            matches!(entry, Err(e) if dbg!(e.source().unwrap().to_string()).contains("data did not match any variant of untagged enum Entry"))
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
