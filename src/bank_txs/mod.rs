pub mod reconciliation_rules;
use crate::{
    entry::journal::{
        JournalAccount,
        JournalAmount::{self, Credit, Debit},
        JournalEntry,
    },
    money::Money,
};
use anyhow::{anyhow, Context, Error, Result};
use async_std::fs::File;
use async_std::io::BufReader;
use async_std::prelude::*;
use chrono::{Datelike, NaiveDate};
use futures::{future, TryStreamExt};
use reconciliation_rules::ReconciliationRules;
use serde::{
    ser::{self, SerializeMap},
    Serialize, Serializer,
};
use std::{convert::TryInto, str::FromStr};

#[derive(Debug, PartialEq, Clone)]
pub struct BankTx {
    date: NaiveDate,
    account: JournalAccount,
    amount: JournalAmount,
    memo: String,
}

/// Implement Serialize into various fields for matching against rules
impl Serialize for BankTx {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use ser::Error;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("date", &self.date.to_string())?;
        // special date component fields
        map.serialize_entry("year", &self.date.year())?;
        map.serialize_entry("month", &self.date.month())?;
        map.serialize_entry("day", &self.date.day())?;

        map.serialize_entry("memo", &self.memo)?;
        map.serialize_entry("account", &self.account)?;

        let amount: f64 = match self.amount {
            Credit(credit) => credit.0.try_into().map_err(S::Error::custom)?,
            Debit(debit) => debit.0.try_into().map_err(S::Error::custom)?,
        };
        match self.amount {
            Credit(_) => {
                map.serialize_entry("credit", &amount)?;
            }
            Debit(_) => {
                map.serialize_entry("debit", &amount)?;
            }
        };
        // special field which is normalized amount of either credit or debit
        map.serialize_entry("amount", &amount)?;
        map.end()
    }
}

impl FromStr for BankTx {
    type Err = Error; // TODO custom parse error?

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split('|');
        let date = parts
            .next()
            .context(anyhow!("Error getting date from bank tx: '{}'", &s))?
            .parse()?;
        let account = parts
            .next()
            .context(anyhow!("Error getting account from bank tx: '{}'", &s))?
            .trim()
            .to_owned();
        let debit = parts
            .next()
            .context(anyhow!("Error getting debit from bank tx: '{}'", &s))?
            .trim()
            .parse::<Money>();
        let credit = parts
            .next()
            .context(anyhow!("Error getting credit from bank tx: '{}'", &s))?
            .trim()
            .parse::<Money>();
        let memo = parts
            .next()
            .context(anyhow!("Error getting memo from bank tx: '{}'", &s))?
            .trim()
            .to_owned();

        let amount = match (&debit, &credit) {
            (Ok(amt), Err(_)) => Ok(JournalAmount::Debit(*amt)),
            (Err(_), Ok(amt)) => Ok(JournalAmount::Credit(*amt)),
            (Ok(_), Ok(_)) => Err(anyhow!("Both credit and debit given in bank tx: '{}'", &s)),
            (Err(_), Err(_)) => Err(anyhow!("Error parsing bank tx amount: '{}'", &s)),
        }?;

        Ok(Self {
            date,
            account,
            amount,
            memo,
        })
    }
}

#[derive(Debug)]
pub struct BankTxs {
    pub txs: Vec<BankTx>,
    pub rules: ReconciliationRules,
}

impl BankTxs {
    pub async fn from_file(file: &str) -> Result<Self> {
        // TODO accept txs as dir and accept rules file
        let file = File::open(file).await?;
        let txs: Vec<BankTx> = BufReader::new(file)
            .lines()
            .map_err(Error::new) // map to anyhow::Error from here on
            .and_then(|line| future::ready(line.parse()))
            .try_collect()
            .await?;
        Ok(Self {
            txs,
            rules: ReconciliationRules::default(),
        })
    }

    // // get max date of txs
    // let until = self.0.iter().fold(None, |date, tx| {
    //     if let Some(date) = date {
    //         if tx.date > date {
    //             return Some(tx.date);
    //         }
    //     }
    //     date
    // });

    /// match given entry to txs and remove from set of txs
    pub fn match_and_rm(&mut self, entry: JournalEntry) -> Option<BankTx> {
        // TODO handle case where entry matches bank account but not found
        if self.txs.is_empty() {
            return None;
        }
        if let Some(i) = self.txs.iter().position(|tx| {
            self.rules
                .apply(tx)
                .is_ok_and(|g| g.match_entry(&entry).is_ok_and(|m| m))
        }) {
            return Some(self.txs.swap_remove(i));
        };
        None
    }

    /// This will generate new entries from remaining txs after matching and removing
    pub fn generate_entries(&mut self) {
        // self.txs.iter().map(|tx| self.rules.apply(tx)?.generate());
        todo!()
    }
}

#[cfg(test)]
mod bank_txs_tests {
    use super::*;
    use crate::entry::Entry;
    use anyhow::Result;
    use indoc::indoc;
    use std::convert::TryInto;

    #[test]
    fn bank_tx_parse() -> Result<()> {
        let tx: BankTx = "2025-03-06 | XXXX0000 |      60.50 |            | Electrical".parse()?;
        assert_eq!(
            tx,
            BankTx {
                date: "2025-03-06".parse()?,
                account: "XXXX0000".to_string(),
                amount: JournalAmount::Debit(60.50f64.try_into()?),
                memo: "Electrical".to_string(),
            }
        );
        let tx: BankTx = "2025-03-07 | XXXX0000 |            |     500.00 | Transfer".parse()?;
        assert_eq!(
            tx,
            BankTx {
                date: "2025-03-07".parse()?,
                account: "XXXX0000".to_string(),
                amount: JournalAmount::Credit(500.00f64.try_into()?),
                memo: "Transfer".to_string(),
            }
        );
        Ok(())
    }

    #[test]
    fn bank_tx_parse_errs() -> Result<()> {
        let tx: Result<BankTx> =
            "2025-03-06 | XXXX0000 |            |            | Electrical".parse();
        assert!(
            matches!(tx, Err(e) if dbg!(e.to_string()).contains("Error parsing bank tx amount"))
        );
        let tx: Result<BankTx> = "2025-03-06 ".parse();
        assert!(
            matches!(tx, Err(e) if dbg!(e.to_string()).contains("Error getting account from bank tx"))
        );
        Ok(())
    }

    // TODO test journal entry

    #[test]
    fn match_payment_sent() -> Result<()> {
        let mut txs = BankTxs {
            txs: vec!["2025-03-06 | XX00 |  60.50 |        | Electrical".parse()?],
            rules: indoc! {r#"
                rule: [eq, account, "XX00"]
                values:
                  bank_account: "Bank Checking"
            "#}
            .parse()?,
        };
        let entry = indoc! {"
            type: Payment Sent
            date: 2025-03-06
            party: ACME Electrical 
            memo: Operating Expenses
            account: Bank Checking
            amount: 60.50
        "}
        .parse::<Entry>()?
        .to_journal_entries(None)?[0]
            .clone();

        let matched = txs.match_and_rm(entry);

        dbg!(&matched);
        assert!(matched.is_some());
        assert!(txs.txs.is_empty());
        Ok(())
    }

    #[test]
    fn match_payment_received() -> Result<()> {
        let mut txs = BankTxs {
            txs: vec!["2025-03-07 | XX00 |        | 310.00 | POS deposit".parse()?],
            rules: indoc! {r#"
                rule: [eq, account, "XX00"]
                values:
                  bank_account: Bank Checking
            "#}
            .parse()?,
        };
        let matched = txs.match_and_rm(
            indoc! {"
                type: Payment Received
                date: 2025-03-07
                party: ACME POS
                account: Bank Checking
                amount: 310.00
            "}
            .parse::<Entry>()?
            .to_journal_entries(None)?[0]
                .clone(),
        );
        dbg!(&matched);
        assert!(matched.is_some());
        assert!(txs.txs.is_empty());
        Ok(())
    }

    // TODO test not matching on various fields

    // TODO test invoices with payment

    // TODO implement inexact dates
    // TODO test entry generation from rules

    #[test]
    #[ignore]
    fn generate_payment_received() -> Result<()> {
        let mut txs = BankTxs {
            txs: vec!["2025-03-07 | X0 |        | 310.00 | POS deposit".parse()?],
            // TODO payments should probably be default with received/sent variations based on bank account and debit/credit
            rules: indoc! {r#"
                rule: [eq, account, "XXX000"]
                values:
                  bank_account: Bank Checking
                ---
                rule: [contains, memo, "POS deposit"]
                entry:
                  type: Payment Received
                  party: ACME POS
            "#}
            .parse()?,
        };
        let entries = txs.generate_entries();

        todo!();

        Ok(())
    }
}
