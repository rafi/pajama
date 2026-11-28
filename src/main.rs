//! pj opens a tmux session per project, by path, folder name, picker or history, and
//! brings back its saved layout. A project is its directory, the session name only a label.
//!
//! Every command reads all sessions in one tmux call and acts in one more.

mod history;
mod layout;
mod paths;
mod picker;
mod process;
mod text;
mod tmux;

use std::env;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

use text::{basename, plain_name, tilde, tmux_arg};
use tmux::Session;

const USAGE: &str = "usage: pj [PATH|NAME|-]
       pj save [NAME]
       pj stop [NAME]
       pj --version";

fn fail(code: i32, message: &str) -> i32 {
    eprintln!("pj: {message}");
    code
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    std::process::exit(run(&args));
}

fn run(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("save" | "stop") => save_or_stop(args),
        Some("-h" | "--help") => {
            println!("{USAGE}");
            0
        }
        Some("--version") => {
            println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
            0
        }
        // Internal commands for tmux hooks, the picker and completions. Their names start
        // with -- so no project folder can shadow them.
        Some("--hook") => hook(&args[1..]),
        Some("--preview") => {
            picker::preview(args.get(1).map_or("", String::as_str));
            0
        }
        Some("--complete") => complete(args.get(1).map_or("", String::as_str)),
        _ => open_command(args),
    }
}

fn save_or_stop(args: &[String]) -> i32 {
    if args.len() > 2 {
        return fail(2, &format!("usage: pj {} [NAME]", args[0]));
    }
    let sessions = tmux::snapshot();
    let session = match args.get(1) {
        Some(name) => match tmux::find_name(&sessions, name) {
            Some(s) => s,
            None => return fail(1, &format!("no session '{name}'")),
        },
        None => match tmux::current(&sessions) {
            Some(s) => s,
            None => return fail(2, &format!("{} needs a session name outside tmux", args[0])),
        },
    };
    if !session.is_project() {
        return fail(
            1,
            &format!("session '{}' was not opened by pj", session.name),
        );
    }
    if !layout::save(&session.id) {
        return 1;
    }
    if args[0] == "stop" {
        // Attached clients move to the most recently used session instead of detaching.
        let id = session.id.as_str();
        tmux::run(&[
            "set-option",
            "-t",
            id,
            "detach-on-destroy",
            "off",
            ";",
            "kill-session",
            "-t",
            id,
        ]);
    }
    0
}

fn is_path(arg: &str) -> bool {
    arg.contains('/') || arg == "." || arg == ".." || arg.starts_with('~')
}

fn open_command(args: &[String]) -> i32 {
    if args.len() > 1 {
        return fail(2, USAGE);
    }
    let sessions = tmux::snapshot();
    // A target is a project root, or the plain name of a session pj did not create.
    let target = match args.first().map(String::as_str) {
        None => match picker::pick("", &sessions, &[]) {
            Ok(t) => t,
            Err(code) => return code,
        },
        Some("-") => {
            let current = tmux::current(&sessions).map(|s| s.entry.as_str());
            match history::recent(&sessions)
                .into_iter()
                .find(|e| Some(e.as_str()) != current)
            {
                Some(t) => t,
                None => return fail(1, "no previous project"),
            }
        }
        Some(arg) if is_path(arg) => {
            let path = match arg.strip_prefix('~') {
                Some(rest) if rest.is_empty() || rest.starts_with('/') => {
                    format!("{}{rest}", env::var("HOME").unwrap_or_default())
                }
                _ => arg.to_owned(),
            };
            match Path::new(&path).canonicalize() {
                Ok(p) if p.is_dir() => p.to_string_lossy().into_owned(),
                _ => return fail(1, &format!("{path} is not a directory")),
            }
        }
        Some(name) => {
            let mut matches: Vec<String> = history::projects(&sessions)
                .into_iter()
                .filter(|r| basename(r) == name)
                .collect();
            if let Some(s) = tmux::find_name(&sessions, name) {
                if s.is_project() {
                    if !matches.contains(&s.entry) {
                        matches.push(s.entry.clone());
                    }
                } else if matches.is_empty() {
                    // A session pj did not create has no project, so pj only switches to it.
                    return attach(&s.id, &s.entry, &sessions, &[]);
                }
            }
            let picked = match matches.len() {
                1 => Ok(matches.remove(0)),
                0 => picker::pick(name, &sessions, &[]),
                _ => picker::pick(name, &sessions, &matches),
            };
            match picked {
                Ok(t) => t,
                Err(code) => return code,
            }
        }
    };
    go(&target, &sessions)
}

fn go(target: &str, sessions: &[Session]) -> i32 {
    if target.starts_with('/') {
        open(target, sessions)
    } else if let Some(s) = tmux::find_entry(sessions, target) {
        attach(&s.id, target, sessions, &[])
    } else {
        fail(1, &format!("no session '{target}'"))
    }
}

/// Goes to the session of project ROOT, restoring it from its saved layout if needed.
fn open(root: &str, sessions: &[Session]) -> i32 {
    if let Some(s) = tmux::find_entry(sessions, root) {
        return attach(&s.id, root, sessions, &[]);
    }
    if !Path::new(root).is_dir() {
        return fail(1, &format!("{root} no longer exists"));
    }
    let mut file = paths::layout_file(root);
    if file.is_file() {
        // A / and a % in the same place of two paths give the same file name.
        let saved = paths::saved_root(&file).unwrap_or_default();
        if saved != root {
            return fail(1, &format!("{} belongs to {saved}", file.display()));
        }
    } else {
        file = paths::config_dir().join("default.tsv");
    }
    let file = file.is_file().then_some(file);
    // Projects in different directories may share a folder name, so later sessions get a number.
    let taken: Vec<String> = sessions.iter().map(|s| plain_name(&s.name)).collect();
    let base = Some(plain_name(basename(root)))
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| "pj".into());
    let mut name = base.clone();
    let mut n = 2;
    while taken.contains(&name) {
        name = format!("{base}-{n}");
        n += 1;
    }
    let Some(restored) = layout::restore(&name, root, file.as_deref()) else {
        let shown = file.map(|f| f.display().to_string()).unwrap_or_default();
        return fail(1, &format!("no pane in {shown} could be restored"));
    };
    if !restored.complete {
        eprintln!("pj: not every pane could be restored");
    }
    let mut finish = tag(&restored.id, root);
    finish.extend(restored.finish);
    attach(&restored.id, root, sessions, &finish)
}

/// Commands that tag session ID as project ROOT and install pj's hooks.
/// The hooks are global because a per-session hook would hide other tools' hooks of the
/// same name. They skip sessions without a root, and pass the session number without
/// its `$` so the shell can't expand it.
fn tag(id: &str, root: &str) -> Vec<String> {
    let exe = env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "pj".into());
    let hook = |event: &str| {
        format!(
            "if-shell -F '#{{@pj_root}}' \
             'run-shell -b \"#{{q:@pj_bin}} --hook {event} #{{s/[$]//:session_id}}\"'"
        )
    };
    #[rustfmt::skip]
    let call = [
        "set-option", "-t", id, "@pj_root", &tmux_arg(root), ";",
        "set-option", "-t", id, "@pj_bin", &tmux_arg(&exe), ";",
        "set-hook", "-g", "client-detached[42]", &hook("detached"), ";",
        "set-hook", "-g", "client-session-changed[42]", &hook("visited"),
    ];
    call.iter().map(|s| s.to_string()).collect()
}

/// Goes to session ID, first running the tmux commands in BEFORE in the same call. The
/// session the client leaves goes into history under the one it goes to, so `pj -` can
/// return to it. Both entries come from the snapshot, so tmux sees one call.
fn attach(id: &str, entry: &str, sessions: &[Session], before: &[String]) -> i32 {
    match tmux::current(sessions) {
        Some(current) if current.id != id => history::add(&[&current.entry, entry]),
        _ => history::add(&[entry]),
    }
    let switch = || {
        Command::new("tmux")
            .args(["switch-client", "-t", id])
            .status()
            .map_or(1, |s| s.code().unwrap_or(1))
    };
    if env::var_os("TMUX").is_some() {
        if before.is_empty() {
            return switch();
        }
        // A failed command ends a tmux call, and BEFORE may end in a stale layout, so the
        // switch gets its own call when the shared one fails.
        let mut call = before.to_vec();
        call.extend([";", "switch-client", "-t", id].map(String::from));
        if tmux::run(&call).ok { 0 } else { switch() }
    } else {
        // Commands after attach-session in one call never run, so attaching comes last.
        if !before.is_empty() {
            tmux::run(before);
        }
        let err = Command::new("tmux")
            .args(["attach-session", "-t", id])
            .exec();
        fail(1, &format!("cannot run tmux: {err}"))
    }
}

/// Handles tmux hook EVENT for the session whose id is $N.
fn hook(args: &[String]) -> i32 {
    let (Some(event), Some(n)) = (args.first(), args.get(1)) else {
        return 2;
    };
    let id = format!("${n}");
    match event.as_str() {
        "detached" => i32::from(!layout::save(&id)),
        "visited" => {
            // The hook runs only in sessions with a root, so the entry is their root.
            if let Some(root) = tmux::run(&["display", "-p", "-t", &id, "#{@pj_root}"]).first_line()
            {
                history::add(&[&root]);
            }
            0
        }
        _ => 2,
    }
}

fn complete(what: &str) -> i32 {
    let sessions = tmux::snapshot();
    match what {
        "projects" => {
            for root in history::projects(&sessions) {
                if Path::new(&root).is_dir() || tmux::find_entry(&sessions, &root).is_some() {
                    println!("{}\t{}", basename(&root), tilde(&root));
                }
            }
        }
        "sessions" => {
            for s in sessions.iter().filter(|s| s.is_project()) {
                println!("{}", s.name);
            }
        }
        _ => return 2,
    }
    0
}
