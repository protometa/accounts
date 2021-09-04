use super::account::*;
use anyhow::{anyhow, Error, Result};
use async_std::fs::File;
use async_std::io::BufReader;
use async_std::prelude::*;
use futures::{future, TryStreamExt};
use lines_ext::LinesExt;

pub type AccountId = usize;

#[derive(Debug)]
pub struct ChartOfAccounts(Vec<Account>);

impl ChartOfAccounts {
    pub async fn from_file(file: &str) -> Result<Self> {
        let file = File::open(file).await?;
        let accounts: Vec<Account> = BufReader::new(file)
            .lines()
            .chunk_by_line("---")
            .map_err(Error::new) // map to anyhow::Error from here on
            .and_then(|doc| future::ready(doc.parse()))
            .try_collect()
            .await?;
        Ok(ChartOfAccounts(accounts))
    }

    pub fn get(&self, name: &str) -> Result<&Account> {
        self.0
            .iter()
            .find(|account| account.name == name)
            .ok_or(anyhow!("Account {} not found", name))
    }
}
