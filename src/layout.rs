//! Saving a session's windows and panes to its layout file, and building a session from one.
//!
//! A layout file starts with `root<TAB><path>`, then one line per pane with window index,
//! window layout, active window, active pane, pane folder, and the command to rerun.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::paths;
use crate::process;
use crate::text::tmux_arg;
use crate::tmux;

/// Saves session ID to the layout file of its project. Sessions pj did not create have no
/// project, so they are never saved.
pub fn save(id: &str) -> bool {
    let format = [
        "#{@pj_root}",
        "#{window_index}",
        "#{window_layout}",
        "#{window_active}",
        "#{pane_active}",
        "#{pane_current_path}",
        "#{pane_pid}",
    ]
    .join("\x1f");
    let out = tmux::run(&["list-panes", "-s", "-t", id, "-F", &format]);
    if !out.ok {
        return false;
    }
    let panes: Vec<Vec<&str>> = out.stdout.lines().map(|l| l.split('\x1f').collect()).collect();
    let Some(root) = panes.first().map(|f| f[0]).filter(|r| !r.is_empty()) else {
        return false;
    };
    let mut text = format!("root\t{root}\n");
    for f in panes.iter().filter(|f| f.len() >= 7) {
        let command = pane_command(f[6]);
        // A field holding a tab or newline would break the line, so they become spaces.
        let fields: Vec<String> = [f[1], f[2], f[3], f[4], f[5], &command]
            .iter()
            .map(|s| s.replace(['\t', '\n'], " "))
            .collect();
        text.push_str(&fields.join("\t"));
        text.push('\n');
    }
    paths::write_atomic(&paths::layout_file(root), &text).is_ok()
}

/// The command that brings back the program a pane's shell runs. An executable
/// `resume/PROGRAM` in the config folder decides for that program, given its PID. Other
/// programs come back with their full command line when the programs file lists them.
pub fn pane_command(shell_pid: &str) -> String {
    let Some(child) = shell_pid.parse().ok().and_then(process::first_child) else {
        return String::new();
    };
    let program = child.args.first().map_or("", |a| a.rsplit('/').next().unwrap_or(a));
    let config = paths::config_dir();
    let resume = config.join("resume").join(program);
    if !program.is_empty()
        && fs::metadata(&resume).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    {
        return Command::new(&resume)
            .arg(child.pid.to_string())
            .stderr(Stdio::null())
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).trim_end().to_owned())
            .unwrap_or_default();
    }
    let listed = fs::read_to_string(config.join("programs")).is_ok_and(|text| {
        text.lines()
            .map(str::trim)
            .any(|line| !line.starts_with('#') && line == program)
    });
    if listed { child.args.join(" ") } else { String::new() }
}

fn args<const N: usize>(fixed: [&str; N]) -> Vec<String> {
    fixed.iter().map(|s| s.to_string()).collect()
}

/// Creates detached session NAME for project ROOT from layout FILE, or one shell without
/// it, and returns the session id.
///
/// Commands that follow split-window or new-window in one tmux call go to the new pane,
/// so each pane takes one call. A failed command ends its call, so commands that can fail,
/// like a stale layout string, come last.
pub fn restore(name: &str, root: &str, file: Option<&Path>) -> Option<String> {
    let lines: Vec<String> = match file {
        Some(file) => fs::read_to_string(file)
            .ok()?
            .lines()
            .filter(|l| !l.starts_with("root\t"))
            .map(String::from)
            .collect(),
        None => vec!["0\t\t1\t1\t\t".into()],
    };
    let mut session = String::new();
    let mut index: Option<String> = None; // window index of the previous line
    let mut layout = String::new(); // saved layout of the window being built
    let mut last = String::new(); // newest pane of that window
    let mut focus: Option<String> = None; // pane of that window to select once it is complete
    let mut active: Option<String> = None; // a pane in the window to select at the end
    for line in &lines {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() != 6 {
            continue;
        }
        let cwd = tmux_arg(if Path::new(f[4]).is_dir() { f[4] } else { root });
        // Typing the command keeps the pane open after the program exits. A new session is
        // not the current target, so its keys name it.
        let mut keys = Vec::new();
        if !f[5].is_empty() {
            let to = if index.is_none() { vec!["-t".to_owned(), format!("={name}:")] } else { vec![] };
            keys.extend([";".to_owned(), "send-keys".to_owned()]);
            keys.extend(to.clone());
            keys.extend(["-l".to_owned(), "--".to_owned(), tmux_arg(f[5])]);
            keys.extend([";".to_owned(), "send-keys".to_owned()]);
            keys.extend(to);
            keys.push("Enter".to_owned());
        }
        let pane;
        if index.as_deref() == Some(f[0]) {
            // A split lands right after its target, so splitting the newest pane keeps the
            // saved order. Each split halves a pane, so the window is rebalanced before it
            // runs out of room.
            let mut call = args(["split-window", "-t", &last, "-c", &cwd, "-P", "-F", "#{pane_id}", ";", "select-layout", "tiled"]);
            call.extend(keys);
            pane = match tmux::run(&call).first_line() {
                Some(p) => {
                    last = p.clone();
                    Some(p)
                }
                // With no room left, an empty pane in its own window keeps later commands in place.
                None => tmux::run(&["new-window", "-d", "-t", &format!("{session}:"), "-c", &cwd, "-P", "-F", "#{pane_id}"])
                    .first_line(),
            };
        } else {
            // The window before this one is complete, so its active pane and layout apply now.
            let mut done = Vec::new();
            if let Some(p) = &focus {
                done.extend(args([";", "select-pane", "-t", p]));
            }
            if !layout.is_empty() {
                done.extend(args([";", "select-layout", "-t", &last, &layout]));
            }
            if index.is_none() {
                let mut call = args(["new-session", "-d", "-s", name, "-c", &cwd, "-P", "-F", "#{session_id}\x1f#{pane_id}"]);
                call.extend(keys);
                let out = tmux::run(&call);
                let (id, p) = out.first_line()?.split_once('\x1f').map(|(a, b)| (a.to_owned(), b.to_owned()))?;
                if !out.ok {
                    return None;
                }
                session = id;
                pane = Some(p);
            } else {
                let mut call = args(["new-window", "-t", &format!("{session}:"), "-c", &cwd, "-P", "-F", "#{pane_id}"]);
                call.extend(keys);
                call.extend(done);
                pane = tmux::run(&call).first_line();
            }
            index = Some(f[0].to_owned());
            layout = f[1].to_owned();
            last = pane.clone().unwrap_or_default();
            focus = None;
        }
        let Some(pane) = pane else { continue };
        if f[2] == "1" {
            active = Some(pane.clone());
        }
        if f[3] == "1" && pane == last {
            focus = Some(pane);
        }
    }
    if session.is_empty() {
        return None;
    }
    let mut done = Vec::new();
    if let Some(p) = &focus {
        done.extend(args([";", "select-pane", "-t", p]));
    }
    if let Some(p) = &active {
        done.extend(args([";", "select-window", "-t", p]));
    }
    if !layout.is_empty() {
        done.extend(args([";", "select-layout", "-t", &last, &layout]));
    }
    if !done.is_empty() {
        tmux::run(&done[1..]);
    }
    Some(session)
}
