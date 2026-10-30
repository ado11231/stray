//! Builds every sprite into the program, so `stray` needs no files next to it.
//!
//! Each folder in `assets/sprites` is one pose, and its PNG files are the frames, in name
//! order. This writes a table of them that `src/frames.rs` includes.

use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

/// Where the sprites live, from the top of the repo.
const SPRITES: &str = "assets/sprites";

/// The file this writes in Cargo's build folder.
const TABLE: &str = "sprites.rs";

const PNG: &str = "png";

fn main() -> Result<(), Box<dyn Error>> {
    // Cargo watches the whole folder, so a new or changed frame rebuilds the table.
    println!("cargo::rerun-if-changed={SPRITES}");
    let repo = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").ok_or("CARGO_MANIFEST_DIR is not set")?,
    );
    let out = PathBuf::from(std::env::var_os("OUT_DIR").ok_or("OUT_DIR is not set")?);

    let mut table = String::from("&[\n");
    for folder in sorted(&repo.join(SPRITES))? {
        if !folder.is_dir() {
            continue;
        }
        let name = folder
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("a sprite folder name is not plain text")?;
        writeln!(table, "    ({name:?}, &[")?;
        for frame in sorted(&folder)? {
            if frame.extension().and_then(|extension| extension.to_str()) == Some(PNG) {
                writeln!(table, "        include_bytes!({frame:?}),")?;
            }
        }
        writeln!(table, "    ]),")?;
    }
    table.push_str("]\n");
    fs::write(out.join(TABLE), table)?;
    Ok(())
}

/// The entries of a folder, sorted by name, so frames keep their order.
fn sorted(folder: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(folder)? {
        paths.push(entry?.path());
    }
    paths.sort();
    Ok(paths)
}
