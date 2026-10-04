use accounts::account::BalanceType::*;
use accounts::account::Class::*;
use accounts::chart_of_accounts::ChartOfAccounts;
use accounts::entry::Entry;
use accounts::render_table::RenderTable;
use accounts::render_table::{RenderStreamTable, RenderTableOpts};
use accounts::report::ReportNode;
use accounts::*;
use anyhow::Result;
use futures::StreamExt;
use futures::stream::TryStreamExt;
use indoc::indoc;
use insta::assert_snapshot;
use itertools::Itertools;

/// Test that a dir containing one entry per file parses without error
#[async_std::test]
async fn test_basic_entries() -> Result<()> {
    let instance = Accounts::new(
        DocSource::Path("./tests/fixtures/entries_flat".to_string()),
        None,
        None,
    )
    .await?;
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
        DocSource::Path("./tests/fixtures/entries_nested_dirs".to_string()),
        None,
        None,
    )
    .await?;
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
        DocSource::Path("./tests/fixtures/entries_multiple_entries_in_one_file".to_string()),
        None,
        None,
    )
    .await?;
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
        DocSource::Path("./tests/fixtures/entries".to_string()),
        None,
        None,
    )
    .await?;

    let journal = instance
        .journal()
        .render_with(RenderTableOpts {
            width: Some(80),
            no_colors: true,
            ..Default::default()
        })
        .collect::<Vec<String>>()
        .await
        .join("\n");

    // TODO automatically add memos for invoices that don't have them
    println!("{journal}");
    assert_snapshot!(journal, @r"
    Date       │ Particulars                       │       Debits │      Credits  
    2020-01-01 │ Business Checking                 │     1,000.00 │               
               │ Capital                           │              │     1,000.00  
               │ (Opening entry)                                                  
    2020-01-01 │ Operating Expenses                │        10.00 │               
               │ Accounts Payable                  │              │        10.00  
    2020-01-02 │ Accounts Payable                  │        10.00 │               
               │ Credit Card                       │              │        10.00  
               │ (Business Services)                                              
    2020-01-03 │ Operating Expenses                │        50.00 │               
               │ Business Checking                 │              │        50.00  
    2020-01-04 │ Operating Expenses                │        50.00 │               
               │ Accounts Payable                  │              │        50.00  
    2020-01-05 │ Accounts Receivable               │       100.00 │               
               │ Widget Sales                      │              │       100.00  
    2020-01-06 │ Business Checking                 │       100.00 │               
               │ Accounts Receivable               │              │       100.00  
               │ (Widget)                                                         
    2020-01-07 │ Business Checking                 │        30.00 │               
               │ Widget Sales                      │              │        30.00  
    2020-01-08 │ Accounts Receivable               │        10.00 │               
               │ Widget Sales                      │              │        10.00
    ");
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
    let instance = Accounts::new(DocSource::Str(JOURNAL.to_string()), None, None).await?;

    let journal = instance
        .journal()
        .render_with(RenderTableOpts {
            width: Some(80),
            no_colors: true,
            body_only: true,
            ..Default::default()
        })
        .collect::<Vec<String>>()
        .await
        .join("\n");

    println!("{journal}");
    assert_snapshot!(journal, @r"
    2020-01-01 │ Bank                              │    15,000.00 │               
               │ Owner Contributions               │              │    15,000.00  
               │ (Initial Contribution)
    ");
    Ok(())
}

/// Test ledger from entries
#[async_std::test]
async fn test_ledger() -> Result<()> {
    let instance = Accounts::new(
        DocSource::Path("./tests/fixtures/entries".to_string()),
        None,
        None,
    )
    .await?;
    let ledger = instance
        .ledger("Business Checking", Some(Debit))?
        .render_with(RenderTableOpts {
            width: Some(80),
            no_colors: true,
            ..Default::default()
        })
        .collect::<Vec<String>>()
        .await
        .join("\n");

    assert_snapshot!(ledger, @r"
    Date       │ Memo               │        Debit │       Credit │   Dr Balance  
    2020-01-01 │ Opening entry      │     1,000.00 │              │     1,000.00  
    2020-01-03 │                    │              │        50.00 │       950.00  
    2020-01-06 │ Widget             │       100.00 │              │     1,050.00  
    2020-01-07 │                    │        30.00 │              │     1,080.00
    ");
    Ok(())
}

// TODO check inverted balance type in ledger
// TODO check balance type from chart of accounts

/// Test balances from entries
#[async_std::test]
async fn test_balance() -> Result<()> {
    let instance = Accounts::new(
        DocSource::Path("./tests/fixtures/entries".to_string()),
        None,
        None,
    )
    .await?;
    let balances = instance.balances().await?.render_with(RenderTableOpts {
        width: Some(80),
        no_colors: true,
        ..Default::default()
    });

    println!("{balances}");
    // accounts are sorted by name
    assert_snapshot!(balances, @r"
    Account                                        │        Debit │       Credit  
    Accounts Payable                               │              │        50.00  
    Accounts Receivable                            │        10.00 │               
    Business Checking                              │     1,080.00 │               
    Capital                                        │              │     1,000.00  
    Credit Card                                    │              │        10.00  
    Operating Expenses                             │       110.00 │               
    Widget Sales                                   │              │       140.00  
    TOTAL                                          │     1,200.00 │     1,200.00
    ");
    Ok(())
}

#[async_std::test]
async fn test_balance_with_chart() -> Result<()> {
    let instance = Accounts::new(
        DocSource::Path("./tests/fixtures/entries".to_string()),
        None,
        Some(DocSource::Path(
            "./tests/fixtures/ChartOfAccounts.yaml".to_string(),
        )),
    )
    .await?;
    let balances = instance.balances().await?.render_with(RenderTableOpts {
        width: Some(80),
        no_colors: true,
        ..Default::default()
    });

    println!("{balances}");
    // accounts are sorted by class and position in chart of accounts
    assert_snapshot!(balances, @r"
    Account                                        │        Debit │       Credit  
    Business Checking                              │     1,080.00 │               
    Accounts Receivable                            │        10.00 │               
    Credit Card                                    │              │        10.00  
    Accounts Payable                               │              │        50.00  
    Capital                                        │              │     1,000.00  
    Widget Sales                                   │              │       140.00  
    Operating Expenses                             │       110.00 │               
    TOTAL                                          │     1,200.00 │     1,200.00
    ");
    Ok(())
}

#[async_std::test]
async fn test_balance_real() -> Result<()> {
    let instance = Accounts::new(
        DocSource::Path("./tests/fixtures/entries".to_string()),
        None,
        Some(DocSource::Path(
            "./tests/fixtures/ChartOfAccounts.yaml".to_string(),
        )),
    )
    .await?;
    let balances = instance
        // passing is_real true
        .balances_filtered(None, None, Some(true))
        .await?
        .render_with(RenderTableOpts {
            width: Some(80),
            no_colors: true,
            ..Default::default()
        });

    println!("{balances}");
    // accounts are sorted by class and position in chart of accounts
    assert_snapshot!(balances, @r"
    Account                                        │        Debit │       Credit  
    Business Checking                              │     1,080.00 │               
    Accounts Receivable                            │        10.00 │               
    Credit Card                                    │              │        10.00  
    Accounts Payable                               │              │        50.00  
    Capital                                        │              │     1,000.00  
    TOTAL                                          │     1,090.00 │     1,060.00  
    NET                                            │        30.00 │
    ");
    Ok(())
}

#[async_std::test]
async fn test_balance_nominal() -> Result<()> {
    let instance = Accounts::new(
        DocSource::Path("./tests/fixtures/entries".to_string()),
        None,
        Some(DocSource::Path(
            "./tests/fixtures/ChartOfAccounts.yaml".to_string(),
        )),
    )
    .await?;
    let balances = instance
        // passing is_real false
        .balances_filtered(None, None, Some(false))
        .await?
        .render_with(RenderTableOpts {
            width: Some(80),
            no_colors: true,
            ..Default::default()
        });

    println!("{balances}");
    // accounts are sorted by class and position in chart of accounts
    assert_snapshot!(balances, @r"
    Account                                        │        Debit │       Credit  
    Widget Sales                                   │              │       140.00  
    Operating Expenses                             │       110.00 │               
    TOTAL                                          │       110.00 │       140.00  
    NET                                            │              │        30.00
    ");
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
        DocSource::Str(JOURNAL.to_string()),
        Some("2020-03-31".parse()?),
        None,
    )
    .await?;
    let ledger = instance
        .ledger("Accounts Payable", Some(Credit))?
        .render_with(RenderTableOpts {
            width: Some(80),
            no_colors: true,
            ..Default::default()
        })
        .collect::<Vec<String>>()
        .await
        .join("\n");

    assert_snapshot!(ledger, @r"
    Date       │ Memo               │        Debit │       Credit │   Cr Balance  
    2020-01-02 │ Weekly bill        │              │        10.00 │        10.00  
    2020-01-03 │ Payment            │        50.00 │              │      (40.00)  
    2020-01-05 │ Monthly bill       │              │       100.00 │        60.00  
    2020-01-09 │ Weekly bill        │              │        10.00 │        70.00  
    2020-01-16 │ Weekly bill        │              │        10.00 │        80.00  
    2020-01-23 │ Weekly bill        │              │        10.00 │        90.00  
    2020-01-30 │ Weekly bill        │              │        10.00 │       100.00  
    2020-02-04 │ Payment            │       100.00 │              │         0.00  
    2020-02-05 │ Monthly bill       │              │       100.00 │       100.00  
    2020-02-06 │ Weekly bill        │              │        10.00 │       110.00  
    2020-02-13 │ Weekly bill        │              │        10.00 │       120.00  
    2020-02-20 │ Weekly bill        │              │        10.00 │       130.00  
    2020-02-27 │ Weekly bill        │              │        10.00 │       140.00  
    2020-03-05 │ Weekly bill        │              │        10.00 │       150.00  
    2020-03-05 │ Monthly bill       │              │       100.00 │       250.00  
    2020-03-06 │ Payment            │       100.00 │              │       150.00  
    2020-03-12 │ Weekly bill        │              │        10.00 │       160.00  
    2020-03-19 │ Weekly bill        │              │        10.00 │       170.00  
    2020-03-26 │ Weekly bill        │              │        10.00 │       180.00
    ");
    Ok(())
}

#[async_std::test]
async fn test_chart_of_accounts() -> Result<()> {
    let chart_of_accounts =
        ChartOfAccounts::from_file("./tests/fixtures/ChartOfAccounts.yaml").await?;
    dbg!(&chart_of_accounts);
    assert_eq!(
        chart_of_accounts.get("Operating Expenses").unwrap().class,
        Expense
    );
    assert_eq!(
        chart_of_accounts.get("Credit Card").unwrap().class,
        Liability
    );
    assert_eq!(
        chart_of_accounts.get("Business Checking").unwrap().class,
        Asset
    );
    assert_eq!(
        chart_of_accounts.get("Widget Sales").unwrap().class,
        Revenue
    );
    Ok(())
}

#[async_std::test]
async fn test_report() -> Result<()> {
    let report = ReportNode::from_file("./tests/fixtures/IncomeStatement.yaml").await?;
    let items = report.items()?;
    dbg!(&report);
    dbg!(&items);
    assert_eq!(
        items[2].0,
        vec!["Income Statement", "Revenue", "Direct Revenue"]
    );
    assert_eq!(
        items[3].0,
        vec!["Income Statement", "Revenue", "Indirect Revenue"]
    );
    assert_eq!(
        items[5].0,
        vec!["Income Statement", "Expenses", "Direct Expenses"]
    );
    assert_eq!(
        items[7].0,
        vec!["Income Statement", "Expenses", "Indirect Expenses", "Rent"]
    );
    Ok(())
}

#[async_std::test]
async fn test_run_report() -> Result<()> {
    let instance = Accounts::new(
        DocSource::Path("./tests/fixtures/entries".to_string()),
        None,
        None,
    )
    .await?;
    let chart_of_accounts =
        ChartOfAccounts::from_file("./tests/fixtures/ChartOfAccounts.yaml").await?;
    let mut report = ReportNode::from_file("./tests/fixtures/IncomeStatement.yaml").await?;
    // TODO run report with chart on instance
    instance.run_report(&chart_of_accounts, &mut report).await?;
    let items = report.items()?;
    dbg!(&items);
    println!("{report}");

    assert_snapshot!(report, @r"
    Income Statement                30.00
      Revenue                       140.00
        Direct Revenue              140.00
        Indirect Revenue            0.00
      Expenses                      110.00
        Direct Expenses             0.00
        Indirect Expenses           110.00
          Rent                      0.00
          Other                     110.00
    ");
    Ok(())
}
