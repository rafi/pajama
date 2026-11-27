//! The recent list, and what pj knows it can go to.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::paths;
use crate::tmux::Session;

fn file() -> PathBuf {
    paths::state_dir().join("recent")
}

fn read() -> Vec<String> {
    fs::read_to_string(file())
        .map(|text| text.lines().filter(|l| !l.is_empty()).map(String::from).collect())
        .unwrap_or_default()
}

/// Entries that can still be opened, newest first. A plain name counts while a session pj
/// did not create runs under it.
pub fn recent(sessions: &[Session]) -> Vec<String> {
    read()
        .into_iter()
        .filter(|entry| {
            if entry.starts_with('/') {
                Path::new(entry).is_dir()
            } else {
                sessions.iter().any(|s| !s.is_project() && s.entry == *entry)
            }
        })
        .collect()
}

/// Moves each entry to the top, the last one ending first. At most 100 are kept.
pub fn add(entries: &[&str]) {
    let mut list: Vec<String> = entries.iter().rev().map(|e| e.to_string()).collect();
    list.extend(read().into_iter().filter(|e| !entries.contains(&e.as_str())));
    list.truncate(100);
    let _ = paths::write_atomic(&file(), &(list.join("\n") + "\n"));
}

/// Known project roots once each, recent ones first, then the rest sorted.
pub fn projects(sessions: &[Session]) -> Vec<String> {
    projects_after(&recent(sessions), sessions)
}

fn projects_after(recent: &[String], sessions: &[Session]) -> Vec<String> {
    let mut others: BTreeSet<String> = sessions
        .iter()
        .filter(|s| s.is_project())
        .map(|s| s.entry.clone())
        .collect();
    if let Ok(dir) = fs::read_dir(paths::state_dir()) {
        for file in dir.flatten() {
            let path = file.path();
            if path.extension().is_some_and(|ext| ext == "tsv")
                && let Some(root) = paths::saved_root(&path)
            {
                others.insert(root);
            }
        }
    }
    let mut out: Vec<String> = recent.iter().filter(|e| e.starts_with('/')).cloned().collect();
    out.extend(others.into_iter().filter(|root| !recent.contains(root)));
    out
}

/// Everything pj can go to, recent first: project roots, then plain names of other sessions.
pub fn targets(sessions: &[Session]) -> Vec<String> {
    let recent = recent(sessions);
    let mut seen: Vec<String> = Vec::new();
    let all = recent
        .iter()
        .cloned()
        .chain(projects_after(&recent, sessions))
        .chain(sessions.iter().map(|s| s.entry.clone()));
    for target in all {
        if !target.is_empty() && !seen.contains(&target) {
            seen.push(target);
        }
    }
    seen
}
