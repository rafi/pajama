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
pub fn run<S: AsRef<OsStr>>(args: &[S]) -> Out {
    match Command::new("tmux")
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

const SEP: char = '\x1f';

/// Every session in one tmux call, so a command makes one more call at most, to act.
pub fn snapshot() -> Vec<Session> {
    let format = [
        "#{session_id}",
        "#{@pj_root}",
        "#{session_name}",
        "#{session_windows}",
        "#{window_name}",
        "#{session_activity}",
    ]
    .join("\x1f");
    let mut current = None;
    let mut text = None;
    // TMUX_PANE names the pane this shell runs in. Without it tmux would guess the most
    // recent session. Asking in the same call saves one.
    if env::var_os("TMUX").is_some()
        && let Ok(pane) = env::var("TMUX_PANE")
    {
        let out = run(&[
            "display",
            "-p",
            "-t",
            &pane,
            "#{session_id}",
            ";",
            "list-sessions",
            "-F",
            &format,
        ]);
        if out.ok
            && let Some((first, rest)) = out.stdout.split_once('\n')
        {
            current = Some(first.to_owned());
            text = Some(rest.to_owned());
        }
    }
    let text = text.unwrap_or_else(|| run(&["list-sessions", "-F", &format]).stdout);
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split(SEP).collect();
            if f.len() < 6 {
                return None;
            }
            Some(Session {
                id: f[0].to_owned(),
                entry: if f[1].is_empty() {
                    plain_name(f[2])
                } else {
                    f[1].to_owned()
                },
                name: f[2].to_owned(),
                windows: f[3].to_owned(),
                window: f[4].to_owned(),
                activity: f[5].parse().unwrap_or(0),
                current: current.as_deref() == Some(f[0]),
            })
        })
        .collect()
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
