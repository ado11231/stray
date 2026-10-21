//! `stray install` and `stray uninstall`: add or remove one startup line in shell settings.

use crate::env;
use anyhow::{Context, Result, bail};
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

/// Ends the startup line, so uninstall can find exactly the line Stray added.
const LINE_TAG: &str = "# stray";

/// Added to a settings file's name while its new contents are written.
const TEMPORARY_EXTENSION: &str = "stray.tmp";

/// Login bash reads the first of these that exists.
const BASH_PROFILES: [&str; 3] = [".bash_profile", ".bash_login", ".profile"];

/// Adds the startup line when `install` is true, and removes it otherwise.
pub fn run(install: bool) -> Result<()> {
    let shell = env::shell()?;
    let shell = Path::new(&shell).file_name().and_then(|name| name.to_str());
    let home = PathBuf::from(env::home()?);
    let (name, paths, guard) = match shell {
        Some("zsh") => {
            let directory = std::env::var_os(env::ZDOTDIR)
                .map(PathBuf::from)
                .unwrap_or(home);
            ("zsh", vec![directory.join(".zshrc")], "")
        }
        // A .profile can also be read by sh, so only bash runs the line.
        Some("bash") => (
            "bash",
            bash_paths(&home, install),
            "[ -n \"${BASH_VERSION-}\" ] && ",
        ),
        _ => bail!("install and uninstall support bash and zsh only"),
    };
    let line = startup_line(name, guard);
    for path in paths {
        if edit_startup(&path, &line, install)
            .with_context(|| format!("could not update {}", path.display()))?
        {
            println!("stray: updated {}", path.display());
        }
    }
    if install {
        println!("stray: installed; open a new terminal");
    } else {
        println!("stray: uninstalled; existing sessions stay active");
    }
    Ok(())
}

/// The line that starts Stray, which does nothing when stray is not on the PATH.
fn startup_line(shell_name: &str, guard: &str) -> String {
    format!(
        "if {guard}command -v stray >/dev/null 2>&1; then eval \"$(stray init {shell_name})\"; fi {LINE_TAG}"
    )
}

/// The bash settings files to change. Uninstall checks every profile.
fn bash_paths(home: &Path, install: bool) -> Vec<PathBuf> {
    let profiles = BASH_PROFILES.map(|name| home.join(name));
    let mut paths = vec![home.join(".bashrc")];
    if install {
        // Login bash reads the first available profile, and may never read .bashrc.
        let profile = profiles
            .into_iter()
            .find(|path| path.is_file())
            .unwrap_or_else(|| home.join(BASH_PROFILES[0]));
        paths.push(profile);
    } else {
        paths.extend(profiles);
    }
    paths
}

/// Returns the file contents with the line added once, or with every copy removed.
fn updated_contents(contents: &str, line: &str, install: bool) -> String {
    if !install {
        return contents
            .split_inclusive('\n')
            .filter(|existing| existing.trim_end_matches(['\r', '\n']) != line)
            .collect();
    }
    let mut updated = contents.to_owned();
    if !contents.lines().any(|existing| existing == line) {
        // Match the file's own line endings.
        let newline = if contents.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        if !updated.is_empty() && !updated.ends_with('\n') {
            updated.push_str(newline);
        }
        updated.push_str(line);
        updated.push_str(newline);
    }
    updated
}

/// Changes one settings file safely, and returns true if anything changed.
///
/// The new contents go to a temporary file first, which then replaces the original, so a
/// failure never leaves a half-written file behind.
fn edit_startup(path: &Path, line: &str, install: bool) -> Result<bool> {
    // Follow dotfile links so replacing the file does not replace the link itself.
    let target = if path.is_symlink() {
        fs::canonicalize(path)?
    } else {
        path.to_path_buf()
    };
    let contents = match fs::read_to_string(&target) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let updated = updated_contents(&contents, line, install);
    if updated == contents {
        return Ok(false);
    }
    let permissions = fs::metadata(&target).map(|metadata| metadata.permissions());
    let temporary = target.with_extension(TEMPORARY_EXTENSION);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    let result = (|| -> Result<()> {
        file.write_all(updated.as_bytes())?;
        if let Ok(permissions) = permissions {
            file.set_permissions(permissions)?;
        }
        file.sync_all()?;
        fs::rename(&temporary, &target)?;
        Ok(())
    })();
    let _ = fs::remove_file(&temporary);
    result?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    const LINE: &str = "startup # stray";

    /// A temporary folder that is removed when the test ends.
    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Result<Self> {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos();
            let path =
                std::env::temp_dir().join(format!("stray-install-{}-{unique}", std::process::id()));
            fs::create_dir(&path)?;
            Ok(Self(path))
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn startup_line_ends_with_the_tag() {
        assert!(startup_line("zsh", "").ends_with(LINE_TAG));
    }

    #[test]
    fn edits_preserve_settings_and_do_not_duplicate_startup() {
        for contents in ["", "export X=1\n", "export X=1", "export X=1\r\n"] {
            let added = updated_contents(contents, LINE, true);
            assert!(added.starts_with(contents));
            assert_eq!(updated_contents(&added, LINE, true), added);

            let removed = updated_contents(&added, LINE, false);
            // A missing final newline was added along with the line.
            let expected = if contents == "export X=1" {
                "export X=1\n"
            } else {
                contents
            };
            assert_eq!(removed, expected);
            assert_eq!(updated_contents(&removed, LINE, false), removed);
        }
    }

    #[test]
    fn file_updates_preserve_links_permissions_and_missing_files() -> Result<()> {
        let directory = TestDirectory::new()?;
        let target = directory.0.join("settings");
        let link = directory.0.join(".zshrc");
        assert!(!edit_startup(&target, LINE, false)?);
        assert!(!target.exists());

        fs::write(&target, "export X=1\n")?;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o640))?;
        symlink(&target, &link)?;
        assert!(edit_startup(&link, LINE, true)?);
        assert!(!edit_startup(&link, LINE, true)?);
        assert!(link.is_symlink());
        assert_eq!(fs::metadata(&target)?.permissions().mode() & 0o777, 0o640);

        assert!(edit_startup(&link, LINE, false)?);
        assert_eq!(fs::read_to_string(&target)?, "export X=1\n");

        // A leftover temporary file stops the edit instead of being overwritten.
        let temporary = target.with_extension(TEMPORARY_EXTENSION);
        fs::write(&temporary, "existing temporary file")?;
        assert!(edit_startup(&link, LINE, true).is_err());
        assert_eq!(fs::read_to_string(&target)?, "export X=1\n");
        assert_eq!(fs::read_to_string(&temporary)?, "existing temporary file");
        Ok(())
    }
}
