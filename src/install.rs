use anyhow::{Context, Result, bail};
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

pub fn run(install: bool) -> Result<()> {
    let shell = std::env::var_os("SHELL").context("SHELL is not set")?;
    let shell = Path::new(&shell).file_name().and_then(|name| name.to_str());
    let home = PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?);
    let (name, paths) = match shell {
        Some("zsh") => {
            let directory = std::env::var_os("ZDOTDIR")
                .map(PathBuf::from)
                .unwrap_or(home);
            ("zsh", vec![directory.join(".zshrc")])
        }
        Some("bash") => ("bash", bash_paths(&home, install)),
        _ => bail!("install and uninstall support bash and zsh only"),
    };
    let guard = match name {
        "bash" => "[ -n \"${BASH_VERSION-}\" ] && ",
        _ => "",
    };
    let line = format!(
        "if {guard}command -v stray >/dev/null 2>&1; then eval \"$(stray init {name})\"; fi # stray"
    );
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

fn bash_paths(home: &Path, install: bool) -> Vec<PathBuf> {
    let profiles = [".bash_profile", ".bash_login", ".profile"].map(|name| home.join(name));
    let mut paths = vec![home.join(".bashrc")];
    if install {
        // Login bash reads the first available profile, and may never read .bashrc.
        let profile = profiles
            .iter()
            .find(|path| path.is_file())
            .unwrap_or(&profiles[0]);
        paths.push(profile.clone());
    } else {
        paths.extend(profiles);
    }
    paths
}

fn updated_contents(contents: &str, line: &str, install: bool) -> String {
    if !install {
        return contents
            .split_inclusive('\n')
            .filter(|existing| existing.trim_end_matches(['\r', '\n']) != line)
            .collect();
    }
    let mut updated = contents.to_owned();
    if !contents.lines().any(|existing| existing == line) {
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
    let temporary = target.with_extension("stray.tmp");
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

    struct TestDirectory(PathBuf);
    impl TestDirectory {
        fn new() -> Self {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path =
                std::env::temp_dir().join(format!("stray-install-{}-{unique}", std::process::id()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn edits_preserve_settings_and_do_not_duplicate_startup() {
        for contents in ["", "export X=1\n", "export X=1", "export X=1\r\n"] {
            let added = updated_contents(contents, "startup # stray", true);
            assert!(added.starts_with(contents));
            assert_eq!(updated_contents(&added, "startup # stray", true), added);
            let removed = updated_contents(&added, "startup # stray", false);
            let expected = if contents == "export X=1" {
                "export X=1\n"
            } else {
                contents
            };
            assert_eq!(removed, expected);
            assert_eq!(
                updated_contents(&removed, "startup # stray", false),
                removed
            );
        }
    }

    #[test]
    fn file_updates_preserve_links_permissions_and_missing_files() {
        let directory = TestDirectory::new();
        let target = directory.0.join("settings");
        let link = directory.0.join(".zshrc");
        assert!(!edit_startup(&target, "startup # stray", false).unwrap());
        assert!(!target.exists());
        fs::write(&target, "export X=1\n").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
        symlink(&target, &link).unwrap();
        assert!(edit_startup(&link, "startup # stray", true).unwrap());
        assert!(!edit_startup(&link, "startup # stray", true).unwrap());
        assert!(link.is_symlink());
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o640
        );
        assert!(edit_startup(&link, "startup # stray", false).unwrap());
        assert_eq!(fs::read_to_string(&target).unwrap(), "export X=1\n");
        let temporary = target.with_extension("stray.tmp");
        fs::write(&temporary, "existing temporary file").unwrap();
        assert!(edit_startup(&link, "startup # stray", true).is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "export X=1\n");
        assert_eq!(
            fs::read_to_string(&temporary).unwrap(),
            "existing temporary file"
        );
    }
}
