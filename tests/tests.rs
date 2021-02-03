use accounts::entry::Entry;
use accounts::journal_entry::JournalEntry;
use accounts::*;
use anyhow::Result;
use futures::stream::TryStreamExt;
use itertools::Itertools;

#[async_std::test]
async fn test_basic_entries() -> Result<()> {
    let entries = entries_from_files("./tests/fixtures/entries_flat")
        .await?
        .try_collect::<Vec<Entry>>()
        .await?;
    dbg!(&entries);
    let count = entries
        .iter()
        .map(|entry| entry.id.clone())
        .unique()
        .count();
    assert_eq!(count, 2);
    Ok(())
}

#[async_std::test]
async fn test_nested_dirs() -> Result<()> {
    let entries = entries_from_files("./tests/fixtures/entries_nested_dirs")
        .await?
        .try_collect::<Vec<Entry>>()
        .await?;
    dbg!(&entries);
    let count = entries
        .iter()
        .map(|entry| entry.id.clone())
        .unique()
        .count();
    assert_eq!(count, 2);
    Ok(())
}

#[async_std::test]
async fn test_multiple_entries_in_one_file() -> Result<()> {
    let entries = entries_from_files("./tests/fixtures/entries_multiple_entries_in_one_file")
        .await?
        .try_collect::<Vec<Entry>>()
        .await?;
    dbg!(&entries);
    let count = entries
        .iter()
        .map(|entry| entry.id.clone())
        .unique()
        .count();
    assert_eq!(count, 2);
    Ok(())
}

#[async_std::test]
async fn test_journal_from_entries() -> Result<()> {
    let entries = entries_from_files("./tests/fixtures/entries").await?;
    let journal: Vec<JournalEntry> = journal(entries).try_collect().await?;
    dbg!(&journal);
    let count = journal.iter().count();
    assert_eq!(count, 4);
    Ok(())
}
