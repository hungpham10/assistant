use std::io;
use std::path::Path;
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Minimal synchronous filesystem surface for components that run before or
/// outside of a session's [`crate::ExecutorFileSystem`] environment (boot-time
/// setup, background services). Implementations are expected to operate on the
/// local host; a remote backend can be introduced by implementing this trait.
pub trait SystemFs: Send + Sync {
    /// Reads a file and decodes it as UTF-8 text.
    fn read_text(&self, path: &Path) -> io::Result<String> {
        let bytes = self.read_bytes(path)?;
        String::from_utf8(bytes).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
    }

    /// Reads a file's full contents as bytes.
    fn read_bytes(&self, path: &Path) -> io::Result<Vec<u8>>;

    /// Writes a file, replacing it if it already exists.
    fn write(&self, path: &Path, contents: &[u8]) -> io::Result<()>;

    /// Writes a file readable only by the current user (mode 0600 on unix),
    /// creating parent directories as needed.
    fn write_private(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            self.create_dir_all(parent)?;
        }
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;

            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .mode(0o600)
                .open(path)?;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
            file.write_all(contents)?;
            file.sync_all()?;
            Ok(())
        }
        #[cfg(not(unix))]
        {
            self.write(path, contents)
        }
    }

    /// Creates a directory and all of its missing parents.
    fn create_dir_all(&self, path: &Path) -> io::Result<()>;

    /// Lists the entries of a directory.
    fn list_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>>;

    /// Removes a file; fails if it does not exist.
    fn remove_file(&self, path: &Path) -> io::Result<()>;

    /// Atomically renames (moves) `from` to `to`.
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
}

/// [`SystemFs`] implementation backed by the local host's `std::fs`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StdSystemFs;

impl SystemFs for StdSystemFs {
    fn read_bytes(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    fn write(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        std::fs::write(path, contents)
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }

    fn list_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        Ok(std::fs::read_dir(path)?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .collect())
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        std::fs::rename(from, to)
    }
}

#[cfg(test)]
#[path = "system_fs_tests.rs"]
mod tests;
