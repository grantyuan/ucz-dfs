use crate::config::Config;
use crate::error::Result;

use std::fs::OpenOptions;
use std::io::SeekFrom;

use serde::{Deserialize, Serialize};
use serde_json;

use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufStream};

/// A namenode modification.
#[derive(Debug, Deserialize, Serialize)]
pub enum EditOperation {
    /// A created directory
    Mkdir(String),
}

/// EditLog is responsible to log all namenode modifications.
pub struct EditLog {
    old_log: BufStream<File>,
    new_log: BufStream<File>,
}

impl EditLog {
    /// Opens old edit logs (if exists) and creates new ones.
    pub fn open(config: &Config) -> Result<Self> {
        let old_log = config.namenode.name_dir.join("edits");
        let old_log = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(old_log)?;

        let new_log = config.namenode.name_dir.join("new-edits");
        let new_log = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(new_log)?;

        Ok(Self {
            old_log: BufStream::new(File::from_std(old_log)),
            new_log: BufStream::new(File::from_std(new_log)),
        })
    }

    /// Returns a list of EditOperations which are needed to restore
    /// the state of the namenode.
    pub async fn restore(&mut self) -> Result<Vec<EditOperation>> {
        self.merge().await?;

        let mut ops = vec![];
        let mut buffer = String::new();
        loop {
            match self.old_log.read_line(&mut buffer).await? {
                0 => break,
                _ => {
                    let op: EditOperation = serde_json::from_str(&buffer)
                        .ok()
                        .ok_or_else(|| {
                            format!("Could not deserialize '{}'. Is the log corrupted?", buffer)
                        })
                        .unwrap();
                    ops.push(op);
                    buffer.clear();
                }
            }
        }

        Ok(ops)
    }

    /// Merges the new log stream into the old one. New one gets truncated.
    async fn merge(&mut self) -> Result<()> {
        self.new_log.flush().await?;

        self.old_log.get_mut().seek(SeekFrom::End(0)).await?;
        self.new_log.get_mut().seek(SeekFrom::Start(0)).await?;

        tokio::io::copy(&mut self.new_log, &mut self.old_log).await?;

        self.new_log.get_mut().set_len(0).await?;
        self.new_log.get_mut().seek(SeekFrom::Start(0)).await?;
        self.old_log.get_mut().seek(SeekFrom::Start(0)).await?;

        Ok(())
    }
}
