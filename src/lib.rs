pub mod account;
pub mod bank_txs;
pub mod chart_of_accounts;
pub mod entry;
pub mod lines;
pub mod money;
pub mod report;

use anyhow::{Error, Result};
use chart_of_accounts::ChartOfAccounts;
use entry::Entry;
use entry::journal::{JournalAccount, JournalAmount, JournalEntry, JournalLine};
use futures::future::{self, Future};
use futures::stream::{self, Stream, TryStreamExt};
use futures::{StreamExt, TryStream};
use lines::lines;
use lines_ext::LinesExt;
use report::ReportNode;
use std::borrow::ToOwned;
use std::collections::HashMap;
use std::ops::AddAssign;
use std::pin::Pin;

// TODO explore using pinned stream instead of attaching everything to this struct
pub struct Ledger {
    path: Option<String>,
}

type Balances = HashMap<JournalAccount, JournalAmount>;

type EntriesStream = Pin<Box<dyn Stream<Item = Result<Entry>> + Send>>;
type LinesStream = Pin<Box<dyn Stream<Item = Result<String, std::io::Error>> + Send>>;
pub fn entries_from_lines(
    lines_stream: LinesStream,
    // filter by account
    account: Option<String>,
    // filter by party
    party: Option<String>,
) -> EntriesStream {
    // let l = lines(Some("".to_string()));
    let stream = lines_stream
        // remove lines starting with #
        .try_filter(|s| future::ready(!s.to_owned().trim().starts_with("#")))
        .chunk_by_line("---")
        // remove any empty chunks
        .try_filter(|s| future::ready(!s.trim().is_empty()))
        .map_err(Error::new) // map to anyhow::Error from here on
        .and_then(|doc| future::ready(doc.parse()))
        // filter by party
        .try_filter(move |entry: &Entry| {
            future::ready(
                party
                    .clone()
                    .is_none_or(|pf| entry.party().is_some_and(|pe| pe == pf)),
            )
        })
        // filter by account
        .try_filter(move |entry: &Entry| {
            future::ready(
                account
                    .clone()
                    .is_none_or(|af| entry.amount_of_account(&af).is_some()),
            )
        });
    Box::pin(stream)
}

impl Ledger {
    // TODO consider making this accept enum for source for stdin, path, or string
    pub fn new(dir: Option<&str>) -> Self {
        Ledger {
            path: dir.map(ToOwned::to_owned),
        }
    }

    /// Parse own stream of lines into `Entry`s
    pub fn entries(
        &self,
    ) -> impl TryStream<Item = Result<Entry>, Ok = Entry, Error = anyhow::Error> + '_ {
        self.entries_filtered(None, None)
    }

    /// Parse own stream of lines into `Entry`s
    pub fn entries_filtered(
        &self,
        // filter by account
        account: Option<String>,
        // filter by party
        party: Option<String>,
    ) -> impl TryStream<Item = Result<Entry>, Ok = Entry, Error = anyhow::Error> + '_ {
        entries_from_lines(Box::pin(lines(self.path.clone())), account, party)
    }

    /// Convert own stream of `Entry`s into `JournalEntry`s
    pub fn journal(
        &self,
    ) -> impl TryStream<Item = Result<JournalEntry>, Ok = JournalEntry, Error = anyhow::Error> + '_
    {
        self.journal_filtered(None, None)
    }

    /// Convert own stream of `Entry`s into `JournalEntry`s
    pub fn journal_filtered(
        &self,
        // filter by account
        account: Option<String>,
        // filter by party
        party: Option<String>,
    ) -> impl TryStream<Item = Result<JournalEntry>, Ok = JournalEntry, Error = anyhow::Error> + '_
    {
        self.entries_filtered(account, party)
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

    /// Run report to get total breakdowns of own balances based on given `ChartOfAccounts` and report spec
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

    // pub fn reconcile(&self, account: JournalAccount, mut txs: BankTxs) {
    //     dbg!(&account, &txs);
    //     self.entries()
    //         .try_filter(|entry| async move { entry.amount_of_account(account.as_str()).is_some() })
    //         .try_for_each(|entry: Entry| async {
    //             if let Some(tx) = txs.match_and_rm(entry.clone()) {
    //                 println!("Tx:\n{tx:?}\nMatched with entry:\n{entry:?}");
    //             }
    //             // try to match each entry
    //             // if !txs.match_and_rm(entry) {
    //             //     // emit entry not found in bank for reconcilliation report
    //             // }
    //             // emit unmatch bank txs as new entries
    //             Ok(())
    //         });
    // }
}

#[cfg(test)]
mod entry_tests {
    use super::*;
    
    use async_std::stream::StreamExt;
    use indoc::indoc;

    const ENTRIES_STR: &str = indoc! {"
        ---
        date: 2020-01-02
        credits:
          Owner Contributions: $100.00  
        debits:
          Bank Checking: $100.00
        ---
        type: Purchase Invoice
        date: 2020-01-03
        party: ACME Electrical 
        account: Operating Expenses
        amount: 60.50
        ---
        type: Payment Sent
        date: 2020-01-04
        party: ACME Electrical 
        account: Bank Checking
        amount: 60.50
    "};

    #[async_std::test]
    async fn entries_from_lines_test() -> Result<()> {
        let lines = Box::pin(stream::iter(
            ENTRIES_STR
                .lines()
                .map(String::from)
                .map(std::io::Result::Ok),
        ));

        let entries = entries_from_lines(lines, None, None)
            .try_collect::<Vec<Entry>>()
            .await?;

        dbg!(&entries);
        assert_eq!(
            entries
                .iter()
                .map(|e| e.date().to_string())
                .collect::<Vec<String>>(),
            vec!["2020-01-02", "2020-01-03", "2020-01-04"]
        );
        Ok(())
    }

    #[async_std::test]
    async fn entries_from_lines_test_account_filter() -> Result<()> {
        let lines = Box::pin(stream::iter(
            ENTRIES_STR
                .lines()
                .map(String::from)
                .map(std::io::Result::Ok),
        ));

        let entries = entries_from_lines(lines, Some("Bank Checking".to_string()), None)
            .try_collect::<Vec<Entry>>()
            .await?;

        dbg!(&entries);
        assert_eq!(
            entries
                .iter()
                .map(|e| e.date().to_string())
                .collect::<Vec<String>>(),
            vec!["2020-01-02", "2020-01-04"]
        );
        Ok(())
    }

    #[async_std::test]
    async fn entries_from_lines_test_party_filter() -> Result<()> {
        let lines = Box::pin(stream::iter(
            ENTRIES_STR
                .lines()
                .map(String::from)
                .map(std::io::Result::Ok),
        ));

        let entries = entries_from_lines(lines, None, Some("ACME Electrical".to_string()))
            .try_collect::<Vec<Entry>>()
            .await?;

        dbg!(&entries);
        assert_eq!(
            entries
                .iter()
                .map(|e| e.date().to_string())
                .collect::<Vec<String>>(),
            vec!["2020-01-03", "2020-01-04"]
        );
        Ok(())
    }

    #[async_std::test]
    async fn entries_from_lines_test_account_and_party_filter() -> Result<()> {
        let lines = Box::pin(stream::iter(
            ENTRIES_STR
                .lines()
                .map(String::from)
                .map(std::io::Result::Ok),
        ));

        let entries = entries_from_lines(
            lines,
            Some("Bank Checking".to_string()),
            Some("ACME Electrical".to_string()),
        )
        .try_collect::<Vec<Entry>>()
        .await?;

        dbg!(&entries);
        assert_eq!(
            entries
                .iter()
                .map(|e| e.date().to_string())
                .collect::<Vec<String>>(),
            vec!["2020-01-04"]
        );
        Ok(())
    }
}
