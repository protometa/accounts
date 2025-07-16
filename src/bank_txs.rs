use crate::{
    entry::{
        journal::{JournalAccount, JournalAmount, JournalEntry, JournalLine},
        Entry,
    },
    money::Money,
};
use anyhow::{anyhow, Context, Error, Result};
use async_std::io::BufReader;
use async_std::prelude::*;
use async_std::{fs::File, path::Iter};
use chrono::NaiveDate;
use futures::{future, TryStreamExt};
use std::{borrow::Borrow, collections::HashMap, str::FromStr, thread::AccessError};

#[derive(Debug, PartialEq)]
struct BankTx {
    date: NaiveDate,
    account: JournalAccount,
    amount: JournalAmount,
    memo: String,
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
        if self.txs.is_empty() {
            return None;
        }
        if let Some(i) = self.txs.iter().position(|tx| {
            tx.date == entry.clone().date()
                && entry.lines().iter().any(|JournalLine(account, amount)| {
                    self.rules
                        .corresponding_account(&tx.account)
                        .map(|ca| &ca == account && &tx.amount.invert() == amount)
                        .unwrap_or(false)
                })
        }) {
            return Some(self.txs.swap_remove(i));
        };
        None
    }

    /// After matching and removing txs, used to generate new entries from remaining txs
    pub fn generate_entries(&mut self) {
        todo!()
        // rules to decide to make it payment entry or journal entry
        // if payment entry, rules for party from memo, date, amount
        // if journal entry, rules for counter account from memo, date, amount
        // templating of memo
    }
}

#[derive(Debug, Default)]
pub struct ReconciliationRules {
    account_map: HashMap<JournalAccount, JournalAccount>,
}

impl ReconciliationRules {
    fn corresponding_account(&self, account: &JournalAccount) -> Option<JournalAccount> {
        self.account_map.get(account).cloned()
    }
}

#[cfg(test)]
mod bank_txs_tests {
    use super::*;
    use anyhow::Result;
    use indoc::indoc;
    use std::{collections::HashMap, convert::TryInto};

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
            txs: vec!["2025-03-06 | X0 |  60.50 |        | Electrical".parse()?],
            rules: ReconciliationRules {
                account_map: HashMap::from([("X0".to_string(), "Bank Checking".to_string())]),
            },
        };
        let matched = txs.match_and_rm(
            indoc! {"
                type: Payment Sent
                date: 2025-03-06
                party: ACME Electrical 
                memo: Operating Expenses
                account: Bank Checking
                amount: 60.50
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

    #[test]
    fn match_payment_received() -> Result<()> {
        let mut txs = BankTxs {
            txs: vec!["2025-03-07 | X0 |        | 310.00 | POS deposit".parse()?],
            rules: ReconciliationRules {
                account_map: HashMap::from([("X0".to_string(), "Bank Checking".to_string())]),
            },
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

    // TODO test invoices with payment

    // TODO implement inexact dates
    // TODO test entry generation from rules
}
