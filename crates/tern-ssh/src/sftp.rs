//! Single-file copy over the SFTP subsystem of a shared connection.

use std::path::{Path, PathBuf};

use russh_sftp::client::SftpSession;
use tokio::io::AsyncWriteExt;

use crate::{Error, SharedConn};

/// Errors from a file copy.
#[derive(Debug, thiserror::Error)]
pub enum SftpError {
    #[error(transparent)]
    Ssh(#[from] Error),
    #[error("sftp: {0}")]
    Sftp(String),
    #[error("{0} already exists")]
    Exists(String),
    #[error("{path}: {source}")]
    Local {
        path: String,
        source: std::io::Error,
    },
}

fn sftp<E: std::fmt::Display>(e: E) -> SftpError {
    SftpError::Sftp(e.to_string())
}

fn local(path: &Path) -> impl FnOnce(std::io::Error) -> SftpError + '_ {
    move |source| SftpError::Local {
        path: path.display().to_string(),
        source,
    }
}

async fn open(conn: &SharedConn) -> Result<SftpSession, SftpError> {
    let channel = conn
        .handle()?
        .channel_open_session()
        .await
        .map_err(Error::from)?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(Error::from)?;
    SftpSession::new(channel.into_stream()).await.map_err(sftp)
}

/// Where a download is staged before the final rename.
fn part_path(local: &Path) -> PathBuf {
    let mut name = local.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

/// Copy `remote` to `local`. Refuses to replace an existing local file. Returns the bytes copied.
pub async fn download(
    conn: &SharedConn,
    remote: &str,
    local_path: &Path,
) -> Result<u64, SftpError> {
    if tokio::fs::try_exists(local_path).await.unwrap_or(false) {
        return Err(SftpError::Exists(local_path.display().to_string()));
    }
    let session = open(conn).await?;
    let result = async {
        let mut src = session.open(remote).await.map_err(sftp)?;
        let part = part_path(local_path);
        let mut dst = tokio::fs::File::create(&part).await.map_err(local(&part))?;
        let copied = tokio::io::copy(&mut src, &mut dst).await;
        let finished = match copied {
            Ok(n) => dst.flush().await.map(|()| n),
            Err(e) => Err(e),
        };
        drop(dst);
        match finished {
            Ok(n) => {
                tokio::fs::rename(&part, local_path)
                    .await
                    .map_err(local(local_path))?;
                Ok(n)
            }
            Err(e) => {
                let _ = tokio::fs::remove_file(&part).await;
                Err(sftp(e))
            }
        }
    }
    .await;
    let _ = session.close().await;
    result
}

/// Copy `local_path` to `remote`, replacing the remote file. Returns the bytes copied.
pub async fn upload(conn: &SharedConn, local_path: &Path, remote: &str) -> Result<u64, SftpError> {
    let mut src = tokio::fs::File::open(local_path)
        .await
        .map_err(local(local_path))?;
    let session = open(conn).await?;
    let result = async {
        let mut dst = session.create(remote).await.map_err(sftp)?;
        let n = tokio::io::copy(&mut src, &mut dst).await.map_err(sftp)?;
        dst.shutdown().await.map_err(sftp)?;
        Ok(n)
    }
    .await;
    let _ = session.close().await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_file_sits_next_to_the_target() {
        assert_eq!(
            part_path(Path::new("/tmp/a/file.tar.gz")),
            PathBuf::from("/tmp/a/file.tar.gz.part")
        );
    }
}
