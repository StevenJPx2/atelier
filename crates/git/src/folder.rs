//! Folders named after a repository, kept inside their root.

use atelier_core::{PluginError, Repo};

/// The repository's owner and name, each safe as a folder's name.
///
/// # Errors
///
/// The repository is not `owner/name`, or a part could leave its root.
pub fn owner_and_name(repo: &Repo) -> Result<(&str, &str), PluginError> {
    let mut parts = repo.0.split('/');

    let (Some(owner), Some(name), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err(PluginError::permanent(format!(
            "{} is not owner/name",
            repo.0
        )));
    };

    for part in [owner, name] {
        checked(part)?;
    }

    Ok((owner, name))
}

/// `part`, when it is safe as one folder's name.
///
/// # Errors
///
/// It holds anything but letters, digits, `-`, `_` and `.`, or only dots.
pub fn checked(part: &str) -> Result<&str, PluginError> {
    let allowed = |c: char| c.is_ascii_alphanumeric() || "-_.".contains(c);

    if !part.is_empty() && part.chars().all(allowed) && part.chars().any(|c| c != '.') {
        Ok(part)
    } else {
        Err(PluginError::permanent(format!(
            "{part:?} cannot name a folder"
        )))
    }
}
