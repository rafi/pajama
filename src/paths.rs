//! Where pj keeps its files.

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process;

fn base(var: &str, default: &str) -> PathBuf {
    match env::var_os(var) {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(default),
    }
}

pub fn state_dir() -> PathBuf {
    base("XDG_STATE_HOME", ".local/state").join("pj")
}

pub fn config_dir() -> PathBuf {
    base("XDG_CONFIG_HOME", ".config").join("pj")
}

/// Like vim's undo files, the name is the full path with each / replaced by %.
pub fn layout_file(root: &str) -> PathBuf {
    state_dir().join(format!("{}.tsv", root.replace('/', "%")))
}

/// The root a layout file was saved for, from its `root` line.
pub fn saved_root(file: &Path) -> Option<String> {
    fs::read_to_string(file)
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("root\t"))
        .map(String::from)
}

/// Writes through a temp file named after this process, so a reader never sees half a
/// file and two hooks writing at once never share a temp file.
pub fn write_atomic(file: &Path, contents: &str) -> io::Result<()> {
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut tmp = file.as_os_str().to_owned();
    tmp.push(format!(".{}", process::id()));
    fs::write(&tmp, contents)?;
    fs::rename(&tmp, file)
}
