use self::JournalAmountTest::*;
use accounts::account::Type::*;
use accounts::chart_of_accounts::ChartOfAccounts;
use accounts::entry::Entry;
use accounts::entry::journal::BalanceType;
use accounts::render_table::{RenderTable, RenderTableOpts};
use accounts::report::ReportNode;
use accounts::*;
use anyhow::{Result, bail};
use entry::journal::{JournalAccount, JournalAmount, JournalEntry};
use futures::StreamExt;
use futures::stream::TryStreamExt;
use indoc::indoc;
use insta::assert_snapshot;
use itertools::Itertools;
use std::collections::HashMap;
use std::convert::TryInto;

/// Test that a dir containing one entry per file parses without error
#[async_std::test]
async fn test_basic_entries() -> Result<()> {
    let instance = Accounts::new(
        JournalSource::Path("./tests/fixtures/entries_flat".to_string()),
        None,
    );
    let entries = instance.entries().try_collect::<Vec<Entry>>().await?;
    dbg!(&entries);
    let count = entries.iter().map(|entry| entry.id()).unique().count();
    assert_eq!(count, 3);
    Ok(())
}

/// Test that a dir containing nested dirs parses without error
#[async_std::test]
async fn test_nested_dirs() -> Result<()> {
    let instance = Accounts::new(
        JournalSource::Path("./tests/fixtures/entries_nested_dirs".to_string()),
        None,
    );
    let entries = instance.entries().try_collect::<Vec<Entry>>().await?;
    dbg!(&entries);
    let count = entries.iter().map(|entry| entry.id()).unique().count();
    assert_eq!(count, 2);
    Ok(())
}

/// Test that a dir with one file containing multiple entries parses without error
#[async_std::test]
async fn test_multiple_entries_in_one_file() -> Result<()> {
    let instance = Accounts::new(
        JournalSource::Path("./tests/fixtures/entries_multiple_entries_in_one_file".to_string()),
        None,
    );
    let entries = instance.entries().try_collect::<Vec<Entry>>().await?;
    dbg!(&entries);
    let count = entries.iter().map(|entry| entry.id()).unique().count();
    assert_eq!(count, 2);
    Ok(())
}

/// Test that journal entries from entries are correct
#[async_std::test]
async fn test_journal_from_entries() -> Result<()> {
    let instance = Accounts::new(
        JournalSource::Path("./tests/fixtures/entries".to_string()),
        None,
    );

    let journal = instance
        .journal()
        .render(RenderTableOpts {
            width: Some(80),
            no_colors: true,
            ..Default::default()
        })
        .collect::<Vec<String>>()
        .await
        .join("\n");

    // TODO automatically add memos for invoices that don't have them
    assert_snapshot!(journal, @r"
    ╭────────────┬───────────────────────────────────┬──────────────┬──────────────╮
    │ Date       │ Particulars                       │       Debits │      Credits │
    ├────────────┼───────────────────────────────────┼──────────────┼──────────────┤
    │ 2020-01-01 │ Operating Expenses                │       100.00 │              │
    │            │ Accounts Payable                  │              │       100.00 │
    │                                                                              │
    │ 2020-01-02 │ Accounts Payable                  │       100.00 │              │
    │            │ Credit Card                       │              │       100.00 │
    │              (Business Services)                                             │
    │                                                                              │
    │ 2020-01-03 │ Operating Expenses                │        50.00 │              │
    │            │ Business Checking                 │              │        50.00 │
    │                                                                              │
    │ 2020-01-04 │ Operating Expenses                │       100.00 │              │
    │            │ Accounts Payable                  │              │       100.00 │
    │                                                                              │
    │ 2020-01-05 │ Accounts Receivable               │        10.00 │              │
    │            │ Widget Sales                      │              │        10.00 │
    │                                                                              │
    │ 2020-01-06 │ Business Checking                 │        10.00 │              │
    │            │ Accounts Receivable               │              │        10.00 │
    │              (Widget)                                                        │
    │                                                                              │
    │ 2020-01-07 │ Business Checking                 │         5.00 │              │
    │            │ Widget Sales                      │              │         5.00 │
    │                                                                              │
    │ 2020-01-08 │ Accounts Receivable               │        10.00 │              │
    │            │ Widget Sales                      │              │        10.00 │
    ╰──────────────────────────────────────────────────────────────────────────────╯
    ");
    println!("{journal}");
    Ok(())
}

/// Test that journal entries from entries are correct
#[async_std::test]
async fn journal_entry() -> Result<()> {
    static JOURNAL: &str = indoc! {"
        ---
        date: 2020-01-01
        memo: Initial Contribution
        debits:
          Bank: 15,000
        credits:
          Owner Contributions: 15,000
    "};
    let instance = Accounts::new(JournalSource::Str(JOURNAL.to_string()), None);

    let journal = instance
        .journal()
        .render(RenderTableOpts {
            width: Some(80),
            no_colors: true,
            ..Default::default()
        })
        .collect::<Vec<String>>()
        .await
        .join("\n");

    assert_snapshot!(journal, @r"
    ╭────────────┬───────────────────────────────────┬──────────────┬──────────────╮
    │ Date       │ Particulars                       │       Debits │      Credits │
    ├────────────┼───────────────────────────────────┼──────────────┼──────────────┤
    │ 2020-01-01 │ Bank                              │    15,000.00 │              │
    │            │ Owner Contributions               │              │    15,000.00 │
    │              (Initial Contribution)                                          │
    ╰──────────────────────────────────────────────────────────────────────────────╯
    ");
    println!("{journal}");
    Ok(())
}

/// Test ledger from entries
#[async_std::test]
async fn test_ledger() -> Result<()> {
    let instance = Accounts::new(
        JournalSource::Path("./tests/fixtures/entries".to_string()),
        None,
    );
    let ledger_lines: Vec<LedgerLine> = instance.ledger("Business Checking").try_collect().await?;
    // with running totals
    let ledger_lines = ledger_lines
        .iter()
        .sorted_by_key(|l| l.date)
        .scan(JournalAmount::default(), |acc, line| {
            *acc += line.amount;
            Some(LedgerLine {
                running_total: *acc,
                ..line.clone()
            })
        })
        .collect::<Vec<LedgerLine>>();

    assert_eq!(dbg!(&ledger_lines).iter().count(), 3);
    Expect(&ledger_lines)
        .contains(
            "2020-01-03",
            JournalAmount::credit(50.00)?,
            JournalAmount::credit(50.00)?,
        )
        .contains(
            "2020-01-06",
            JournalAmount::debit(10.00)?,
            JournalAmount::credit(40.00)?,
        )
        .contains(
            "2020-01-07",
            JournalAmount::debit(5.00)?,
            JournalAmount::credit(35.00)?,
        );
    Ok(())
}

/// Test balances from entries
#[async_std::test]
async fn test_balance() -> Result<()> {
    let instance = Accounts::new(
        JournalSource::Path("./tests/fixtures/entries".to_string()),
        None,
    );
    let balances = instance.balances().await?;
    assert_eq!(balances.len(), 6);
    Expect(&balances)
        .contains("Operating Expenses", Debit(250.00))
        .contains("Accounts Payable", Credit(100.00))
        .contains("Credit Card", Credit(100.00))
        .contains("Business Checking", Credit(35.00))
        .contains("Widget Sales", Credit(25.00))
        .contains("Accounts Receivable", Debit(10.00));
    Ok(())
}

/// Test journal entries from recurring entries
#[async_std::test]
async fn test_recurring() -> Result<()> {
    let instance = Accounts::new(
        JournalSource::Path("./tests/fixtures/entries_recurring".to_string()),
        None,
    );

    let journal_entries: Vec<JournalEntry> = instance.journal().try_collect().await?;

    assert_eq!(dbg!(&journal_entries).iter().count(), 6);
    Expect(&journal_entries)
        .contains(
            "2020-01-01",
            "Operating Expenses",
            JournalAmount::debit(100.00)?,
        )
        .contains(
            "2020-01-01",
            "Accounts Payable",
            JournalAmount::credit(100.00)?,
        )
        .contains(
            "2020-01-02",
            "Accounts Payable",
            JournalAmount::debit(100.00)?,
        )
        .contains("2020-01-02", "Bank Account", JournalAmount::credit(100.00)?)
        .contains(
            "2020-02-01",
            "Operating Expenses",
            JournalAmount::debit(100.00)?,
        )
        .contains(
            "2020-02-01",
            "Accounts Payable",
            JournalAmount::credit(100.00)?,
        )
        .contains(
            "2020-02-03",
            "Accounts Payable",
            JournalAmount::debit(100.00)?,
        )
        .contains("2020-02-03", "Bank Account", JournalAmount::credit(100.00)?)
        .contains(
            "2020-03-01",
            "Operating Expenses",
            JournalAmount::debit(150.00)?,
        )
        .contains(
            "2020-03-01",
            "Accounts Payable",
            JournalAmount::credit(150.00)?,
        )
        .contains(
            "2020-03-02",
            "Accounts Payable",
            JournalAmount::debit(150.00)?,
        )
        .contains("2020-03-02", "Bank Account", JournalAmount::credit(150.00)?);
    Ok(())
}

#[async_std::test]
async fn ordered_recurring() -> Result<()> {
    static JOURNAL: &str = indoc! {"
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
    "};

    let instance = Accounts::new(
        JournalSource::Str(JOURNAL.to_string()),
        Some("2020-03-31".parse()?),
    );
    let ledger = instance
        .ledger("Accounts Payable")
        .render(RenderTableOpts {
            balance: Some(BalanceType::Credit),
            width: Some(80),
            no_colors: true,
            ..Default::default()
        })
        .collect::<Vec<String>>()
        .await
        .join("\n");

    assert_snapshot!(ledger, @r"
        ╭────────────┬────────────────────┬──────────────┬──────────────┬──────────────╮
        │ Date       │ Memo               │        Debit │       Credit │   Cr Balance │
        ├────────────┼────────────────────┼──────────────┼──────────────┼──────────────┤
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
        ╰────────────┴────────────────────┴──────────────┴──────────────┴──────────────╯
    ");
    Ok(())
}

#[async_std::test]
async fn test_chart_of_accounts() -> Result<()> {
    let chart_of_accounts =
        ChartOfAccounts::from_file("./tests/fixtures/ChartOfAccounts.yaml").await?;
    dbg!(&chart_of_accounts);
    assert_eq!(
        chart_of_accounts.get("Operating Expenses")?.acc_type,
        Expense
    );
    assert_eq!(chart_of_accounts.get("Credit Card")?.acc_type, Liability);
    assert_eq!(chart_of_accounts.get("Business Checking")?.acc_type, Asset);
    assert_eq!(chart_of_accounts.get("Widget Sales")?.acc_type, Revenue);
    Ok(())
}

#[async_std::test]
async fn test_report() -> Result<()> {
    let report = ReportNode::from_file("./tests/fixtures/IncomeStatement.yaml").await?;
    let items = report.items()?;
    dbg!(&report);
    dbg!(&items);
    assert_eq!(
        items[3].0,
        vec!["Income Statement", "Expenses", "Indirect Expenses", "Rent"]
    );
    assert_eq!(
        items[4].0,
        vec!["Income Statement", "Expenses", "Direct Expenses"]
    );
    assert_eq!(
        items[6].0,
        vec!["Income Statement", "Revenue", "Direct Revenue"]
    );
    assert_eq!(
        items[7].0,
        vec!["Income Statement", "Revenue", "Indirect Revenue"]
    );
    Ok(())
}

#[async_std::test]
async fn test_run_report() -> Result<()> {
    let instance = Accounts::new(
        JournalSource::Path("./tests/fixtures/entries".to_string()),
        None,
    );
    let chart_of_accounts =
        ChartOfAccounts::from_file("./tests/fixtures/ChartOfAccounts.yaml").await?;
    let mut report = ReportNode::from_file("./tests/fixtures/IncomeStatement.yaml").await?;
    instance.run_report(&chart_of_accounts, &mut report).await?;
    let items = report.items()?;
    dbg!(&items);
    println!("{report}");

    assert_eq!(items[0].0, vec!["Income Statement"],);
    assert_eq!(items[0].2.0, vec!["Operating Expenses", "Widget Sales"]);
    assert_eq!(items[0].2.1, JournalAmount::Debit(225.00.try_into()?));

    assert_eq!(
        items[4].0,
        vec!["Income Statement", "Expenses", "Indirect Expenses", "Other"],
    );
    assert_eq!(items[4].2.0, vec!["Operating Expenses"]);
    assert_eq!(items[4].2.1, JournalAmount::Debit(250.00.try_into()?));

    assert_eq!(items[6].0, vec!["Income Statement", "Revenue"]);
    assert_eq!(items[6].2.0, vec!["Widget Sales"]);
    assert_eq!(items[6].2.1, JournalAmount::Credit(25.00.try_into()?));

    assert_eq!(
        items[7].0,
        vec!["Income Statement", "Revenue", "Direct Revenue"]
    );
    assert_eq!(items[7].2.0, vec!["Widget Sales"]);
    assert_eq!(items[7].2.1, JournalAmount::Credit(25.00.try_into()?));

    assert_eq!(
        items[8].0,
        vec!["Income Statement", "Revenue", "Indirect Revenue"]
    );
    assert!(items[8].2.0.is_empty());
    assert_eq!(items[8].2.1, JournalAmount::default());

    Ok(())
}

#[derive(Debug)]
enum JournalAmountTest {
    Debit(f64),
    Credit(f64),
}

/// struct for special assertions
struct Expect<'a, T>(&'a T);

impl Expect<'_, Vec<LedgerLine>> {
    fn contains(&self, date: &str, amount: JournalAmount, total: JournalAmount) -> &Self {
        assert!(
            self.0.iter().any(|actual| {
                actual.date.to_string() == date
                    && actual.amount == amount
                    && actual.running_total == total
            }),
            "ledger line with date {date} amount {:?} and running total {:?} not found in {:?}",
            amount,
            total,
            self.0
        );
        self
    }
}

impl Expect<'_, Vec<JournalEntry>> {
    fn contains(&self, date: &str, account: &str, amount: JournalAmount) -> &Self {
        assert!(
            self.0.iter().any(|actual| {
                actual.date().to_string() == date
                    && actual
                        .lines()
                        .iter()
                        .any(|l| l.0 == account && l.1 == amount)
            }),
            "entry with date {:?} with account {:?} for {:?} not found in {:?}",
            date,
            account,
            amount,
            self.0
        );
        self
    }
}

impl Expect<'_, HashMap<JournalAccount, JournalAmount>> {
    fn contains(&self, account: &str, amount: JournalAmountTest) -> &Self {
        let amount = match amount {
            Debit(m) => JournalAmount::Debit(m.try_into().unwrap()),
            Credit(m) => JournalAmount::Credit(m.try_into().unwrap()),
        };
        assert!(
            self.0
                .iter()
                .any(|actual| actual.0 == account && actual.1 == &amount),
            "({}: {:?}) not found in {:?}",
            account,
            amount,
            self.0
        );
        self
    }
}
