use accounts::entries_from_files;
use accounts::entry::Entry;
use anyhow::Result;
use futures::stream::TryStreamExt;
use itertools::Itertools;

#[async_std::test]
async fn test() -> Result<()> {
    let entries = entries_from_files("./tests/fixtures/entries")
        .await?
        .try_collect::<Vec<Entry>>()
        .await?;
    dbg!(&entries);
    assert_eq!(entries.len(), 1);
    Ok(())
}

#[async_std::test]
async fn test_nested_dirs() -> Result<()> {
    let entries = entries_from_files("./tests/fixtures/entries_nested_dirs")
        .await?
        .try_collect::<Vec<Entry>>()
        .await?;
    dbg!(&entries);
    assert_eq!(entries.len(), 1);
    Ok(())
}

#[async_std::test]
async fn test_multiple_entries_in_one_file() -> Result<()> {
    let entries = entries_from_files("./tests/fixtures/entries.yaml")
        .await?
        .try_collect::<Vec<Entry>>()
        .await?;
    dbg!(&entries);
    let count = entries.iter().map(|entry| entry.id()).unique().count();
    assert_eq!(count, 2);
    Ok(())
}
