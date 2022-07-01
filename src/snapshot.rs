use derive_more::{Deref, Into};
use std::fs::File;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SnapshotFileOpenError {
    #[error(
          "Encountered and IO error while opening snapshot file '{0}' ({})",
          .1.kind()
      )]
    Io(PathBuf, #[source] std::io::Error),
}

#[derive(Debug, Into, Deref)]
pub struct SnapshotFile(File);

impl SnapshotFile {
    pub fn open<P: AsRef<Path>>(p: P) -> Result<Self, SnapshotFileOpenError> {
        Ok(Self(File::open(p.as_ref()).map_err(|e| {
            SnapshotFileOpenError::Io(p.as_ref().to_path_buf(), e)
        })?))
    }
}
