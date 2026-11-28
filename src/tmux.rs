//! tmux calls and the session snapshot every command starts from.

use std::env;
use std::ffi::OsStr;
use std::process::{Command, Stdio};

use crate::text::plain_name;

pub struct Out {
    pub ok: bool,
    pub stdout: String,
}

impl Out {
    pub fn first_line(&self) -> Option<String> {
        self.stdout
            .lines()
            .next()
            .filter(|l| !l.is_empty())
            .map(String::from)
    }
}

/// Runs one tmux call. Arguments that are a lone ";" chain commands into the same call.
///
/// Without -u, tmux 3.4 prints tabs and non-ASCII names as `_` unless the locale is UTF-8.
pub fn run<S: AsRef<OsStr>>(args: &[S]) -> Out {
    match Command::new("tmux")
        .arg("-u")
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    {
        Ok(out) => Out {
            ok: out.status.success(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        },
        Err(_) => Out {
            ok: false,
            stdout: String::new(),
        },
    }
}

/// A running session. The entry is the project root, or for a session pj did not create,
/// its plain name.
pub struct Session {
    pub id: String,
    pub entry: String,
    pub name: String,
    pub windows: String,
    pub window: String,
    pub activity: i64,
    pub current: bool,
}

impl Session {
    pub fn is_project(&self) -> bool {
        self.entry.starts_with('/')
    }
}

/// Separates the fields of every tmux format pj reads. tmux 3.4 and 3.5 print other control
/// characters as octal escapes, and layout files cannot hold a tab either. A folder may
/// still hold a tab, so a path is always the last field and is split off with splitn.
pub const SEP: &str = "\t";

/// Every session, and the binary pj's global hooks run.
pub struct Snapshot {
    pub sessions: Vec<Session>,
    /// The global `@pj_bin`, empty when it is unset.
    pub bin: String,
}

/// Reads every session in one tmux call, so a command makes one more call at most, to act.
pub fn snapshot() -> Snapshot {
    let format = [
        "#{session_id}",
        "#{session_name}",
        "#{session_windows}",
        "#{window_name}",
        "#{session_activity}",
        "#{@pj_root}",
    ]
    .join(SEP);
    let mut current = None;
    let mut text = None;
    // The global @pj_bin comes last and prints nothing when unset, so it is the one line
    // that does not start with a session id.
    let tail = [
        "list-sessions",
        "-F",
        format.as_str(),
        ";",
        "show-options",
        "-gqv",
        "@pj_bin",
    ];
    // TMUX_PANE names the pane this shell runs in. Without it tmux would guess the most
    // recent session. Asking in the same call saves one.
    if env::var_os("TMUX").is_some()
        && let Ok(pane) = env::var("TMUX_PANE")
    {
        let mut call = vec!["display", "-p", "-t", pane.as_str(), "#{session_id}", ";"];
        call.extend(tail);
        let out = run(&call);
        if out.ok
            && let Some((first, rest)) = out.stdout.split_once('\n')
        {
            current = Some(first.to_owned());
            text = Some(rest.to_owned());
        }
    }
    let text = text.unwrap_or_else(|| run(&tail).stdout);
    let mut bin = String::new();
    let mut sessions = Vec::new();
    for line in text.lines() {
        if !line.starts_with('$') {
            bin = line.to_owned();
            continue;
        }
        let f: Vec<&str> = line.splitn(6, SEP).collect();
        if f.len() < 6 {
            continue;
        }
        sessions.push(Session {
            id: f[0].to_owned(),
            entry: if f[5].is_empty() {
                plain_name(f[1])
            } else {
                f[5].to_owned()
            },
            name: f[1].to_owned(),
            windows: f[2].to_owned(),
            window: f[3].to_owned(),
            activity: f[4].parse().unwrap_or(0),
            current: current.as_deref() == Some(f[0]),
        });
    }
    Snapshot { sessions, bin }
}

pub fn find_entry<'a>(sessions: &'a [Session], entry: &str) -> Option<&'a Session> {
    if entry.is_empty() {
        return None;
    }
    sessions.iter().find(|s| s.entry == entry)
}

/// Names are compared without the status marks agent tools append to them.
pub fn find_name<'a>(sessions: &'a [Session], name: &str) -> Option<&'a Session> {
    let name = plain_name(name);
    if name.is_empty() {
        return None;
    }
    sessions.iter().find(|s| plain_name(&s.name) == name)
}

pub fn current(sessions: &[Session]) -> Option<&Session> {
    sessions.iter().find(|s| s.current)
}
