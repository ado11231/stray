//! Explains once, ever, why the cat is missing when the terminal cannot show images.

use directories::ProjectDirs;
use std::fs::{self, OpenOptions};
use std::path::Path;

/// An empty file whose presence means the hint was already shown.
const MARKER_FILE: &str = "image-hint-shown";

const HINT: &str = "stray: images unavailable; enable terminal images or tmux allow-passthrough; starting shell without a cat";

/// Prints the hint the first time it is called on this machine.
pub fn show_once() {
    let Some(directories) = ProjectDirs::from("", "", "stray") else {
        return;
    };
    if claim(directories.data_dir()) {
        eprintln!("{HINT}");
    }
}

/// Creates the marker file, and returns true only for the caller that created it.
fn claim(directory: &Path) -> bool {
    // A separate marker avoids overwriting settings when two windows start together.
    if fs::create_dir_all(directory).is_err() {
        return false;
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join(MARKER_FILE))
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_windows_claim_the_hint_only_once() -> anyhow::Result<()> {
        let directory = std::env::temp_dir().join(format!(
            "stray-image-hint-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        let claimed = std::thread::scope(|scope| -> anyhow::Result<usize> {
            let attempts: Vec<_> = (0..8).map(|_| scope.spawn(|| claim(&directory))).collect();
            let mut claimed = 0;
            for thread in attempts {
                claimed += usize::from(
                    thread
                        .join()
                        .map_err(|_| anyhow::anyhow!("hint thread panicked"))?,
                );
            }
            Ok(claimed)
        })?;
        assert_eq!(claimed, 1);
        assert!(!claim(&directory));
        fs::remove_dir_all(directory)?;
        Ok(())
    }

    #[test]
    fn unavailable_state_does_not_block_startup() {
        assert!(!claim(Path::new("/dev/null/stray")));
    }
}
