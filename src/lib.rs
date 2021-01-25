pub mod entry;
mod money;

use anyhow::{Context, Error, Result};
use async_std::fs;
use async_walkdir::{DirEntry, WalkDir};
use entry::raw_entry::RawEntry;
use entry::Entry;
use futures::stream::{self, StreamExt, TryStreamExt};
use std::convert::TryInto;
use std::path::PathBuf;

pub async fn entries_from_files(dir: &str) -> Result<impl TryStreamExt<Ok = Entry, Error = Error>> {
    let dir1 = dir.to_owned();
    let dir2 = dir.to_owned();
    Ok(WalkDir::new(dir)
        .map_ok(|dir_entry| dir_entry.path())
        .map(move |result| match result {
            Err(error) => match error.raw_os_error() {
                Some(20) => Ok(PathBuf::from(&dir1)),
                _ => Err(error),
            },
            _ => result,
        })
        .map_err(Error::new) // map to anyhow::Error from here o
        .try_filter_map(move |path: PathBuf| {
            let dir2 = dir2.clone();
            async move {
                let filename = path
                    .file_name()
                    .context("can't get filename")?
                    .to_string_lossy();
                if path.is_dir() || filename.starts_with(".") {
                    return Ok(None);
                };

                let content = fs::read_to_string(&path).await?;
                let docs: Vec<String> = content
                    .trim_start_matches("---")
                    .split("---")
                    // only way I can tell to avoid returning reference to content
                    .map(ToOwned::to_owned)
                    .collect();
                let sub_stream = stream::iter(docs).map(move |yaml: String| -> Result<_> {
                    let subpath = path.strip_prefix(&dir2)?.to_owned();
                    Ok((dbg!(subpath), yaml))
                });

                Ok(Some(sub_stream))
            }
        })
        .try_flatten()
        .and_then(|(path, yaml)| async move {
            let mut raw_entry: RawEntry = serde_yaml::from_str(&yaml)
                .context(format!("Failed to deserialize entry in {}", path.display()))?;
            raw_entry
                .id
                .get_or_insert(path.to_string_lossy().into_owned());
            let entry: Entry = raw_entry.try_into()?;
            Ok(entry)
        }))
}

pub async fn balance(entries: impl TryStreamExt<Ok = Entry, Error = Error>) -> Result<()> {
    entries
        .try_for_each(|entry: Entry| async {
            dbg!(entry);
            Ok(())
        })
        .await?;
    Ok(())
}
