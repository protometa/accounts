use super::account::*;
use anyhow::{Error, Result};
use async_std::fs::File;
use async_std::io::BufReader;
use async_std::prelude::*;
use futures::{StreamExt, TryStreamExt, future, stream};
use itertools::Itertools;
use lines_ext::LinesExt;

pub type AccountId = usize;

#[derive(Debug, Clone)]
pub struct ChartOfAccounts(Vec<Account>);

impl ChartOfAccounts {
    pub async fn from_file(file: &str) -> Result<Self> {
        let file = File::open(file).await?;
        // read lines stream from file
        ChartOfAccounts::from_lines(BufReader::new(file).lines()).await
    }

    pub async fn from_str(s: &str) -> Result<Self> {
        let ls: Vec<_> = s
            .lines()
            .map(String::from)
            .map(std::io::Result::Ok)
            .collect();
        ChartOfAccounts::from_lines(stream::iter(ls).boxed()).await
    }

    pub async fn from_lines<'a>(
        lines: impl Stream<Item = Result<String, std::io::Error>> + Send + 'a,
    ) -> Result<Self> {
        // read and parse lines
        let mut accounts: Vec<Account> = lines
            // remove lines starting with #
            .try_filter(|s| future::ready(!s.trim().starts_with("#")))
            .chunk_by_line("---")
            // remove any empty chunks
            .try_filter(|s| future::ready(!s.trim().is_empty()))
            .map_err(Error::new) // map to anyhow::Error from here on
            .and_then(|doc| future::ready(doc.parse()))
            .try_collect()
            .await?;
        // sort
        // TODO throw error if account names or nums not unique
        accounts = accounts
            .into_iter()
            .enumerate()
            .map(|(i, mut account)| {
                account.index = Some(i);
                account
            })
            .sorted_by_key(|account| {
                (
                    account.class,
                    account.num.unwrap_or(usize::MAX),
                    account.index,
                )
            })
            // re-index based on overall sort
            .enumerate()
            .map(|(i, mut account)| {
                account.index = Some(i);
                account
            })
            .collect();
        Ok(ChartOfAccounts(accounts))
    }

    pub fn get(&self, name: &str) -> Option<&Account> {
        self.0.iter().find(|account| account.name == name)
    }
}
