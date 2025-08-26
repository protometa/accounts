pub mod rec_rules;
use crate::{
    entry::{
        Entry,
        journal::{
            JournalAccount,
            JournalAmount::{self, Credit, Debit},
        },
    },
    money::Money,
};
use anyhow::{Context, Error, Result, anyhow};
use async_std::io::BufReader;
use async_std::prelude::*;
use async_std::{fs::File, io::ReadExt};
use chrono::{Datelike, NaiveDate};
use futures::{TryStreamExt, future};
use rec_rules::RecRules;
use serde::{
    Serialize, Serializer,
    ser::{self, SerializeMap},
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
    pub rules: RecRules,
}

impl BankTxs {
    pub async fn from_files(txs_file: &str, rules_file: Option<&str>) -> Result<Self> {
        // TODO accept txs as dir and accept rules file
        let file = File::open(txs_file).await?;
        let txs: Vec<BankTx> = BufReader::new(file)
            .lines()
            .map_err(Error::new) // map to anyhow::Error from here on
            .and_then(|line| future::ready(line.parse()))
            .try_collect()
            .await?;

        let rules = if let Some(rules_file) = rules_file {
            let mut file = File::open(rules_file).await?;
            let mut contents = String::new();
            file.read_to_string(&mut contents).await?;
            contents.parse()?
        } else {
            RecRules::default()
        };

        Ok(Self { txs, rules })
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
    pub fn match_and_rm(&mut self, entry: Entry) -> Vec<BankTx> {
        if self.txs.is_empty() {
            return Vec::new();
        }
        let rules = self.rules.clone();
        let mut removed = Vec::new();

        while let Some((position, relevant)) =
            self.txs
                .clone()
                .into_iter()
                .enumerate()
                .find_map(|(i, tx)| {
                    rules
                        .apply(&tx)
                        .and_then(|g| g.match_entry(&entry))
                        .unwrap_or(None)
                        .map(|r| (i, r))
                })
            && removed.len() < relevant
        {
            removed.push(self.txs.swap_remove(position));
        }
        removed
    }

    /// This will generate new entries from remaining txs after matching and removing
    pub fn generate_entries(&mut self) -> Result<Vec<Entry>> {
        // TODO stream this? or at least allow some errors without all failing
        self.txs
            .iter()
            .map(|tx| anyhow::Ok(self.rules.apply(tx)?.generate()?))
            .collect::<Result<Vec<Entry>>>()
    }
}

#[cfg(test)]
mod bank_txs_tests {
    use super::*;

    use anyhow::Result;

    use indoc::indoc;
    use std::convert::TryInto;

    #[test]
    fn bank_tx_parse() -> Result<()> {
        let tx: BankTx = "2025-03-06 | XXXX0000 |      60.50 |            | Electrical".parse()?;
        dbg!(&tx);
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
    fn match_journal_entry_multiple_lines() -> Result<()> {
        let mut txs = BankTxs {
            txs: vec![
                "2020-01-03 | XX00 |        |   1000 | Transfer from 003".parse()?,
                "2020-01-02 | XX00 |        |  10000 | Transfer from 001".parse()?,
                "2020-01-02 | XX00 |        |   5000 | Transfer from 002".parse()?,
            ],
            rules: indoc! {r#"
                rule: [eq, account, "XX00"]
                values:
                  bank_account: Bank
            "#}
            .parse()?,
        };
        let entry: Entry = indoc! {"
            ---
            date: 2020-01-02
            memo: Initial Contribution
            debits:
              - account: Bank
                amount: $10,000
              - account: Bank
                amount: $5,000
            credits:
              Owner Contributions: $15,000  
        "}
        .parse()?;

        let matched = txs.match_and_rm(entry);

        dbg!(&matched);
        assert_eq!(matched.len(), 2);
        assert_eq!(txs.txs.len(), 1);
        Ok(())
    }

    #[test]
    fn one_tx_per_match() -> Result<()> {
        let mut txs = BankTxs {
            txs: vec![
                "2020-01-03 | XX00 |        |   1000 | Transfer from 003".parse()?,
                "2020-01-03 | XX00 |        |   1000 | Transfer from 001".parse()?,
            ],
            rules: indoc! {r#"
                rule: [eq, account, "XX00"]
                values:
                  bank_account: Bank
                ---
                rule: [eq, memo, "Transfer From 001"]
                values:
                  offset_account: Owner Contributions
                ---
                rule: [eq, memo, "Transfer From 003"]
                values:
                  offset_account: Owner Contributions
            "#}
            .parse()?,
        };
        let entry: Entry = indoc! {"
            date: 2020-01-03
            debits:
              Bank: $1,000
            credits:
              Owner Contributions: $1,000  
        "}
        .parse()?;

        let matched = txs.match_and_rm(entry);

        dbg!(&matched);
        assert_eq!(matched.len(), 1);
        assert_eq!(txs.txs.len(), 1);
        Ok(())
    }

    #[test]
    fn match_payment_sent() -> Result<()> {
        let mut txs = BankTxs {
            txs: vec![
                "2025-03-06 | XX00 |  60.50 |        | Electrical".parse()?,
                "2025-03-06 | XX00 |   5.00 |        | Misc".parse()?,
                "2025-03-07 | XX00 |        | 310.00 | POS deposit".parse()?,
                "2025-03-07 | XX00 |        |   5.00 | Misc".parse()?,
            ],
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
        .parse()?;

        let matched = txs.match_and_rm(entry);

        dbg!(&matched);
        assert_eq!(matched.len(), 1);
        assert_eq!(txs.txs.len(), 3);
        Ok(())
    }

    #[test]
    fn match_payment_received() -> Result<()> {
        let mut txs = BankTxs {
            txs: vec![
                "2025-03-06 | XX00 |  60.50 |        | Electrical".parse()?,
                "2025-03-06 | XX00 |   5.00 |        | Misc".parse()?,
                "2025-03-07 | XX00 |        | 310.00 | POS deposit".parse()?,
                "2025-03-07 | XX00 |        |   5.00 | Misc".parse()?,
            ],
            rules: indoc! {r#"
                rule: [eq, account, "XX00"]
                values:
                  bank_account: Bank Checking
            "#}
            .parse()?,
        };
        let entry = indoc! {"
            type: Payment Received
            date: 2025-03-07
            party: ACME POS
            account: Bank Checking
            amount: 310.00
        "}
        .parse()?;

        let matched = txs.match_and_rm(entry);

        dbg!(&matched);
        assert_eq!(matched.len(), 1);
        assert_eq!(txs.txs.len(), 3);
        Ok(())
    }

    // TODO test not matching on various fields
    // TODO test invoices with payment
    // TODO implement inexact dates

    #[test]
    fn generate_payment_entries() -> Result<()> {
        let mut txs = BankTxs {
            txs: vec![
                "2025-03-07 | XX00 |        | 310.00 | POS deposit".parse()?,
                "2025-03-08 | XX00 | 60.50  |        | ACME ElecSvc".parse()?,
            ],
            rules: indoc! {r#"
                rule: [eq, account, "XX00"]
                values:
                  bank_account: Bank Checking
                ---
                rule: [eq, memo, "POS deposit"]
                entry:
                  party: ACME POS
                ---
                rule: [match, memo, "ACME Elec*"]
                entry:
                  memo: Electric payment
                  party: ACME Electrical Services
            "#}
            .parse()?,
        };
        let entries = txs.generate_entries()?;

        dbg!(&entries);
        assert_eq!(entries.len(), 2);

        assert_eq!(entries[0].date(), "2025-03-07".parse()?);
        assert_eq!(entries[0].memo(), Some("POS deposit".to_string()));
        assert_eq!(entries[0].party(), Some("ACME POS".to_string()));
        assert_eq!(
            entries[0].amount_of_account("Bank Checking").unwrap(),
            JournalAmount::debit(310.00)?
        );
        assert_eq!(
            entries[0].amount_of_account("Accounts Receivable").unwrap(),
            JournalAmount::credit(310.00)?
        );

        assert_eq!(entries[1].date(), "2025-03-08".parse()?);
        assert_eq!(entries[1].memo(), Some("Electric payment".to_string()));
        assert_eq!(
            entries[1].party(),
            Some("ACME Electrical Services".to_string())
        );
        assert_eq!(
            entries[1].amount_of_account("Bank Checking").unwrap(),
            JournalAmount::credit(60.50)?
        );
        assert_eq!(
            entries[1].amount_of_account("Accounts Payable").unwrap(),
            JournalAmount::debit(60.50)?
        );

        Ok(())
    }

    #[test]
    fn generate_journal_entries() -> Result<()> {
        let mut txs = BankTxs {
            txs: vec![
                "2025-03-01 | XX00 |       | 1000.00 | Transfer from 1234".parse()?,
                "2025-03-02 | XX00 |       |  500.00 | Transfer from 5678".parse()?,
                "2025-03-03 | XX00 | 20.00 |         | Transfer to 1234".parse()?,
            ],
            rules: indoc! {r#"
                rule: [eq, account, "XX00"]
                values:
                  bank_account: Bank Checking
                ---
                rule:
                  - or
                  - [match, memo, "Transfer from 1234"]
                  - [match, memo, "Transfer from 5678"]
                values:
                  offset_account: Capital Account
                entry:
                  memo: contribution
                ---
                rule:
                  - or
                  - [match, memo, "Transfer to 1234"]
                  - [match, memo, "Transfer to 5678"]
                values:
                  offset_account: Capital Account
                entry:
                  memo: withdrawl
            "#}
            .parse()?,
        };
        let entries = txs.generate_entries()?;

        dbg!(&entries);
        assert_eq!(entries.len(), 3);

        assert_eq!(entries[0].date(), "2025-03-01".parse()?);
        assert_eq!(entries[0].memo(), Some("contribution".to_string()));
        assert_eq!(
            entries[0].amount_of_account("Capital Account").unwrap(),
            JournalAmount::credit(1000.00)?
        );
        assert_eq!(
            entries[0].amount_of_account("Bank Checking").unwrap(),
            JournalAmount::debit(1000.00)?
        );

        assert_eq!(entries[1].date(), "2025-03-02".parse()?);
        assert_eq!(entries[1].memo(), Some("contribution".to_string()));
        assert_eq!(
            entries[1].amount_of_account("Capital Account").unwrap(),
            JournalAmount::credit(500.00)?
        );
        assert_eq!(
            entries[1].amount_of_account("Bank Checking").unwrap(),
            JournalAmount::debit(500.00)?
        );

        assert_eq!(entries[2].date(), "2025-03-03".parse()?);
        assert_eq!(entries[2].memo(), Some("withdrawl".to_string()));
        assert_eq!(
            entries[2].amount_of_account("Bank Checking").unwrap(),
            JournalAmount::credit(20.00)?
        );
        assert_eq!(
            entries[2].amount_of_account("Capital Account").unwrap(),
            JournalAmount::debit(20.00)?
        );

        Ok(())
    }

    #[test]
    fn generate_invoice_entries() -> Result<()> {
        let mut txs = BankTxs {
            txs: vec!["2025-03-08 | XX00 | 60.50 | | ACME ElecSvc".parse()?],
            // offset account with party implies invoice entry with payment
            // (invoice entries without payments are not generated by bank txs)
            rules: indoc! {r#"
                rule: [eq, account, "XX00"]
                values:
                  bank_account: Bank Checking
                ---
                rule: [match, memo, "ACME Elec*"]
                values:
                  offset_account: Utilities
                entry:
                  memo: Electric bill
                  party: ACME Electrical Services
            "#}
            .parse()?,
        };
        let entries = txs.generate_entries()?;

        dbg!(&entries);
        assert_eq!(entries.len(), 1);

        assert_eq!(entries[0].date(), "2025-03-08".parse()?);
        assert_eq!(entries[0].memo(), Some("Electric bill".to_string()));
        assert_eq!(
            entries[0].party(),
            Some("ACME Electrical Services".to_string())
        );
        assert_eq!(
            entries[0].amount_of_account("Utilities").unwrap(),
            JournalAmount::debit(60.50)?
        );
        assert_eq!(
            entries[0].amount_of_account("Bank Checking").unwrap(),
            JournalAmount::credit(60.50)?
        );

        Ok(())
    }
}
