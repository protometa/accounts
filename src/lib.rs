pub mod account;
pub mod bank_txs;
pub mod chart_of_accounts;
pub mod entry;
pub mod lines;
pub mod money;
pub mod report;

use anyhow::{Error, Result};
use chart_of_accounts::ChartOfAccounts;
use chrono::NaiveDate;
use comfy_table::*;
use entry::journal::{BalanceType, JournalAccount, JournalAmount, JournalEntry, JournalLine};
use entry::{Entry, JournalEntryIterator};
use futures::future::{self, Future};
use futures::stream::{self, BoxStream, TryStreamExt};
use futures::{Stream, StreamExt, TryStream};
use lines::lines;
use lines_ext::LinesExt;
use money::Money;
use report::ReportNode;
use std::borrow::ToOwned;
use std::cmp;
use std::collections::HashMap;
use std::iter::Peekable;
use std::ops::AddAssign;

#[derive(Debug, Clone)]
pub enum JournalSource {
    Stdin,
    Path(String),
    Str(String),
}

pub struct Accounts {
    journal: JournalSource,
    end: Option<NaiveDate>,
}

type Balances = HashMap<JournalAccount, JournalAmount>;

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
                .and_modify(|total: &mut JournalAmount| {
                    total.add_assign(amount);
                })
                .or_insert(amount);
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
    pub fn new(journal: JournalSource, end: Option<NaiveDate>) -> Self {
        Accounts { journal, end }
    }

    /// Parse own stream of lines into `Entry`s
    pub fn entries(&self) -> BoxStream<Result<Entry>> {
        self.entries_filtered(None, None)
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
            JournalSource::Stdin => entries_from_lines(lines(None), account, party),
            JournalSource::Path(path) => {
                entries_from_lines(lines(Some(path.clone())), account, party)
            }
            JournalSource::Str(s) => {
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
        self.balances_filtered(None, None)
    }

    pub fn balances_filtered(
        &self,
        // filter by account
        account: Option<String>,
        // filter by party
        party: Option<String>,
    ) -> impl Future<Output = Result<Balances>> {
        let lines = self.journal_lines_filtered(account, party);
        balances_from_journal_lines(lines)
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
                let account = chart.get(account)?;
                report.apply_balance((account, balance))?;
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
            .filter(|(_, amt)| amt.abs_amount() != Money::default())
            .collect())
    }

    pub async fn receivable(&self) -> Result<Balances> {
        let account = "Accounts Receivable".to_string();
        let party_lines = self.journal_lines_with_party(None, account); // TODO pass in until
        let balances = balances_from_journal_lines(party_lines).await?;
        // filter out zero balances
        Ok(balances
            .into_iter()
            .filter(|(_, amt)| amt.abs_amount() != Money::default())
            .collect())
    }
}

enum TableRow<L> {
    Header,
    Body(L),
    Footer,
}

const DATE_COL_WIDTH: u16 = 12;
const MEMO_COL_WIDTH: u16 = 60;
const MONEY_COL_WIDTH: u16 = 14;
// the width of all static table content
// momo column will be dynamic and wrap on small screens
// (we can't use comfy_table dynamic width since it can't know the data ahead of time)
const STATIC_WIDTH: u16 = DATE_COL_WIDTH + MONEY_COL_WIDTH * 3 + 6;
const TABLE_WIDTH: u16 = STATIC_WIDTH + MEMO_COL_WIDTH;

const LEDGER_HEADER_STYLE: TableStyle = TableStyle::new()
    .top_border(LineStyle::new('╭', '─', '┬', '╮'))
    .header_lines(ContentLineStyle::new('│', '│', '│'))
    .header_separator(LineStyle::new('├', '─', '┼', '┤'))
    .content_lines(ContentLineStyle::new('│', '│', '│'));

const LEDGER_BODY_STYLE: TableStyle =
    TableStyle::new().content_lines(ContentLineStyle::new('│', '│', '│'));

const LEDGER_FOOTER_STYLE: TableStyle = TableStyle::new()
    .content_lines(ContentLineStyle::new('│', ' ', '│'))
    .bottom_border(LineStyle::new('╰', '─', '┴', '╯'));

fn set_cols(t: &mut comfy_table::Table, width: u16) {
    // this sets default max width
    t.set_width(width);
    // this tells us above or tty width
    let width = t.width().unwrap_or_default();
    let memo_col_dyn_width = cmp::min(width.saturating_sub(STATIC_WIDTH), MEMO_COL_WIDTH);
    t.column_mut(0)
        .unwrap()
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(DATE_COL_WIDTH)));
    t.column_mut(1)
        .unwrap()
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(memo_col_dyn_width)));
    t.column_mut(2)
        .unwrap()
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(MONEY_COL_WIDTH)))
        .set_cell_alignment(CellAlignment::Right);
    t.column_mut(3)
        .unwrap()
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(MONEY_COL_WIDTH)))
        .set_cell_alignment(CellAlignment::Right);
    t.column_mut(4)
        .unwrap()
        .set_constraint(ColumnConstraint::Absolute(Width::Fixed(MONEY_COL_WIDTH)))
        .set_cell_alignment(CellAlignment::Right);
}

pub trait RenderTable<'a> {
    fn render(
        self,
        balance: Option<BalanceType>,
        width: Option<u16>,
        body_only: bool,
    ) -> BoxStream<'a, String>;
}

impl<'a> RenderTable<'a> for BoxStream<'a, Result<LedgerLine>> {
    fn render(
        self,
        balance: Option<BalanceType>,
        width: Option<u16>,
        body_only: bool,
    ) -> BoxStream<'a, String> {
        stream::once(async { TableRow::Header })
            .chain(self.map(TableRow::Body))
            .chain(stream::once(async { TableRow::Footer }))
            .filter(move |row| {
                future::ready(match row {
                    TableRow::Body(_) => true,
                    _ => !body_only,
                })
            })
            .map(move |row| match row {
                TableRow::Header => {
                    let mut t = Table::new();
                    t.load_style(LEDGER_HEADER_STYLE).set_header(
                        ["Date", "Memo", "Debit", "Credit", "Balance"]
                            .iter()
                            .map(|h| Cell::new(h).add_attribute(Attribute::Bold)),
                    );
                    set_cols(&mut t, width.unwrap_or(TABLE_WIDTH));
                    t.to_string()
                }
                TableRow::Body(row) => match row {
                    Ok(row) => {
                        let mut t = Table::new();
                        t.load_style(LEDGER_BODY_STYLE).add_row([
                            row.date.to_string(),
                            row.memo.unwrap_or(String::default()),
                            row.amount
                                .as_abs_debit()
                                .map(|m| m.to_string())
                                .unwrap_or(String::default()),
                            row.amount
                                .as_abs_credit()
                                .map(|m| m.to_string())
                                .unwrap_or(String::default()),
                            row.running_total
                                .as_balance_type(balance.as_ref().unwrap_or(&BalanceType::Debit))
                                .to_string(),
                        ]);
                        set_cols(&mut t, width.unwrap_or(TABLE_WIDTH));
                        t.to_string()
                    }
                    _ => "ERROR".to_string(),
                },
                TableRow::Footer => {
                    let mut t = Table::new();
                    t.load_style(LEDGER_FOOTER_STYLE)
                        .add_row((0..5).map(|_| ""));
                    set_cols(&mut t, width.unwrap_or(TABLE_WIDTH));
                    // rm empty row, keep only bottom border
                    t.to_string().split_once('\n').unwrap().1.to_string()
                }
            })
            .boxed()
    }
}

#[cfg(test)]
mod entry_tests {
    use super::*;

    use indoc::indoc;
    use insta::assert_snapshot;

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

    #[async_std::test]
    async fn ordered_recurring() -> Result<()> {
        let instance = Accounts::new(
            JournalSource::Str(
                indoc! {"
                ---
                type: Purchase Invoice
                date: 2020-01-02
                memo: Weekly bill
                party: ACME Business Services
                account: Operating Expenses
                amount: 10
                repeat: weekly
                ---
                date: 2020-01-03
                type: Payment Sent
                party: ACME Business Services
                memo: Payment
                account: Checking
                amount: 50
                ---
                type: Purchase Invoice
                date: 2020-01-05
                memo: Monthly bill
                party: ACME Business Services
                account: Operating Expenses
                amount: 100
                repeat: monthly
                ---
                date: 2020-02-04
                type: Payment Sent
                party: ACME Business Services
                memo: Payment 
                account: Checking
                amount: 100
                ---
                date: 2020-03-06
                type: Payment Sent
                party: ACME Business Services
                memo: Payment
                account: Checking
                amount: 100
                "}
                .to_string(),
            ),
            Some("2020-03-31".parse()?),
        );
        let entries = instance
            .ledger("Accounts Payable")
            .render(Some(BalanceType::Credit), Some(80), true)
            .collect::<Vec<String>>()
            .await
            .join("\n");

        assert_snapshot!(entries, @r"
        │ 2020-01-02 │ Weekly bill        │              │        10.00 │        10.00 │
        │ 2020-01-03 │ Payment            │        50.00 │              │      (40.00) │
        │ 2020-01-05 │ Monthly bill       │              │       100.00 │        60.00 │
        │ 2020-01-09 │ Weekly bill        │              │        10.00 │        70.00 │
        │ 2020-01-16 │ Weekly bill        │              │        10.00 │        80.00 │
        │ 2020-01-23 │ Weekly bill        │              │        10.00 │        90.00 │
        │ 2020-01-30 │ Weekly bill        │              │        10.00 │       100.00 │
        │ 2020-02-04 │ Payment            │       100.00 │              │         0.00 │
        │ 2020-02-05 │ Monthly bill       │              │       100.00 │       100.00 │
        │ 2020-02-06 │ Weekly bill        │              │        10.00 │       110.00 │
        │ 2020-02-13 │ Weekly bill        │              │        10.00 │       120.00 │
        │ 2020-02-20 │ Weekly bill        │              │        10.00 │       130.00 │
        │ 2020-02-27 │ Weekly bill        │              │        10.00 │       140.00 │
        │ 2020-03-05 │ Weekly bill        │              │        10.00 │       150.00 │
        │ 2020-03-05 │ Monthly bill       │              │       100.00 │       250.00 │
        │ 2020-03-06 │ Payment            │       100.00 │              │       150.00 │
        │ 2020-03-12 │ Weekly bill        │              │        10.00 │       160.00 │
        │ 2020-03-19 │ Weekly bill        │              │        10.00 │       170.00 │
        │ 2020-03-26 │ Weekly bill        │              │        10.00 │       180.00 │
        ");
        Ok(())
    }
}
