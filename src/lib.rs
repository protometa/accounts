pub mod account;
pub mod chart_of_accounts;
pub mod entry;
pub mod journal_entry;
pub mod money;

use anyhow::{Error, Result};
use async_std::fs::File;
use async_std::io::prelude::*;
use async_std::io::{stdin, BufReader};
use async_walkdir::{DirEntry, WalkDir};
use entry::Entry;
use futures::future;
use futures::stream::{self, Stream, StreamExt, TryStreamExt};
use journal_entry::{JournalAccount, JournalAmount, JournalEntry};
use lines_ext::LinesExt;
use std::borrow::ToOwned;
use std::collections::HashMap;
use std::io::ErrorKind;
use std::ops::AddAssign;

pub struct Ledger {
    dir: Option<String>,
}

impl Ledger {
    pub fn new(dir: Option<&str>) -> Self {
        Ledger {
            dir: dir.map(ToOwned::to_owned),
        }
    }

    fn dir_lines(dir: String) -> impl Stream<Item = std::io::Result<String>> {
        WalkDir::new(dir)
            .try_filter_map(|dir_entry: DirEntry| async move {
                let path = dir_entry.path();
                let filestem = path
                    .file_stem()
                    .ok_or_else(|| std::io::Error::new(ErrorKind::Other, "No file stem"))?
                    .to_string_lossy();
                if path.is_dir() || filestem.starts_with('.') {
                    return Ok(None);
                };
                File::open(&path).await.map(Option::Some)
            })
            .map_ok(|file| BufReader::new(file).lines())
            .try_flatten()
    }

    fn lines(&self) -> impl Stream<Item = std::io::Result<String>> + '_ {
        if let Some(dir) = self.dir.clone() {
            Self::dir_lines(dir).left_stream()
        } else {
            BufReader::new(stdin()).lines().right_stream()
        }
    }

    pub fn entries(&self) -> impl Stream<Item = Result<Entry>> + '_ {
        self.lines()
            .chunk_by_line("---")
            .map_err(Error::new) // map to anyhow::Error from here on
            .and_then(|doc| future::ready(doc.parse()))
    }

    pub fn journal(&self) -> impl Stream<Item = Result<JournalEntry>> + '_ {
        self.entries()
            .and_then(|entry| async {
                Ok(stream::iter(JournalEntry::from_entry(entry, None)?).map(Ok))
            })
            .try_flatten()
    }

    pub async fn balances(&self) -> Result<HashMap<JournalAccount, JournalAmount>> {
        self.journal()
            .try_fold(
                HashMap::new(),
                |mut acc, JournalEntry(_, account, amount)| async move {
                    acc.entry(account.clone())
                        .and_modify(|total: &mut JournalAmount| {
                            total.add_assign(amount);
                        })
                        .or_insert(amount);
                    Ok(acc)
                },
            )
            .await
    }
}
