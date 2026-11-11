//! The lock that lets one daemon run at a time.

use std::fs::{self, File, TryLockError};
use std::io::Write;
use std::path::Path;

use color_eyre::eyre::{Result, WrapErr, bail};

/// Takes the lock at `path` for as long as the returned file is open, and
/// writes this process's id in it.
///
/// # Errors
///
/// Another daemon holds it, named by its process id.
pub fn take(path: &Path) -> Result<File> {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }

    let mut file = File::options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .wrap_err_with(|| format!("opening {}", path.display()))?;

    match file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            let holder = fs::read_to_string(path).unwrap_or_default();
            bail!(
                "an atelier daemon is already running (process {})",
                holder.trim()
            );
        }
        Err(TryLockError::Error(error)) => return Err(error).wrap_err("taking the daemon lock"),
    }

    file.set_len(0)?;
    write!(file, "{}", std::process::id())?;

    Ok(file)
}
