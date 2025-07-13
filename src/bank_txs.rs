use crate::{
    entry::journal::{JournalAccount, JournalAmount},
    money::Money,
};
use anyhow::{anyhow, Context, Error, Result};
use async_std::fs::File;
use async_std::io::BufReader;
use async_std::prelude::*;
use chrono::NaiveDate;
use futures::{future, TryStreamExt};
use std::str::FromStr;

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
pub struct BankTxs(Vec<BankTx>);

impl BankTxs {
    pub async fn from_file(file: &str) -> Result<Self> {
        let file = File::open(file).await?;
        let txs: Vec<BankTx> = BufReader::new(file)
            .lines()
            .map_err(Error::new) // map to anyhow::Error from here on
            .and_then(|line| future::ready(line.parse()))
            .try_collect()
            .await?;
        Ok(Self(txs))
    }
}

pub struct ReconciliationRules();

#[cfg(test)]
mod bank_txs_tests {
    use anyhow::Result;
    use std::convert::TryInto;

    use super::*;

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
}
