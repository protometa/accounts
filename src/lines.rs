use async_std::fs::{self, File};
use async_std::io::prelude::*;
use async_std::io::{BufReader, stdin};
use async_walkdir::{DirEntry, WalkDir};
use futures::FutureExt;
use futures::stream::{Stream, StreamExt, TryStreamExt};
use std::io::{Error, ErrorKind, Result};
use std::path::Path;

/// Reads an entire dir of files by line
fn dir_lines(dir: &str) -> impl Stream<Item = Result<String>> + use<> {
    WalkDir::new(dir)
        .try_filter_map(|dir_entry: DirEntry| async move {
            let path = dir_entry.path();
            let filestem = path
                .file_stem()
                .ok_or_else(|| Error::new(ErrorKind::Other, "No file stem"))?
                .to_string_lossy();
            if path.is_dir() || filestem.starts_with('.') {
                return Ok(None);
            };
            File::open(&path).await.map(Option::Some)
        })
        .map_ok(|file| BufReader::new(file).lines())
        .try_flatten()
}

/// Reads dir or file by line
async fn dir_or_file_lines(pathstr: String) -> Result<impl Stream<Item = Result<String>>> {
    let path = Path::new(&pathstr);
    if path.exists() {
        let metadata = fs::metadata(path).await?;
        if metadata.is_file() {
            let file = File::open(&pathstr).await?;
            Ok(BufReader::new(file).lines().left_stream())
        } else if metadata.is_dir() {
            Ok(dir_lines(&pathstr).right_stream())
        } else {
            Err(Error::new(
                ErrorKind::InvalidInput,
                "The path is neither a file nor a directory.",
            ))
        }
    } else {
        Err(Error::new(ErrorKind::NotFound, "The path does not exist."))
    }
}

/// Reads lines of given dir or file, or stdin if None
pub fn lines(path: Option<String>) -> impl Stream<Item = Result<String>> {
    if let Some(pathstr) = path.clone() {
        dir_or_file_lines(pathstr)
            .into_stream()
            .try_flatten()
            .left_stream()
    } else {
        BufReader::new(stdin()).lines().right_stream()
    }
}
