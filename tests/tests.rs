use accounts::entries_from_files;
use accounts::entry::Entry;
use anyhow::Result;
use futures::stream::TryStreamExt;

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
