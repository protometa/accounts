use self::JournalAmountTest::*;
use accounts::account::Type::*;
use accounts::chart_of_accounts::ChartOfAccounts;
use accounts::entry::Entry;
use accounts::journal_entry::*;
use accounts::money::Money;
use accounts::report::{ReportNode, Total};
use accounts::*;
use anyhow::Result;
use futures::stream::TryStreamExt;
use itertools::Itertools;
use std::collections::HashMap;
use std::convert::TryInto;

/// Test that a dir containing one entry per file parses without error
#[async_std::test]
async fn test_basic_entries() -> Result<()> {
    let ledger = Ledger::new(Some("./tests/fixtures/entries_flat"));
    let entries = ledger.entries().try_collect::<Vec<Entry>>().await?;
    dbg!(&entries);
    let count = entries.iter().map(|entry| entry.id()).unique().count();
    assert_eq!(count, 2);
    Ok(())
}

/// Test that a dir containing nested dirs parses without error
#[async_std::test]
async fn test_nested_dirs() -> Result<()> {
    let ledger = Ledger::new(Some("./tests/fixtures/entries_nested_dirs"));
    let entries = ledger.entries().try_collect::<Vec<Entry>>().await?;
    dbg!(&entries);
    let count = entries.iter().map(|entry| entry.id()).unique().count();
    assert_eq!(count, 2);
    Ok(())
}

/// Test that a dir with one file containing multiple entries parses without error
#[async_std::test]
async fn test_multiple_entries_in_one_file() -> Result<()> {
    let ledger = Ledger::new(Some(
        "./tests/fixtures/entries_multiple_entries_in_one_file",
    ));
    let entries = ledger.entries().try_collect::<Vec<Entry>>().await?;
    dbg!(&entries);
    let count = entries.iter().map(|entry| entry.id()).unique().count();
    assert_eq!(count, 2);
    Ok(())
}

/// Test that journal entries from entries are correct
#[async_std::test]
async fn test_journal_from_entries() -> Result<()> {
    let ledger = Ledger::new(Some("./tests/fixtures/entries"));

    let journal_entries: Vec<JournalEntry> = ledger.journal().try_collect().await?;

    assert_eq!(dbg!(&journal_entries).iter().count(), 16);
    Expect(&journal_entries)
        .contains("2020-01-01", "Operating Expenses", Debit(100.00))
        .contains("2020-01-01", "Accounts Payable", Credit(100.00))
        .contains("2020-01-02", "Accounts Payable", Debit(100.00))
        .contains("2020-01-02", "Credit Card", Credit(100.00))
        .contains("2020-01-03", "Operating Expenses", Debit(50.00))
        .contains("2020-01-03", "Business Checking", Credit(50.00))
        .contains("2020-01-04", "Operating Expenses", Debit(100.00))
        .contains("2020-01-04", "Accounts Payable", Credit(100.00))
        .contains("2020-01-05", "Accounts Receivable", Debit(10.00))
        .contains("2020-01-05", "Widget Sales", Credit(10.00))
        .contains("2020-01-06", "Business Checking", Debit(10.00))
        .contains("2020-01-06", "Accounts Receivable", Credit(10.00))
        .contains("2020-01-07", "Business Checking", Debit(5.00))
        .contains("2020-01-07", "Widget Sales", Credit(5.00))
        .contains("2020-01-08", "Accounts Receivable", Debit(10.00))
        .contains("2020-01-08", "Widget Sales", Credit(10.00));
    Ok(())
}

/// Test balances from entries
#[async_std::test]
async fn test_balance() -> Result<()> {
    let ledger = Ledger::new(Some("./tests/fixtures/entries"));
    let balances = ledger.balances().await?;
    assert_eq!(balances.iter().count(), 6);
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
    let ledger = Ledger::new(Some("./tests/fixtures/entries_recurring"));

    let journal_entries: Vec<JournalEntry> = ledger.journal().try_collect().await?;

    assert_eq!(dbg!(&journal_entries).iter().count(), 12);
    Expect(&journal_entries)
        .contains("2020-01-01", "Operating Expenses", Debit(100.00))
        .contains("2020-01-01", "Accounts Payable", Credit(100.00))
        .contains("2020-01-02", "Accounts Payable", Debit(100.00))
        .contains("2020-01-02", "Bank Account", Credit(100.00))
        .contains("2020-02-01", "Operating Expenses", Debit(100.00))
        .contains("2020-02-01", "Accounts Payable", Credit(100.00))
        .contains("2020-02-03", "Accounts Payable", Debit(100.00))
        .contains("2020-02-03", "Bank Account", Credit(100.00))
        .contains("2020-03-01", "Operating Expenses", Debit(150.00))
        .contains("2020-03-01", "Accounts Payable", Credit(150.00))
        .contains("2020-03-02", "Accounts Payable", Debit(150.00))
        .contains("2020-03-02", "Bank Account", Credit(150.00));
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
    let ledger = Ledger::new(Some("./tests/fixtures/entries"));
    let chart_of_accounts =
        ChartOfAccounts::from_file("./tests/fixtures/ChartOfAccounts.yaml").await?;
    let mut report = ReportNode::from_file("./tests/fixtures/IncomeStatement.yaml").await?;
    ledger.run_report(&chart_of_accounts, &mut report).await?;
    let items = report.items()?;
    dbg!(&items);
    println!("{}", report);

    assert_eq!(items[0].0, vec!["Income Statement"],);
    assert_eq!(items[0].2 .0, vec!["Operating Expenses", "Widget Sales"]);
    assert_eq!(items[0].2 .1, JournalAmount::Debit(225.00.try_into()?));

    assert_eq!(
        items[4].0,
        vec!["Income Statement", "Expenses", "Indirect Expenses", "Other"],
    );
    assert_eq!(items[4].2 .0, vec!["Operating Expenses"]);
    assert_eq!(items[4].2 .1, JournalAmount::Debit(250.00.try_into()?));

    assert_eq!(items[6].0, vec!["Income Statement", "Revenue"]);
    assert_eq!(items[6].2 .0, vec!["Widget Sales"]);
    assert_eq!(items[6].2 .1, JournalAmount::Credit(25.00.try_into()?));

    assert_eq!(
        items[7].0,
        vec!["Income Statement", "Revenue", "Direct Revenue"]
    );
    assert_eq!(items[7].2 .0, vec!["Widget Sales"]);
    assert_eq!(items[7].2 .1, JournalAmount::Credit(25.00.try_into()?));

    assert_eq!(
        items[8].0,
        vec!["Income Statement", "Revenue", "Indirect Revenue"]
    );
    assert!(items[8].2 .0.is_empty());
    assert_eq!(items[8].2 .1, JournalAmount::default());

    Ok(())
}

#[derive(Debug)]
enum JournalAmountTest {
    Debit(f64),
    Credit(f64),
}

/// struct for special assertions
struct Expect<'a, T>(&'a T);

impl Expect<'_, Vec<JournalEntry>> {
    fn contains(&self, date: &str, account: &str, amount: JournalAmountTest) -> &Self {
        let expected = &&JournalEntry(
            date.parse().unwrap(),
            account.into(),
            match amount {
                Debit(m) => JournalAmount::Debit(m.try_into().unwrap()),
                Credit(m) => JournalAmount::Credit(m.try_into().unwrap()),
            },
        );
        assert!(
            self.0.iter().find(|actual| actual == expected).is_some(),
            "{:?} not found in {:?}",
            expected,
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
                .find(|actual| actual.0 == account && actual.1 == &amount)
                .is_some(),
            "({}: {:?}) not found in {:?}",
            account,
            amount,
            self.0
        );
        self
    }
}
