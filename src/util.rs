use displaydoc::Display;
use std::{ffi, fs, io, path};
use thiserror::Error;

/// Error while iterating contents of directory.
#[derive(Debug, Display, Error)]
pub enum DirReadError {
    /// Could not open directory for reading: {0}
    DirReadFailed(io::Error),
    /// Could not read entry from directory: {0}
    CouldNotReadEntry(io::Error),
    /// Could not convert {0:?} into string.
    NonUnicodeFilename(ffi::OsString),
}

/// Return a sorted list of filepaths of all files in the given `root` that match `filter`.
///
/// Returns complete, absolute paths.
// Note: Filtered dir enumeration is hard with lots of nesting, which is why this method exists.
pub fn read_dir_matching<P: AsRef<path::Path>>(
    root: P,
    filter: &regex::Regex,
) -> Result<Vec<path::PathBuf>, DirReadError> {
    let base = root.as_ref();
    let mut paths = Vec::new();

    for dir_result in fs::read_dir(base).map_err(DirReadError::DirReadFailed)? {
        let dir_entry = dir_result.map_err(DirReadError::CouldNotReadEntry)?;
        let filename = dir_entry
            .file_name()
            .into_string()
            .map_err(DirReadError::NonUnicodeFilename)?;

        if filter.is_match(&filename) {
            paths.push(base.join(filename));
        }
    }

    paths.sort();
    Ok(paths)
}

/// An iterator that yields an initial error or a stream of results.
///
/// This `enum` is created by the `flatten_results` function. See its documentation for more.
#[derive(Debug)]
pub enum FlattenResult<I, E> {
    Failed(Option<E>),
    Inner(I),
}

/// Helper trait for `flatten_results`.
///
/// In an ideal world, `FlattenResultsIter` would be a method of `Result`.
pub trait FlattenResultsIter {
    type Iter;
    type Error;

    /// Turn a result of an iterator of results into just an iterator of results.
    ///
    /// If a `Result` contains an `Iterator` of `Result`s, this function will turn the result into
    /// a flat iterator, similar to `flatten`. When iterated over, it will return the error from
    /// the top level `Result`, if there was any and stop. If there was no error, the results from
    /// the inner iterator are passed through
    fn flatten_results(self) -> FlattenResult<Self::Iter, Self::Error>;
}

impl<T, I, E> FlattenResultsIter for ::std::result::Result<I, E>
where
    I: Iterator<Item = ::std::result::Result<T, E>>,
{
    type Iter = I;
    type Error = E;

    fn flatten_results(self) -> FlattenResult<Self::Iter, Self::Error> {
        match self {
            Ok(inner) => FlattenResult::Inner(inner),
            Err(e) => FlattenResult::Failed(Some(e)),
        }
    }
}

impl<T, I, E> Iterator for FlattenResult<I, E>
where
    I: Iterator<Item = ::std::result::Result<T, E>>,
{
    type Item = ::std::result::Result<T, E>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            FlattenResult::Failed(err) => err.take().map(Err),
            FlattenResult::Inner(inner) => inner.next(),
        }
    }
}
