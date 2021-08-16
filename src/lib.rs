pub mod account;
pub mod chart_of_accounts;
pub mod entry;
pub mod journal_entry;
pub mod money;

use anyhow::{Context, Error, Result};
use async_std::fs::File;
use async_std::io::prelude::*;
use async_std::io::{stdin, BufReader};
use async_walkdir::{DirEntry, WalkDir};
use chart_of_accounts::ChartOfAccounts;
use entry::raw_entry::RawEntry;
use entry::Entry;
use futures::stream::{self, StreamExt, TryStream, TryStreamExt};
use journal_entry::{JournalAccount, JournalAmount, JournalEntry};
use lines_ext::LinesExt;
use std::borrow::ToOwned;
use std::collections::HashMap;
use std::convert::TryInto;
use std::io::ErrorKind;
use std::ops::AddAssign;

pub struct Ledger {
    pub chart_of_accounts: ChartOfAccounts,
    dir: Option<String>,
}

impl Ledger {
    pub fn new(dir: Option<&str>) -> Self {
        Ledger {
            chart_of_accounts: ChartOfAccounts::new(),
            dir: dir.map(ToOwned::to_owned),
        }
    }

    fn dir_lines(
        dir: String,
    ) -> impl TryStream<Item = std::io::Result<String>, Error = std::io::Error, Ok = String> {
        WalkDir::new(dir)
            .try_filter_map(|dir_entry: DirEntry| async move {
                let path = dir_entry.path();
                let filestem = path
                    .file_stem()
                    .ok_or(std::io::Error::new(ErrorKind::Other, "No file stem"))?
                    .to_string_lossy();
                if path.is_dir() || filestem.starts_with(".") {
                    return Ok(None);
                };
                File::open(&path).await.map(Option::Some)
            })
            .map_ok(|file| BufReader::new(file).lines())
            .try_flatten()
    }

    fn lines(
        &self,
    ) -> impl TryStream<Item = std::io::Result<String>, Error = std::io::Error, Ok = String> + '_
    {
        if let Some(dir) = self.dir.clone() {
            Self::dir_lines(dir.clone()).left_stream()
        } else {
            BufReader::new(stdin()).lines().right_stream()
        }
    }

    pub fn entries(&self) -> impl TryStream<Item = Result<Entry>, Error = Error, Ok = Entry> + '_ {
        self.lines()
            .chunk_by_line("---")
            .map_err(|err: std::io::Error| Error::new(err)) // map to anyhow::Error from here on
            .and_then(|doc: String| async move {
                let mut raw_entry: RawEntry = serde_yaml::from_str(doc.as_str())
                    .context(format!("Failed to deserialize entry:\n{:?}", doc))?;
                raw_entry.id.get_or_insert(format!(
                    "{}-{}-{}-{}",
                    raw_entry.date, raw_entry.r#type, raw_entry.party, raw_entry.account
                ));
                let entry: Entry = raw_entry.try_into()?;
                Ok(entry)
            })
    }

    pub fn journal(&self) -> impl TryStream<Ok = JournalEntry, Error = Error> + '_ {
        self.entries()
            .and_then(move |entry| {
                let journal_entry = JournalEntry::from_entry(entry, None);
                async {
                    let stream = stream::iter(journal_entry?).map(|x| Ok(x));
                    Ok(stream)
                }
            })
            .try_flatten()
    }

    pub async fn balances(&self) -> Result<HashMap<JournalAccount, JournalAmount>> {
        let balance = self
            .journal()
            .try_fold(
                HashMap::new(),
                |mut acc, JournalEntry(_, account, amount)| async move {
                    // dbg!((&account, acc.get(&account), &amount));
                    acc.entry(account.clone())
                        .and_modify(|total: &mut JournalAmount| {
                            total.add_assign(amount);
                        })
                        .or_insert({
                            let mut new = JournalAmount::new();
                            new.add_assign(amount);
                            new
                        });
                    Ok(acc)
                },
            )
            .await?;
        Ok(balance)
    }
}
