mod invoice;
mod payment;
mod raw;

use super::money::Money;
use anyhow::{Context, Error, Result};
use chrono::prelude::*;
use invoice::{default_monthly_rrule, Invoice};
use payment::*;
use rrule::RRule;
use std::convert::{TryFrom, TryInto};
use std::iter::{self, Iterator};
use std::str::FromStr;

/// This is a fully valid entry.
#[derive(Debug)]
pub struct Entry {
    id: String,
    date: EntryDate,
    memo: Option<String>,
    body: EntryBody,
}

#[derive(Debug)]
enum EntryDate {
    SingleDate(NaiveDate),
    RRule(Box<RRule>),
}

impl EntryDate {
    fn iter(&self) -> Box<dyn Iterator<Item = NaiveDate> + '_> {
        match self {
            EntryDate::SingleDate(date) => Box::new(iter::once(*date)),
            EntryDate::RRule(rrule) => Box::new(rrule.into_iter().map(|d| d.date().naive_utc())),
        }
    }
}

#[derive(Debug, Clone)]
pub enum EntryBody {
    JournalEntry(JournalEntryBody),
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
    pub fn body(&self) -> EntryBody {
        self.body.clone()
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
                Ok(EntryDate::SingleDate(date)),
                |rule_str| {
                    let ed = match rule_str.to_uppercase().as_str() {
                        // if simply MONTHLY use basic monthy rrule
                        "MONTHLY" => RRule::new(end.map_or(default_monthly_rrule(date), |end| {
                            default_monthly_rrule(date)
                                .until(Utc.from_utc_datetime(&end.and_hms(0, 0, 0)))
                        }))?,
                        rule_str => rule_str.parse()?,
                    };
                    Ok(EntryDate::RRule(Box::new(ed)))
                },
            )?,
            memo: raw_entry.memo.to_owned(),
            body: match raw_entry.r#type {
                Some(ref s) if s == "Payment Sent" => {
                    Ok(EntryBody::PaymentSent(raw_entry.try_into()?))
                }
                Some(ref s) if s == "Payment Received" => {
                    Ok(EntryBody::PaymentReceived(raw_entry.try_into()?))
                }
                Some(ref s) if s == "Purchase Invoice" => {
                    Ok(EntryBody::PurchaseInvoice(raw_entry.try_into()?))
                }
                Some(ref s) if s == "Sales Invoice" => {
                    Ok(EntryBody::SaleInvoice(raw_entry.try_into()?))
                }
                Some(ref s) => Err(Error::msg(format!("{} not a valid Entry type", s))),
                None => Ok(EntryBody::JournalEntry(raw_entry.try_into()?)),
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

#[derive(Debug, Clone)]
pub struct JournalEntryBody {
    pub debits: Vec<(String, Money)>,
    pub credits: Vec<(String, Money)>,
}

impl TryFrom<raw::Entry> for JournalEntryBody {
    type Error = Error;

    fn try_from(
        raw::Entry {
            debits, credits, ..
        }: raw::Entry,
    ) -> Result<Self> {
        Ok(Self {
            debits: debits
                .context("Debits not listed on Journal Entry")?
                .iter()
                .map(|(account, amount)| {
                    Ok((
                        account.to_owned(),
                        amount.to_owned().replace(",", "").parse()?,
                    ))
                })
                .collect::<Result<Vec<(String, Money)>>>()?,
            credits: credits
                .context("Credits not listed on Journal Entry")?
                .iter()
                .map(|(account, amount)| {
                    Ok((
                        account.to_owned(),
                        amount.to_owned().replace(",", "").parse()?,
                    ))
                })
                .collect::<Result<Vec<(String, Money)>>>()?,
        })
    }
}
