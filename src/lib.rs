pub mod account;
pub mod bank_txs;
pub mod chart_of_accounts;
pub mod entry;
mod lines;
pub mod money;
pub mod report;

use anyhow::{Error, Result};
use bank_txs::{BankTxs, ReconciliationRules};
use chart_of_accounts::ChartOfAccounts;
use entry::journal::{JournalAccount, JournalAmount, JournalEntry, JournalLine};
use entry::Entry;
use futures::future::{self, Future};
use futures::stream::{self, Stream, StreamExt, TryStreamExt};
use lines::lines;
use lines_ext::LinesExt;
use report::ReportNode;
use std::borrow::ToOwned;
use std::collections::HashMap;
use std::ops::AddAssign;

pub struct Ledger {
    path: Option<String>,
}

type Balances = HashMap<JournalAccount, JournalAmount>;

impl Ledger {
    pub fn new(dir: Option<&str>) -> Self {
        Ledger {
            path: dir.map(ToOwned::to_owned),
        }
    }

    /// Parse own stream of lines into `Entry`s
    pub fn entries(&self) -> impl Stream<Item = Result<Entry>> + '_ {
        lines(self.path.clone())
            .chunk_by_line("---")
            .map_err(Error::new) // map to anyhow::Error from here on
            .and_then(|doc| future::ready(doc.parse()))
    }

    /// Convert own stream of `Entry`s into `JournalEntry`s
    pub fn journal(&self) -> impl Stream<Item = Result<JournalEntry>> + '_ {
        self.entries()
            .and_then(
                |entry| async move { Ok(stream::iter(entry.to_journal_entries(None)?).map(Ok)) },
            )
            .try_flatten()
    }

    /// Get balances for each account appearing in own stream of `JournalEntry`s
    pub fn balances(&self) -> impl Future<Output = Result<Balances>> + '_ {
        // TODO: work on set of given JournalLines and use for payable/recievable too
        self.journal()
            .and_then(|entry| async move { Ok(stream::iter(entry.lines()).map(Ok)) })
            .try_flatten()
            .try_fold(
                HashMap::new(),
                |mut acc, JournalLine(account, amount)| async move {
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

    pub fn payable(&self)
    // -> impl Future<Output = Result<HashMap<String, JournalAmount>>> + '_
    {
        unimplemented!("This function is not yet implemented");
        // self.journal().try_fold(
        //     HashMap::new(),
        //     |mut acc, JournalEntry(_, account, amount, party)| async move {
        //         if account == "Accounts Payable" {
        //             if let Some(party) = party {
        //                 acc.entry(party)
        //                     .and_modify(|total: &mut JournalAmount| {
        //                         total.add_assign(amount);
        //                     })
        //                     .or_insert(amount);
        //             }
        //         }
        //         Ok(acc)
        //     },
        // )
    }

    pub fn receivable(&self)
    // -> impl Future<Output = Result<HashMap<String, JournalAmount>>> + '_
    {
        unimplemented!("This function is not yet implemented");
        // self.journal().try_fold(
        //     HashMap::new(),
        //     |mut acc, JournalEntry(_, account, amount, party)| async move {
        //         if account == "Accounts Receivable" {
        //             if let Some(party) = party {
        //                 acc.entry(party)
        //                     .and_modify(|total: &mut JournalAmount| {
        //                         total.add_assign(amount);
        //                     })
        //                     .or_insert(amount);
        //             }
        //         }
        //         Ok(acc)
        //     },
        // )
    }

    pub fn reconcile(&self, txs: BankTxs, rules: ReconciliationRules) {
        dbg!(txs);
        // self.entries().for_each(|entry: Entry| {
        //     // try to match each entry
        //     // if !txs.match_and_rm(entry) {
        //     //     // emit entry not found in bank for reconcilliation report
        //     // }
        //     // emit unmatch bank txs as new entries
        // });
    }
}
