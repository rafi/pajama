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
    // The root comes on its own line and each pane's folder last, so a tab in either stays
    // inside its field.
    let format = [
        "#{window_index}",
        "#{window_layout}",
        "#{window_active}",
        "#{pane_active}",
        "#{pane_pid}",
        "#{pane_current_path}",
    ]
    .join(tmux::SEP);
    #[rustfmt::skip]
    let out = tmux::run(&[
        "display", "-p", "-t", id, "#{@pj_root}", ";",
        "list-panes", "-s", "-t", id, "-F", &format,
    ]);
    if !out.ok {
        return false;
    }
    let mut lines = out.stdout.lines();
    let Some(root) = lines.next().filter(|r| !r.is_empty()) else {
        return false;
    };
    let mut text = format!("root\t{root}\n");
    let config = paths::config_dir();
    let resume = config.join("resume");
    let programs = fs::read_to_string(config.join("programs")).unwrap_or_default();
    for f in lines.map(|l| l.splitn(6, tmux::SEP).collect::<Vec<_>>()) {
        if f.len() < 6 {
            continue;
        }
        let command = pane_command(f[4], &resume, &programs);
        // A field holding a tab or newline would break the line, so they become spaces.
        let fields: Vec<String> = [f[0], f[1], f[2], f[3], f[5], &command]
            .iter()
            .map(|s| s.replace(['\t', '\n'], " "))
            .collect();
        text.push_str(&fields.join("\t"));
        text.push('\n');
    }
    // Every detach saves, and the layout rarely changes between two.
    let file = paths::layout_file(root);
    if fs::read_to_string(&file).is_ok_and(|old| old == text) {
        return true;
    }
    paths::write_atomic(&file, &text).is_ok()
}

/// The command that brings back the program a pane's shell runs. An executable
/// `PROGRAM` in the RESUME folder decides for that program, given its PID. Other programs
/// come back with their full command line when PROGRAMS, the text of the programs file,
/// lists them.
fn pane_command(shell_pid: &str, resume: &Path, programs: &str) -> String {
    let Some(child) = shell_pid.parse().ok().and_then(process::first_child) else {
        return String::new();
    };
    let program = child
        .args
        .first()
        .map_or("", |a| a.rsplit('/').next().unwrap_or(a));
    let resume = resume.join(program);
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
    let listed = programs
        .lines()
        .map(str::trim)
        .any(|line| !line.starts_with('#') && line == program);
    if listed {
        child.args.join(" ")
    } else {
        String::new()
    }
}

fn args<const N: usize>(fixed: [&str; N]) -> Vec<String> {
    fixed.iter().map(|s| s.to_string()).collect()
}

/// A session [`restore`] built, and the commands that finish it.
pub struct Restored {
    pub id: String,
    /// Selects the saved active panes and window, then applies the saved layouts. Each command
    /// starts with the `;` that chains it, so callers append them to their next tmux call.
    /// A stale layout fails, which ends a tmux call, so layouts come last.
    pub finish: Vec<String>,
    /// False when tmux stopped partway, for example at a split with no room left.
    pub complete: bool,
}

/// Creates detached session NAME for project ROOT from layout FILE, or one shell without it.
///
/// The whole session takes one tmux call. Every command in it names the new session, since
/// inside tmux the current session is the one pj runs from. Splits and new windows are not
/// detached, so the session's active pane is always the newest one. Each create prints the
/// id of its pane.
pub fn restore(name: &str, root: &str, file: Option<&Path>) -> Option<Restored> {
    let lines: Vec<String> = match file {
        Some(file) => fs::read_to_string(file)
            .ok()?
            .lines()
            .filter(|l| !l.starts_with("root\t"))
            .map(String::from)
            .collect(),
        None => vec!["0\t\t1\t1\t\t".into()],
    };
    let panes: Vec<Vec<&str>> = lines
        .iter()
        .map(|l| l.split('\t').collect::<Vec<_>>())
        .filter(|f| f.len() == 6)
        .collect();
    let target = format!("={name}:");
    let mut call = Vec::new();
    for (i, f) in panes.iter().enumerate() {
        let cwd = tmux_arg(if Path::new(f[4]).is_dir() { f[4] } else { root });
        let pane = "#{pane_id}";
        if i == 0 {
            let format = format!("#{{session_id}}{}#{{pane_id}}", tmux::SEP);
            call.extend(args([
                "new-session",
                "-d",
                "-s",
                name,
                "-c",
                &cwd,
                "-P",
                "-F",
                &format,
            ]));
        } else if panes[i - 1][0] == f[0] {
            // A split lands right after the newest pane, which keeps the saved order. Each
            // split halves a pane, so the window is rebalanced before it runs out of room.
            call.extend(args([
                ";",
                "split-window",
                "-t",
                &target,
                "-c",
                &cwd,
                "-P",
                "-F",
                pane,
            ]));
            call.extend(args([";", "select-layout", "-t", &target, "tiled"]));
        } else {
            call.extend(args([
                ";",
                "new-window",
                "-t",
                &target,
                "-c",
                &cwd,
                "-P",
                "-F",
                pane,
            ]));
        }
        // Typing the command keeps the pane open after the program exits.
        if !f[5].is_empty() {
            call.extend(args([
                ";",
                "send-keys",
                "-t",
                &target,
                "-l",
                "--",
                &tmux_arg(f[5]),
            ]));
            call.extend(args([";", "send-keys", "-t", &target, "Enter"]));
        }
    }
    if call.is_empty() {
        return None;
    }
    let out = tmux::run(&call);
    let mut created = out.stdout.lines();
    let (id, first) = created.next()?.split_once(tmux::SEP)?;
    let ids: Vec<&str> = std::iter::once(first).chain(created).collect();

    // Only the panes tmux created before any failure get finished.
    let mut focus = Vec::new();
    let mut active = None;
    let mut layouts = Vec::new();
    for (i, (f, &pane)) in panes.iter().zip(&ids).enumerate() {
        if f[2] == "1" {
            active = Some(pane);
        }
        if f[3] == "1" {
            focus.push(pane);
        }
        let window_ends = i + 1 == ids.len() || panes[i + 1][0] != f[0];
        if window_ends && !f[1].is_empty() {
            layouts.push((pane, f[1]));
        }
    }
    let mut finish = Vec::new();
    for pane in focus {
        finish.extend(args([";", "select-pane", "-t", pane]));
    }
    if let Some(pane) = active {
        finish.extend(args([";", "select-window", "-t", pane]));
    }
    for (pane, layout) in layouts {
        finish.extend(args([";", "select-layout", "-t", pane, layout]));
    }
    Some(Restored {
        id: id.to_owned(),
        finish,
        complete: out.ok,
    })
}
