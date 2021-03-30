pub mod account;
pub mod chart_of_accounts;
pub mod entry;
pub mod journal_entry;
pub mod money;

use account::Account;
use anyhow::{Context, Error, Result};
use async_std::fs;
use async_walkdir::{DirEntry, WalkDir};
use chart_of_accounts::ChartOfAccounts;
use entry::raw_entry::RawEntry;
use entry::Entry;
use futures::stream::{self, StreamExt, TryStreamExt};
use journal_entry::JournalEntry;
use money::Money;
use num_traits::identities::Zero;
use std::collections::HashMap;
use std::convert::TryInto;

pub struct Ledger {
    pub chart_of_accounts: ChartOfAccounts,
    dir: String,
}

impl Ledger {
    pub fn new(dir: &str) -> Self {
        Ledger {
            chart_of_accounts: ChartOfAccounts::new(),
            dir: dir.to_owned(),
        }
    }

    pub async fn entries(&self) -> Result<impl TryStreamExt<Ok = Entry, Error = Error> + '_> {
        Ok(WalkDir::new(&self.dir)
            .map_err(Error::new) // map to anyhow::Error from here o
            .try_filter_map(move |dir_entry: DirEntry| {
                // let dir = dir.clone();
                async move {
                    let path = dir_entry.path();
                    let filestem = path
                        .file_stem()
                        .context("can't get filestem")?
                        .to_string_lossy();
                    if path.is_dir() || filestem.starts_with(".") {
                        return Ok(None);
                    };

                    let content = fs::read_to_string(&path).await?;
                    let docs: Vec<String> = content
                        .trim_start_matches("---")
                        .split("\n---\n")
                        // only way I can tell to avoid returning reference to content
                        .map(ToOwned::to_owned)
                        .collect();
                    let filestem = filestem.into_owned();
                    let sub_stream =
                        stream::iter(docs)
                            .enumerate()
                            .map(move |(index, yaml)| -> Result<_> {
                                let mut subpath = path.strip_prefix(&self.dir)?.to_owned();
                                subpath.pop();
                                subpath.push(filestem.clone());
                                Ok((subpath, index, yaml))
                            });

                    Ok(Some(sub_stream))
                }
            })
            .try_flatten()
            .and_then(|(path, index, yaml)| async move {
                let mut raw_entry: RawEntry = serde_yaml::from_str(&yaml)
                    .context(format!("Failed to deserialize entry in {:?}", path))?;
                raw_entry
                    .id
                    .get_or_insert(format!("{}-{}", path.to_string_lossy(), index));
                let entry: Entry = raw_entry.try_into()?;
                Ok(entry)
            }))
    }

    pub async fn journal(
        &self,
    ) -> Result<impl TryStreamExt<Ok = JournalEntry, Error = Error> + '_> {
        let stream = self
            .entries()
            .await?
            .and_then(move |entry| {
                let journal_entry = JournalEntry::from_entry(entry, &self.chart_of_accounts);
                async {
                    let stream = stream::iter(journal_entry?).map(|x| Ok(x));
                    Ok(stream)
                }
            })
            .try_flatten();
        Ok(stream)
    }

    pub async fn balances(&self) -> Result<HashMap<Account, Money>> {
        let balance = self
            .journal()
            .await?
            .try_fold(
                HashMap::new(),
                |mut acc, JournalEntry(_, account, amount)| async move {
                    dbg!((&account, acc.get(&account), &amount));
                    acc.entry(account.clone())
                        .and_modify(|total| account.add(total, amount.clone()))
                        .or_insert({
                            let mut zero = Money::zero();
                            account.add(&mut zero, amount);
                            zero
                        });
                    Ok(acc)
                },
            )
            .await?;
        Ok(balance)
    }
}
