//! The fzf picker and its preview, in the layout of `wt switch`: a mark column, then
//! aligned columns under a bold header. Marks: ▸ the session this shell runs in, ● a
//! running project, ◇ a session pj did not create, whose row is dim. A stopped project
//! shows its saved window count and the age of its layout file, dimmed, like the ref rows
//! of `wt list`.

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::io::{IsTerminal, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::history;
use crate::paths;
use crate::text::{age, basename, pad_left, pad_right, sh_quote, shorten, tilde, width};
use crate::tmux::{self, Session};

const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const RESET: &str = "\x1b[0m";

struct Row {
    target: String,
    marks: String,
    label: String,
    win: String,
    window: String,
    age: String,
    stale: bool,
    place: String,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The header, then per target the target, a tab, and the row fzf shows.
pub fn rows(sessions: &[Session], targets: &[String]) -> Vec<String> {
    let now = now();
    let mut rows = Vec::new();
    for target in targets {
        let running = sessions.iter().find(|s| s.entry == *target);
        let mut row = Row {
            target: target.clone(),
            marks: String::new(),
            label: String::new(),
            win: String::new(),
            window: String::new(),
            age: String::new(),
            stale: false,
            place: "tmux session".into(),
        };
        let kind;
        if target.starts_with('/') {
            row.label = basename(target).to_owned();
            row.place = tilde(target);
            if running.is_some() {
                kind = '●';
            } else if !Path::new(target).is_dir() {
                // A deleted project keeps its layout file, but there is nothing left to open.
                continue;
            } else {
                kind = ' ';
                let file = paths::layout_file(target);
                if let Ok(text) = fs::read_to_string(&file) {
                    let windows: BTreeSet<&str> = text
                        .lines()
                        .filter(|l| !l.starts_with("root\t"))
                        .map(|l| l.split('\t').next().unwrap_or(""))
                        .collect();
                    row.win = windows.len().to_string();
                    let saved = fs::metadata(&file)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(now);
                    row.age = age(now - saved);
                    row.stale = true;
                }
            }
        } else if running.is_some() {
            kind = '◇';
        } else {
            continue;
        }
        let mut here = ' ';
        if let Some(s) = running {
            if s.current {
                here = '▸';
            }
            row.label = s.name.clone();
            row.win = s.windows.clone();
            row.window = shorten(&s.window, 20);
            row.age = age(now - s.activity);
        }
        row.marks = format!("{here}{kind}");
        rows.push(row);
    }

    // Each column is as wide as its widest cell or its title.
    let widest = |title: &str, cell: fn(&Row) -> &str| {
        rows.iter()
            .map(|r| width(cell(r)))
            .max()
            .unwrap_or(0)
            .max(width(title))
    };
    let w_name = widest("Name", |r| &r.label);
    let w_win = widest("Win", |r| &r.win);
    let w_window = widest("Window", |r| &r.window);
    let w_age = widest("Age", |r| &r.age);

    // fzf takes the header from the first line. Its empty first field is never printed.
    let mut lines = vec![format!(
        "\t   {BOLD}{}{RESET}",
        [
            pad_right("Name", w_name),
            pad_left("Win", w_win),
            pad_right("Window", w_window),
            pad_right("Age", w_age),
            "Path".into(),
        ]
        .join("  ")
    )];
    for r in &rows {
        // The dim codes wrap the text only, so the padding still lines up.
        let win_pad = " ".repeat(w_win - width(&r.win));
        let age_pad = " ".repeat(w_age - width(&r.age));
        let (win, age) = if r.stale {
            (
                format!("{win_pad}{DIM}{}{RESET}", r.win),
                format!("{DIM}{}{RESET}{age_pad}", r.age),
            )
        } else {
            (format!("{win_pad}{}", r.win), format!("{}{age_pad}", r.age))
        };
        let mut shown = [
            format!("{} {}", r.marks, pad_right(&r.label, w_name)),
            win,
            pad_right(&r.window, w_window),
            age,
            r.place.clone(),
        ]
        .join("  ");
        if r.marks.ends_with('◇') {
            shown = format!("{DIM}{shown}{RESET}");
        }
        // fzf goes to the target before the tab and shows only the row after it.
        lines.push(format!("{}\t{shown}", r.target));
    }
    lines
}

/// Lets the user pick one of TARGETS, or everything pj can go to when there are none, with
/// QUERY typed in. Errs with the exit code.
pub fn pick(query: &str, sessions: &[Session], targets: &[String]) -> Result<String, i32> {
    if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        eprintln!("pj: picking a project needs a terminal, pass a path or an exact folder name");
        return Err(2);
    }
    let all;
    let targets = if targets.is_empty() {
        all = history::targets(sessions);
        &all
    } else {
        targets
    };
    let rows = rows(sessions, targets);
    let no_match = || {
        if query.is_empty() {
            eprintln!("pj: no projects yet, open one with pj PATH");
        } else {
            eprintln!("pj: no project matches '{query}', run pj to pick one");
        }
        Err(1)
    };
    // The first line is the header, so a second one means there is something to pick.
    if rows.len() < 2 {
        return no_match();
    }
    let exe = paths::exe();
    let (cols, lines) = terminal_size();
    let mut cmd = Command::new("fzf");
    // Only a typed name may open its single match directly. Bare pj always shows the list.
    if !query.is_empty() {
        cmd.arg("--select-1");
    }
    // The look of `wt switch`: a list under the prompt with its header, the whole selected
    // row lit with fzf's light theme, input order kept until a typed query ranks the rows,
    // and the preview with its keys. The preview runs in sh, so the user's shell config
    // does not load on every cursor move.
    cmd.args(["--exit-0", "--ansi", "--query", query])
        .args([
            "--delimiter=\t",
            "--with-nth=2",
            "--header-lines=1",
            "--height=90%",
            "--layout=reverse",
            "--tiebreak=begin,end",
            "--highlight-line",
            "--no-hscroll",
            "--scrollbar=▐",
            "--ellipsis=",
            "--info=hidden",
            "--no-separator",
            "--no-multi",
            "--pointer=>",
            "--gutter= ",
            "--color=fg:-1,bg:-1,header:-1,hl:108,fg+:237,bg+:251,hl+:108",
            "--with-shell=sh -c",
        ])
        .arg(format!("--preview={} --preview {{1}}", sh_quote(&exe)))
        .arg(format!("--preview-window={}", preview_window(cols, lines)))
        .arg(
            "--bind=alt-p:toggle-preview,ctrl-u:preview-half-page-up,ctrl-d:preview-half-page-down",
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    let Ok(mut child) = cmd.spawn() else {
        eprintln!("pj: the picker needs fzf");
        return Err(1);
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all((rows.join("\n") + "\n").as_bytes());
    }
    let Ok(out) = child.wait_with_output() else {
        return Err(1);
    };
    match out.status.code() {
        Some(0) => {
            let text = String::from_utf8_lossy(&out.stdout);
            Ok(text
                .lines()
                .next()
                .unwrap_or("")
                .split('\t')
                .next()
                .unwrap_or("")
                .to_owned())
        }
        Some(130) => Err(130),
        _ => no_match(),
    }
}

/// Like `wt switch`, the preview sits beside the list on a wide landscape terminal and below
/// it otherwise. A character is about twice as tall as it is wide, so rows count double.
pub fn preview_window(cols: usize, rows: usize) -> &'static str {
    if cols >= 80 && cols >= 2 * rows {
        "right:50%,border-left"
    } else {
        "down:50%,border-top"
    }
}

#[repr(C)]
struct Winsize {
    rows: u16,
    cols: u16,
    x: u16,
    y: u16,
}

unsafe extern "C" {
    fn ioctl(fd: i32, request: std::ffi::c_ulong, ...) -> i32;
}

#[cfg(target_os = "macos")]
const TIOCGWINSZ: std::ffi::c_ulong = 0x4008_7468;
#[cfg(not(target_os = "macos"))]
const TIOCGWINSZ: std::ffi::c_ulong = 0x5413;

/// Columns and rows of the terminal on stderr, which fzf draws on.
fn terminal_size() -> (usize, usize) {
    let mut size = Winsize {
        rows: 0,
        cols: 0,
        x: 0,
        y: 0,
    };
    // SAFETY: TIOCGWINSZ writes one Winsize into the pointer, which lives for the call.
    let ok = unsafe { ioctl(2, TIOCGWINSZ, &mut size as *mut Winsize) } == 0;
    if ok && size.cols > 0 && size.rows > 0 {
        (size.cols as usize, size.rows as usize)
    } else {
        (80, 24)
    }
}

/// The preview of TARGET: with worktrunk installed, the project's worktrees as `wt list`
/// draws them, sized to the pane, then the last 15 commits.
pub fn preview(target: &str) {
    let dir = if target.starts_with('/') {
        target.to_owned()
    } else {
        // A session pj did not create has no project, so its active pane's folder stands in.
        // Sessions pj created print an empty line, and the folder comes last since it may
        // hold a tab.
        let format = format!(
            "#{{?@pj_root,,#{{session_name}}{}#{{pane_current_path}}}}",
            tmux::SEP
        );
        let out = tmux::run(&["list-sessions", "-F", &format]);
        let found = out.stdout.lines().find_map(|line| {
            let (name, dir) = line.split_once(tmux::SEP)?;
            (crate::text::plain_name(name) == target).then(|| dir.to_owned())
        });
        match found {
            Some(dir) => dir,
            None => return,
        }
    };
    let is_repo = Command::new("git")
        .args(["-C", &dir, "rev-parse", "--git-dir"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !is_repo {
        println!("{DIM}Not a git repository: {}{RESET}", tilde(&dir));
        return;
    }
    let width = env::var("FZF_PREVIEW_COLUMNS")
        .ok()
        .filter(|w| !w.is_empty())
        .unwrap_or_else(|| "80".into());
    // fzf reads the pane through a pipe, where wt would drop its colors. Its summary line
    // goes to stderr.
    let wt = Command::new("wt")
        .args(["-C", &dir, "list"])
        .env("CLICOLOR_FORCE", "1")
        .env("COLUMNS", &width)
        .stderr(Stdio::null())
        .status();
    if wt.is_ok_and(|s| s.success()) {
        println!();
    }
    let _ = std::io::stdout().flush();
    let log = Command::new("git")
        .args(["-C", &dir, "log", "-15", "--graph", "--color=always"])
        .arg("--format=%C(auto)%h%d%C(reset) %s %C(dim)%cr · %an%C(reset)")
        .stderr(Stdio::null())
        .status();
    if !log.is_ok_and(|s| s.success()) {
        println!("{DIM}No commits yet{RESET}");
    }
}

#[cfg(test)]
mod tests {
    use super::preview_window;

    #[test]
    fn preview_sits_right_on_a_wide_terminal() {
        assert_eq!(preview_window(200, 50), "right:50%,border-left");
    }

    #[test]
    fn preview_drops_below_on_a_narrow_or_portrait_terminal() {
        assert_eq!(preview_window(70, 40), "down:50%,border-top");
        assert_eq!(preview_window(180, 136), "down:50%,border-top");
    }
}
