use directories::ProjectDirs;
use std::fs::{self, OpenOptions};
use std::path::Path;

pub fn show_once() {
    let Some(directories) = ProjectDirs::from("", "", "stray") else {
        return;
    };
    if claim(directories.data_dir()) {
        eprintln!(
            "stray: images unavailable; enable terminal images or tmux allow-passthrough; starting shell without a cat"
        );
    }
}

fn claim(directory: &Path) -> bool {
    // A separate marker avoids overwriting settings when two windows start together.
    if fs::create_dir_all(directory).is_err() {
        return false;
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join("image-hint-shown"))
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_windows_claim_the_hint_only_once() {
        let directory = std::env::temp_dir().join(format!(
            "stray-image-hint-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let attempts: Vec<_> = (0..8)
            .map(|_| {
                let directory = directory.clone();
                std::thread::spawn(move || claim(&directory))
            })
            .collect();
        let claimed = attempts
            .into_iter()
            .map(|thread| usize::from(thread.join().unwrap()))
            .sum::<usize>();
        assert_eq!(claimed, 1);
        assert!(!claim(&directory));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn unavailable_state_does_not_block_startup() {
        assert!(!claim(Path::new("/dev/null/stray")));
    }
}
