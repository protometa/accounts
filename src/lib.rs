pub mod account;
pub mod chart_of_accounts;
pub mod entry;
pub mod journal_entry;
pub mod money;
pub mod report;

use anyhow::{Error, Result};
use async_std::fs::File;
use async_std::io::prelude::*;
use async_std::io::{stdin, BufReader};
use async_walkdir::{DirEntry, WalkDir};
use chart_of_accounts::ChartOfAccounts;
use entry::Entry;
use futures::future::{self, Future};
use futures::stream::{self, Stream, StreamExt, TryStreamExt};
use journal_entry::{JournalAccount, JournalAmount, JournalEntry};
use lines_ext::LinesExt;
use report::ReportNode;
use std::borrow::ToOwned;
use std::collections::HashMap;
use std::io::ErrorKind;
use std::ops::AddAssign;

pub struct Ledger {
    dir: Option<String>,
}

type Balances = HashMap<JournalAccount, JournalAmount>;

impl Ledger {
    pub fn new(dir: Option<&str>) -> Self {
        Ledger {
            dir: dir.map(ToOwned::to_owned),
        }
    }

    /// Reads an entire dir of files by line
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

    /// Reads lines of self.dir or stdin if None
    fn lines(&self) -> impl Stream<Item = std::io::Result<String>> + '_ {
        if let Some(dir) = self.dir.clone() {
            Self::dir_lines(dir).left_stream()
        } else {
            BufReader::new(stdin()).lines().right_stream()
        }
    }

    /// Parse own stream of lines into `Entry`s
    pub fn entries(&self) -> impl Stream<Item = Result<Entry>> + '_ {
        self.lines()
            .chunk_by_line("---")
            .map_err(Error::new) // map to anyhow::Error from here on
            .and_then(|doc| future::ready(doc.parse()))
    }

    /// Convert own stream of `Entry`s into `JournalEntry`s
    pub fn journal(&self) -> impl Stream<Item = Result<JournalEntry>> + '_ {
        self.entries()
            .and_then(|entry| async {
                Ok(stream::iter(JournalEntry::from_entry(entry, None)?).map(Ok))
            })
            .try_flatten()
    }

    /// Get balances for each account appearing in own stream of `JournalEntry`s
    pub fn balances(&self) -> impl Future<Output = Result<Balances>> + '_ {
        self.journal().try_fold(
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
    }

    /// Run report to get total breakdowns of own balances based on give `ChartOfAccounts` and report spec
    pub async fn run_report<'a>(
        &'a self,
        chart: &ChartOfAccounts,
        report: &'a mut ReportNode,
    ) -> Result<&'a mut ReportNode> {
        self.balances()
            .await?
            .iter()
            .try_fold(report, |report, (account, balance)| {
                // recursively find total in report to which account applies and add name to list and value to total
                let account = chart.get(account)?;
                report.apply_balance((account, balance))?;
                Ok(report)
            })
    }
}
