use accounts::entry::Entry;
use accounts::*;
use anyhow::Result;
use futures::stream::TryStreamExt;
use itertools::Itertools;

#[async_std::test]
async fn test_basic_entries() -> Result<()> {
    let ledger = Ledger::new("./tests/fixtures/entries_flat");
    let entries = ledger.entries().await?.try_collect::<Vec<Entry>>().await?;
    dbg!(&entries);
    let count = entries.iter().map(|entry| entry.id()).unique().count();
    assert_eq!(count, 2);
    Ok(())
}

#[async_std::test]
async fn test_nested_dirs() -> Result<()> {
    let ledger = Ledger::new("./tests/fixtures/entries_nested_dirs");
    let entries = ledger.entries().await?.try_collect::<Vec<Entry>>().await?;
    dbg!(&entries);
    let count = entries.iter().map(|entry| entry.id()).unique().count();
    assert_eq!(count, 2);
    Ok(())
}

#[async_std::test]
async fn test_multiple_entries_in_one_file() -> Result<()> {
    let ledger = Ledger::new("./tests/fixtures/entries_multiple_entries_in_one_file");
    let entries = ledger.entries().await?.try_collect::<Vec<Entry>>().await?;
    dbg!(&entries);
    let count = entries.iter().map(|entry| entry.id()).unique().count();
    assert_eq!(count, 2);
    Ok(())
}

#[async_std::test]
async fn test_journal_from_entries() -> Result<()> {
    let mut ledger = Ledger::new("./tests/fixtures/entries");
    ledger
        .chart_of_accounts
        .create_bank_account("Business Checking", "000000");
    ledger
        .chart_of_accounts
        .create_credit_card_account("Credit Card", "000000");

    let journal_entry_strings: Vec<String> = ledger
        .journal()
        .await?
        .map_ok(|journal_entry| journal_entry.to_string())
        .try_collect()
        .await?;

    assert_eq!(dbg!(&journal_entry_strings).iter().count(), 16);
    let display = journal_entry_strings.iter().join("\n");
    assert_eq!(
        display,
        "| 3000-01-01 | Operating Expenses        |      $100.00 |              |\n\
         | 3000-01-01 | Accounts Payable          |              |      $100.00 |\n\
         | 3000-01-02 | Credit Card               |              |      $100.00 |\n\
         | 3000-01-02 | Accounts Payable          |      $100.00 |              |\n\
         | 3000-01-03 | Operating Expenses        |       $50.00 |              |\n\
         | 3000-01-03 | Business Checking         |              |       $50.00 |\n\
         | 3000-01-04 | Operating Expenses        |      $100.00 |              |\n\
         | 3000-01-04 | Accounts Payable          |              |      $100.00 |\n\
         | 3000-01-05 | Widget Sales              |              |       $10.00 |\n\
         | 3000-01-05 | Accounts Receivable       |       $10.00 |              |\n\
         | 3000-01-06 | Business Checking         |       $10.00 |              |\n\
         | 3000-01-06 | Accounts Receivable       |              |       $10.00 |\n\
         | 3000-01-07 | Widget Sales              |              |        $5.00 |\n\
         | 3000-01-07 | Business Checking         |        $5.00 |              |\n\
         | 3000-01-08 | Widget Sales              |              |       $10.00 |\n\
         | 3000-01-08 | Accounts Receivable       |       $10.00 |              |"
    );
    Ok(())
}

fn contains_balance(balances_strings: &Vec<(String, String)>, account: &str, amount: &str) -> bool {
    balances_strings.contains(&(String::from(account), String::from(amount)))
}

#[async_std::test]
async fn test_balance() -> Result<()> {
    let mut ledger = Ledger::new("./tests/fixtures/entries");
    ledger
        .chart_of_accounts
        .create_bank_account("Business Checking", "000000");
    ledger
        .chart_of_accounts
        .create_credit_card_account("Credit Card", "000000");

    let balances = ledger.balances().await?;
    let balances_strings: Vec<(String, String)> = balances
        .iter()
        .map(|(account, amount)| (account.to_string(), amount.to_string()))
        .collect();
    dbg!(&balances_strings);
    assert_eq!(balances_strings.len(), 6);
    assert!(contains_balance(
        &balances_strings,
        "Operating Expenses",
        "$250.00"
    ));
    assert!(contains_balance(
        &balances_strings,
        "Accounts Payable",
        "$100.00"
    ));
    assert!(contains_balance(
        &balances_strings,
        "Credit Card",
        "$100.00"
    ));
    assert!(contains_balance(
        &balances_strings,
        "Business Checking",
        "($35.00)"
    ));
    assert!(contains_balance(
        &balances_strings,
        "Widget Sales",
        "$25.00"
    ));
    assert!(contains_balance(
        &balances_strings,
        "Accounts Receivable",
        "$10.00"
    ));
    Ok(())
}
