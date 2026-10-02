pub mod account;
pub mod bank_txs;
pub mod chart_of_accounts;
pub mod entry;
pub mod lines;
pub mod money;
pub mod render_table;
pub mod report;

use account::Account;
use anyhow::{Context, Error, Result};
use chart_of_accounts::ChartOfAccounts;
use chrono::NaiveDate;
use entry::journal::{JournalAccount, JournalAmount, JournalEntry, JournalLine};
use entry::{Entry, JournalEntryIterator};
use futures::future::OptionFuture;
use futures::future::{self, Future};
use futures::stream::{self, BoxStream, TryStreamExt};
use futures::{Stream, StreamExt};
use itertools::Itertools;
use lines::lines;
use lines_ext::LinesExt;
use money::Money;
use report::ReportNode;
use std::borrow::ToOwned;
use std::collections::HashMap;
use std::iter::Peekable;
use std::ops::AddAssign;

#[derive(Debug, Clone)]
pub enum DocSource {
    Stdin,
    Path(String),
    Str(String),
}

pub struct Accounts {
    journal: DocSource,
    end: Option<NaiveDate>,
    chart: Option<ChartOfAccounts>,
}

// Simple balances of account names to amounts with possible full account metadata
// TODO perhaps add balance as field on account and just make this a list
type Balances = HashMap<JournalAccount, (JournalAmount, Option<Account>)>;

pub fn entries_from_lines<'a>(
    lines_stream: impl Stream<Item = Result<String, std::io::Error>> + Send + 'a,
    // filter by account
    account: Option<String>,
    // filter by party
    party: Option<String>,
) -> BoxStream<'a, Result<Entry>> {
    lines_stream
        // remove lines starting with #
        .try_filter(|s| future::ready(!s.to_owned().trim().starts_with("#")))
        .chunk_by_line("---")
        // remove any empty chunks
        .try_filter(|s| future::ready(!s.trim().is_empty()))
        .map_err(Error::new) // map to anyhow::Error from here on
        .and_then(|doc| future::ready(doc.parse()))
        // filter by account
        .try_filter(move |entry: &Entry| {
            future::ready(
                account
                    .clone()
                    .is_none_or(|af| entry.amount_of_account(&af).is_some()),
            )
        })
        // filter by party
        .try_filter(move |entry: &Entry| {
            future::ready(
                party
                    .clone()
                    .is_none_or(|pf| entry.party().is_some_and(|pe| pe == pf)),
            )
        })
        .boxed()
}

/// Get ledger of given account from journal entries
fn ledger_from_journal(
    journal_entries: BoxStream<Result<JournalEntry>>,
    account: String,
) -> BoxStream<Result<LedgerLine>> {
    journal_entries
        // flatten to ledger lines
        .map_ok(move |entry| {
            stream::iter(entry.lines().into_iter().filter_map({
                let account = account.clone();
                move |line| {
                    if line.0 == account {
                        Some(Ok(LedgerLine {
                            date: entry.date(),
                            memo: entry.memo(),
                            amount: line.1,
                            running_total: Default::default(),
                        }))
                    } else {
                        None
                    }
                }
            }))
        })
        .try_flatten()
        // scan ledger lines for running_total
        .scan(JournalAmount::default(), |acc, line| {
            // TODO maybe make running total error if there are any errors encountered
            future::ready(Some(line.map(|line| {
                *acc += line.amount;
                LedgerLine {
                    running_total: *acc,
                    ..line
                }
            })))
        })
        .boxed()
}

fn balances_from_journal_lines(
    lines: BoxStream<Result<JournalLine>>,
) -> impl Future<Output = Result<Balances>> {
    lines.try_fold(
        HashMap::new(),
        async |mut acc, JournalLine(account, amount)| {
            acc.entry(account.clone())
                .and_modify(|(total, _): &mut (JournalAmount, _)| {
                    total.add_assign(amount);
                })
                .or_insert((amount, None));
            Ok(acc)
        },
    )
}

#[derive(Debug, Clone)]
pub struct LedgerLine {
    pub date: NaiveDate,
    pub memo: Option<String>,
    pub amount: JournalAmount,
    pub running_total: JournalAmount,
}

impl Accounts {
    pub async fn new(
        journal: DocSource,
        end: Option<NaiveDate>,
        chart: Option<DocSource>,
    ) -> Result<Self> {
        let chart: OptionFuture<_> = chart
            .map(async |chart| match chart {
                DocSource::Path(file) => ChartOfAccounts::from_file(&file).await,
                DocSource::Str(s) => ChartOfAccounts::from_str(&s).await,
                DocSource::Stdin => unimplemented!(),
            })
            .into();
        Ok(Accounts {
            journal,
            end,
            chart: chart.await.transpose()?,
        })
    }

    /// Parse own stream of lines into `Entry`s
    pub fn entries(&self) -> BoxStream<Result<Entry>> {
        self.entries_filtered(None, None)
    }

    pub fn chart(&self) -> Option<&ChartOfAccounts> {
        self.chart.as_ref()
    }

    pub fn account(&self, name: &str) -> Option<&Account> {
        self.chart.as_ref().and_then(|c| c.get(name))
    }

    /// Parse own stream of lines into `Entry`s
    pub fn entries_filtered(
        &self,
        // filter by account
        account: Option<String>,
        // filter by party
        party: Option<String>,
    ) -> BoxStream<Result<Entry>> {
        match self.journal.clone() {
            DocSource::Stdin => entries_from_lines(lines(None), account, party),
            DocSource::Path(path) => entries_from_lines(lines(Some(path.clone())), account, party),
            DocSource::Str(s) => {
                // TODO maybe separate out the part that works with docs
                let ls: Vec<_> = s
                    .lines()
                    .map(String::from)
                    .map(std::io::Result::Ok)
                    .collect();
                entries_from_lines(stream::iter(ls).boxed(), account, party)
            }
        }
    }

    /// Convert own stream of `Entry`s into `JournalEntry`s
    pub fn journal(&self) -> BoxStream<Result<JournalEntry>> {
        self.journal_filtered(None, None)
    }

    /// Convert own stream of `Entry`s into `JournalEntry`s
    pub fn journal_filtered(
        &self,
        // filter by account
        account: Option<String>,
        // filter by party
        party: Option<String>,
    ) -> BoxStream<Result<JournalEntry>> {
        self.entries_filtered(account, party)
            // end will terminate recurring entry iterators
            .map_ok(|entry| Some(entry.into_journal_entries(self.end)))
            .chain(stream::once(async { Ok(None) }))
            // emits possibly recurring journal entries in order provided entries are in order
            .scan(
                vec![],
                |cur_iters: &mut Vec<Peekable<JournalEntryIterator>>,
                 jes: Result<Option<JournalEntryIterator>>| {
                    future::ready(
                        // map ok
                        jes.map(|mut jes| {
                            // TODO check dates of entries are in order

                            let cur_entry = jes.as_mut().and_then(|j| j.next());

                            let cur_date = cur_entry
                                .as_ref()
                                .and_then(|j| j.as_ref().ok().map(|e| e.clone().date()));

                            if let Some(jes) = jes {
                                let mut jp = jes.peekable();
                                // if it has more add to cur_iters
                                if jp.peek().is_some() {
                                    cur_iters.push(jp);
                                }
                            }

                            let mut cur_entries = Vec::new();

                            if !cur_iters.is_empty() {
                                while cur_iters.iter_mut().all(|it| it.peek().is_some()) {
                                    let mut earliest_date = None;
                                    let earliest = cur_iters
                                        .iter_mut()
                                        .reduce(|acc, it| {
                                            match (acc.peek().unwrap(), it.peek().unwrap()) {
                                                (Ok(a), Ok(b)) => {
                                                    if a.date() <= b.date() {
                                                        earliest_date = Some(a.date());
                                                        acc
                                                    } else {
                                                        earliest_date = Some(b.date());
                                                        it
                                                    }
                                                }
                                                _ => acc,
                                            }
                                        })
                                        .unwrap();
                                    if cur_date.is_none_or(|cd| {
                                        earliest
                                            .peek()
                                            .unwrap()
                                            .as_ref()
                                            .is_ok_and(|e| e.date() <= cd)
                                    }) {
                                        cur_entries.push(earliest.next().unwrap())
                                    } else {
                                        break;
                                    }
                                }
                            }

                            // rm iters which don't have a remaining next value
                            cur_iters.retain_mut(|it| it.peek().is_some());

                            if let Some(ce) = cur_entry {
                                cur_entries.push(ce);
                            }
                            Some(stream::iter(cur_entries))
                        })
                        .transpose(),
                    )
                },
            )
            .try_flatten()
            .boxed()
    }

    pub fn ledger(&self, account: &str) -> BoxStream<Result<LedgerLine>> {
        let lines = self.journal_filtered(None, None);
        ledger_from_journal(lines, account.to_string())
    }

    /// Get balances for each account appearing in own stream of `JournalEntry`s
    pub fn balances(&self) -> impl Future<Output = Result<Balances>> {
        self.balances_filtered(None, None, None)
    }

    pub async fn balances_filtered(
        &self,
        // filter by account
        account: Option<String>,
        // filter by party
        party: Option<String>,
        // filter by permanence
        is_real: Option<bool>,
    ) -> Result<Balances> {
        let lines = self.journal_lines_filtered(account, party);
        let mut balances = balances_from_journal_lines(lines).await?;
        // add full account objects from chart if possible
        // TODO auto add details for Accounts Payable/Receivable?
        if let Some(chart) = self.chart() {
            balances = balances
                .into_iter()
                .map(|(name, (amt, account))| {
                    (
                        name.clone(),
                        (amt, account.or_else(|| chart.get(&name).cloned())),
                    )
                })
                .collect();
        };
        if let Some(is_real) = is_real {
            balances = balances
                .into_iter()
                .map(|b| {
                    let a = b.1.1.clone().context(format!(
                        "Permanence filter applied but cannot be determined for account: {}",
                        b.0
                    ))?;
                    anyhow::Ok((b, a))
                })
                .filter_map_ok(|(b, a)| {
                    if a.is_real() == is_real {
                        Some(b)
                    } else {
                        None
                    }
                })
                .try_collect()?;
        }
        Ok(balances)
    }

    pub fn journal_lines_filtered(
        &self,
        // filter by account
        account: Option<String>,
        // filter by party
        party: Option<String>,
    ) -> BoxStream<Result<JournalLine>> {
        self.journal_filtered(account, party)
            .and_then(|entry| future::ready(Ok(stream::iter(entry.lines()).map(Ok))))
            .try_flatten()
            .boxed()
    }

    /// get journal lines with entry party in place of given account
    /// e.g. for accounts payable/receivable
    pub fn journal_lines_with_party(
        &self,
        until: Option<NaiveDate>,
        account: JournalAccount,
    ) -> BoxStream<Result<JournalLine>> {
        self.entries_filtered(Some(account.clone()), None)
            .and_then(move |entry| {
                future::ready(Ok(stream::iter(
                    entry
                        .journal_lines_with_party(until, account.clone())
                        .unwrap_or_default(),
                )
                .map(Ok)))
            })
            .try_flatten()
            .boxed()
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
                let account = chart.get(account).context("Account not found")?;
                report.apply_balance((account, &balance.0))?;
                Ok(report)
            })
    }

    pub async fn payable(&self) -> Result<Balances> {
        let account = "Accounts Payable".to_string();
        let party_lines = self.journal_lines_with_party(None, account); // TODO pass in until
        let balances = balances_from_journal_lines(party_lines).await?;
        // filter out zero balances
        Ok(balances
            .into_iter()
            .filter(|(_, (amt, _))| amt.abs_amount() != Money::default())
            .collect())
    }

    pub async fn receivable(&self) -> Result<Balances> {
        let account = "Accounts Receivable".to_string();
        let party_lines = self.journal_lines_with_party(None, account); // TODO pass in until
        let balances = balances_from_journal_lines(party_lines).await?;
        // filter out zero balances
        Ok(balances
            .into_iter()
            .filter(|(_, (amt, _))| amt.abs_amount() != Money::default())
            .collect())
    }
}

#[cfg(test)]
mod entry_tests {
    use super::*;
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
