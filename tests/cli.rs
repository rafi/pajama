//! End-to-end tests. Each runs the pj binary against its own private tmux server and its own
//! state and config folders, so real sessions and layouts stay untouched.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::sleep;
use std::time::Duration;

const PJ: &str = env!("CARGO_BIN_EXE_pj");
static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fx {
    dir: PathBuf,
    path: String,
    socket: String,
}

fn script(file: &Path, body: &str) {
    fs::write(file, body).unwrap();
    fs::set_permissions(file, fs::Permissions::from_mode(0o755)).unwrap();
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn uid() -> String {
    let out = Command::new("id").arg("-u").output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

impl Fx {
    fn new() -> Fx {
        // tmux sockets live under this folder and have a length limit, so it stays short.
        let dir = PathBuf::from(format!(
            "/tmp/pj-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&dir);
        for sub in ["home", "state/pj", "config/pj", "tmux", "bin", "code"] {
            fs::create_dir_all(dir.join(sub)).unwrap();
        }
        // pj resolves folders, and /tmp is a link on macOS.
        let dir = dir.canonicalize().unwrap();
        fs::write(
            dir.join("config/pj/default.tsv"),
            "0\t\t1\t1\t\t\n1\t\t0\t1\t\t\n2\t\t0\t1\t\t\n",
        )
        .unwrap();
        fs::write(dir.join("config/pj/programs"), "# comment\n\nsleep\n").unwrap();
        // A fake fzf keeps its input and arguments, then prints the line whose first field
        // is $FAKE_FZF_PICK, or exits with $FAKE_FZF_STATUS.
        let d = dir.display();
        script(
            &dir.join("bin/fzf"),
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" >{d}/fzf.args\ncat >{d}/fzf.in\n\
                 test -n \"$FAKE_FZF_STATUS\" && exit \"$FAKE_FZF_STATUS\"\n\
                 awk -F '\\t' -v pick=\"$FAKE_FZF_PICK\" '$1 == pick' {d}/fzf.in\n"
            ),
        );
        let path = format!(
            "{}:{}",
            dir.join("bin").display(),
            std::env::var("PATH").unwrap()
        );
        let socket = format!("{}/tmux/tmux-{}/default", dir.display(), uid());
        Fx { dir, path, socket }
    }

    fn p(&self, rel: &str) -> PathBuf {
        self.dir.join(rel)
    }

    fn s(&self, rel: &str) -> String {
        self.p(rel).display().to_string()
    }

    fn cmd(&self, program: &str) -> Command {
        let mut c = Command::new(program);
        c.env("HOME", self.p("home"))
            .env("XDG_STATE_HOME", self.p("state"))
            .env("XDG_CONFIG_HOME", self.p("config"))
            .env("TMUX_TMPDIR", self.p("tmux"))
            .env("SHELL", "/bin/sh")
            .env("PATH", &self.path)
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env_remove("NO_COLOR")
            .current_dir(&self.dir)
            .stdin(Stdio::null());
        c
    }

    fn tmux(&self, args: &[&str]) -> String {
        let out = self.cmd("tmux").args(args).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim_end().to_owned()
    }

    fn pj(&self, args: &[&str]) -> Output {
        self.cmd(PJ).args(args).output().unwrap()
    }

    /// Makes C act as a shell in the first pane of SESSION.
    fn inside(&self, c: &mut Command, session: &str) {
        let target = format!("={session}:");
        let env = self.tmux(&[
            "display",
            "-p",
            "-t",
            &target,
            "#{socket_path},#{pid},#{session_id}",
        ]);
        c.env("TMUX", env.replace('$', "")).env(
            "TMUX_PANE",
            self.tmux(&["display", "-p", "-t", &target, "#{pane_id}"]),
        );
    }

    fn pj_in(&self, session: &str, args: &[&str]) -> Output {
        let mut c = self.cmd(PJ);
        self.inside(&mut c, session);
        c.args(args).output().unwrap()
    }

    /// Runs pj under script, which gives it a terminal on stdin and stderr. TMUX names the
    /// test server, so pj switches a client that is not there instead of attaching.
    fn pj_tty(&self, inside: Option<&str>, env: &[(&str, &str)], args: &[&str]) -> Output {
        let mut c = self.cmd("script");
        if cfg!(target_os = "macos") {
            c.args(["-q", "/dev/null", PJ]).args(args);
        } else {
            // util-linux script runs one shell command line, not an argument list.
            let line: Vec<String> = std::iter::once(PJ)
                .chain(args.iter().copied())
                .map(|a| format!("'{}'", a.replace('\'', r"'\''")))
                .collect();
            c.args(["-qec", &line.join(" "), "/dev/null"]);
        }
        match inside {
            Some(session) => self.inside(&mut c, session),
            None => {
                // tmux creates its socket folder only when it finds the socket through
                // TMUX_TMPDIR, not through TMUX.
                let dir = Path::new(&self.socket).parent().unwrap();
                fs::create_dir_all(dir).unwrap();
                fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).unwrap();
                c.env("TMUX", format!("{},0,999", self.socket));
            }
        }
        for (k, v) in env {
            c.env(k, v);
        }
        c.output().unwrap()
    }

    fn project(&self, rel: &str) -> String {
        fs::create_dir_all(self.p(rel)).unwrap();
        self.s(rel)
    }

    fn layout(&self, root: &str) -> PathBuf {
        self.p("state/pj")
            .join(format!("{}.tsv", root.replace('/', "%")))
    }

    fn sessions(&self) -> Vec<String> {
        self.tmux(&["list-sessions", "-F", "#S"])
            .lines()
            .map(String::from)
            .collect()
    }

    fn root_of(&self, session: &str) -> String {
        self.tmux(&[
            "show-options",
            "-qv",
            "-t",
            &format!("={session}:"),
            "@pj_root",
        ])
    }

    fn recent(&self) -> Vec<String> {
        fs::read_to_string(self.p("state/pj/recent"))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }

    fn fzf_in(&self) -> Vec<String> {
        fs::read_to_string(self.p("fzf.in"))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }

    fn fzf_args(&self) -> Vec<String> {
        fs::read_to_string(self.p("fzf.args"))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }

    fn row(&self, target: &str) -> String {
        self.fzf_in()
            .into_iter()
            .find(|l| l.starts_with(&format!("{target}\t")))
            .unwrap_or_default()
    }

    /// Waits until the pane's shell runs a program other than a copy of itself.
    fn wait_child(&self, pane: &str) -> bool {
        let pid = self.tmux(&["display", "-p", "-t", pane, "#{pane_pid}"]);
        for _ in 0..30 {
            let out = Command::new("pgrep").args(["-P", &pid]).output().unwrap();
            if let Some(child) = String::from_utf8_lossy(&out.stdout).lines().next() {
                let args = Command::new("ps")
                    .args(["-o", "args=", "-p", child])
                    .output()
                    .unwrap();
                let args = String::from_utf8_lossy(&args.stdout).trim().to_owned();
                if !args.is_empty()
                    && !args.starts_with("sh")
                    && !args.starts_with("-sh")
                    && !args.starts_with("/bin/sh")
                {
                    return true;
                }
            }
            sleep(Duration::from_millis(100));
        }
        false
    }

    fn run_in(&self, pane: &str, cmd: &str) {
        self.tmux(&["send-keys", "-t", pane, "-l", "--", cmd]);
        self.tmux(&["send-keys", "-t", pane, "Enter"]);
        assert!(self.wait_child(pane), "nothing started in {pane}: {cmd}");
    }

    /// A PATH where tmux, ps and pgrep record each call in FILE before running.
    fn recording_path(&self, file: &str) -> String {
        let shim = self.p("shim");
        fs::create_dir_all(&shim).unwrap();
        for tool in ["tmux", "ps", "pgrep"] {
            let real = Command::new("sh")
                .args(["-c", &format!("command -v {tool}")])
                .output()
                .unwrap();
            let real = String::from_utf8_lossy(&real.stdout).trim().to_owned();
            script(
                &shim.join(tool),
                &format!(
                    "#!/bin/sh\necho {tool} >>{}\nexec {real} \"$@\"\n",
                    self.s(file)
                ),
            );
        }
        format!("{}:{}", shim.display(), self.path)
    }

    /// How many times each of TOOLS ran since FILE was last read, in order.
    fn calls(&self, file: &str, tools: &[&str]) -> Vec<usize> {
        let calls = fs::read_to_string(self.p(file)).unwrap_or_default();
        let _ = fs::remove_file(self.p(file));
        tools
            .iter()
            .map(|tool| calls.lines().filter(|l| l == tool).count())
            .collect()
    }

    /// A control-mode client stands in for a terminal attached to SESSION. It leaves after
    /// a few seconds, or when the server goes.
    fn attach_client(&self, session: &str) -> Child {
        let client = self
            .cmd("sh")
            .args([
                "-c",
                &format!("sleep 5 | tmux -C attach-session -t ={session}:"),
            ])
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        for _ in 0..30 {
            if !self
                .tmux(&["list-clients", "-t", &format!("={session}:")])
                .is_empty()
            {
                break;
            }
            sleep(Duration::from_millis(100));
        }
        client
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .args(["-S", &self.socket, "kill-server"])
            .stderr(Stdio::null())
            .status();
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn layout_lines(file: &Path) -> Vec<Vec<String>> {
    fs::read_to_string(file)
        .unwrap()
        .lines()
        .skip(1)
        .map(|l| l.split('\t').map(String::from).collect())
        .collect()
}

#[test]
fn a_path_opens_a_tagged_session_from_the_default_layout() {
    let fx = Fx::new();
    let root = fx.project("code/app");
    fx.pj(&[&root]);
    assert_eq!(fx.tmux(&["list-windows", "-t", "=app:"]).lines().count(), 3);
    assert_eq!(fx.root_of("app"), root);
    assert_eq!(fx.recent().first(), Some(&root));
    let own = fx.tmux(&["show-hooks", "-t", "=app:"]);
    assert!(
        !own.contains("client-session-changed") && !own.contains("client-detached"),
        "{own}"
    );
    let global = fx.tmux(&["show-hooks", "-g"]);
    let hook = |name: &str| {
        global
            .lines()
            .find(|l| l.starts_with(&format!("{name}[42] ")))
            .unwrap_or_default()
            .to_owned()
    };
    assert!(
        hook("client-detached").contains("--hook detached"),
        "{global}"
    );
    assert!(
        hook("client-session-changed").contains("--hook visited"),
        "{global}"
    );
}

#[test]
fn the_hooks_skip_sessions_pj_did_not_create() {
    let fx = Fx::new();
    let root = fx.project("code/app");
    fx.pj(&[&root]);
    fx.tmux(&["new-session", "-d", "-s", "plain", "-c", &fx.s("")]);
    // Without the guard, the hook would run this through the session's own @pj_bin.
    let spy = fx.p("bin/spy");
    script(
        &spy,
        &format!("#!/bin/sh\necho \"$@\" >>{}\n", fx.s("spied")),
    );
    fx.tmux(&[
        "set-option",
        "-t",
        "=plain:",
        "@pj_bin",
        &spy.display().to_string(),
    ]);
    let mut client = fx.attach_client("app");
    let name = fx.tmux(&["list-clients", "-t", "=app:", "-F", "#{client_name}"]);
    fx.tmux(&["switch-client", "-c", &name, "-t", "=plain:"]);
    fx.tmux(&["detach-client", "-s", "=plain:"]);
    let _ = client.wait();
    sleep(Duration::from_millis(500));
    assert!(!fx.p("spied").exists(), "pj ran for a plain session");
    let messages = fx.tmux(&["show-messages"]);
    assert!(!messages.contains("returned"), "{messages}");
}

#[test]
fn folders_with_the_same_name_get_numbered_sessions() {
    let fx = Fx::new();
    let a = fx.project("code/a/app");
    let b = fx.project("code/b/app");
    fx.pj(&[&a]);
    fx.pj(&[&b]);
    assert_eq!(fx.root_of("app-2"), b);
}

#[test]
fn a_renamed_session_is_found_by_its_folder() {
    let fx = Fx::new();
    let root = fx.project("code/app");
    fx.pj(&[&root]);
    fx.tmux(&["rename-session", "-t", "=app:", "whatever"]);
    fx.pj(&[&root]);
    assert_eq!(fx.sessions(), ["whatever"]);
}

#[test]
fn version_prints_the_package_version() {
    let fx = Fx::new();
    let out = fx.pj(&["--version"]);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("{} {}\n", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
    );
    assert!(fx.sessions().is_empty(), "no tmux call");
}

#[test]
fn bad_arguments_are_refused() {
    let fx = Fx::new();
    assert_eq!(fx.pj(&[&fx.s("nope/")]).status.code(), Some(1));
    assert_eq!(fx.pj(&["a", "b"]).status.code(), Some(2));
    let picker = fx.pj(&[]);
    assert_eq!(picker.status.code(), Some(2), "{}", text(&picker));
    assert!(!fx.p("fzf.args").exists());
    assert_eq!(fx.pj(&["save"]).status.code(), Some(2));
}

#[test]
fn stop_saves_and_closes_and_refuses_other_sessions() {
    let fx = Fx::new();
    let root = fx.project("code/app");
    fx.pj(&[&root]);
    assert!(fx.pj(&["stop", "app"]).status.success());
    assert_eq!(
        fs::read_to_string(fx.layout(&root)).unwrap().lines().next(),
        Some(format!("root\t{root}").as_str())
    );
    assert!(fx.sessions().is_empty());

    fx.tmux(&["new-session", "-d", "-s", "live", "-c", &fx.s("")]);
    assert_eq!(fx.pj(&["stop", "live"]).status.code(), Some(1));
    assert_eq!(fx.sessions(), ["live"]);
}

#[test]
fn saving_records_programs_without_ps() {
    let fx = Fx::new();
    let root = fx.project("code/app");
    fs::create_dir_all(fx.p("config/pj/resume")).unwrap();
    script(
        &fx.p("config/pj/resume/tail"),
        "#!/bin/sh\necho \"tail again $1\" | cut -d' ' -f1,2\n",
    );
    fx.pj(&[&root]);
    fx.run_in("=app:1", "sleep 300");
    fx.run_in("=app:2", "tail -f /dev/null");
    // Tabs reach a layout only through a folder, since tmux rejects them in window names.
    let tab = fx.project("code/app/t\tab");
    fx.tmux(&["split-window", "-d", "-t", "=app:0", "-c", &tab]);

    let path = fx.recording_path("calls");
    let out = fx
        .cmd(PJ)
        .env("PATH", path)
        .args(["save", "app"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(fx.calls("calls", &["ps", "pgrep"]), [0, 0]);

    let lines = layout_lines(&fx.layout(&root));
    assert!(lines.iter().all(|f| f.len() == 6), "{lines:?}");
    let command = |w: &str| {
        lines
            .iter()
            .filter(|f| f[0] == w)
            .map(|f| f[5].clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(command("0"), ["", ""]);
    assert_eq!(command("1"), ["sleep 300"]);
    assert_eq!(command("2"), ["tail again"]);
    assert!(lines.iter().any(|f| f[4] == tab.replace('\t', " ")));
}

#[test]
fn a_saved_layout_comes_back() {
    let fx = Fx::new();
    let root = fx.project("code/app");
    for d in ["a", "b", "c"] {
        fx.project(&format!("code/app/{d}"));
    }
    fs::write(
        fx.layout(&root),
        format!(
            "root\t{root}\n0\t\t0\t0\t{root}/a\t\n0\t\t0\t1\t{root}/b\techo semi;\n0\t\t0\t0\t{root}/c\t\n1\t\t1\t1\t\tsleep 300\n"
        ),
    )
    .unwrap();
    fx.pj(&[&root]);
    let panes = fx.tmux(&[
        "list-panes",
        "-t",
        "=app:0",
        "-F",
        "#{pane_index} #{pane_current_path}",
    ]);
    assert_eq!(panes, format!("0 {root}/a\n1 {root}/b\n2 {root}/c"));
    // Splits make the newest pane active, so the saved active pane is selected again.
    assert_eq!(
        fx.tmux(&["display", "-p", "-t", "=app:0", "#{pane_index}"]),
        "1"
    );
    assert_eq!(
        fx.tmux(&["display", "-p", "-t", "=app:", "#{window_index}"]),
        "1"
    );
    assert!(fx.wait_child("=app:1"));
    sleep(Duration::from_millis(300));
    assert!(
        fx.tmux(&["capture-pane", "-p", "-t", "=app:0.1"])
            .contains("echo semi;")
    );
}

#[test]
fn a_window_layout_survives_save_and_restore() {
    let fx = Fx::new();
    let root = fx.project("code/app");
    fs::write(fx.p("config/pj/default.tsv"), "0\t\t1\t1\t\t\n").unwrap();
    fx.pj(&[&root]);
    fx.tmux(&["resize-window", "-t", "=app:0", "-x", "120", "-y", "40"]);
    fx.tmux(&[
        "split-window",
        "-d",
        "-h",
        "-l",
        "30",
        "-t",
        "=app:0",
        "-c",
        &root,
    ]);
    let shape = |s: String| {
        // Pane ids change on every restore, so they are dropped before comparing.
        let mut out = String::new();
        let mut rest = s.as_str();
        while let Some(i) = rest.find("\"I\":\"%") {
            out.push_str(&rest[..i]);
            rest = rest[i + 6..].trim_start_matches(|c: char| c.is_ascii_digit());
            rest = rest.strip_prefix('"').unwrap_or(rest);
        }
        out + rest
    };
    let before = shape(fx.tmux(&["display", "-p", "-t", "=app:0", "#{window_layout}"]));
    fx.pj(&["stop", "app"]);
    fx.pj(&[&root]);
    assert_eq!(
        shape(fx.tmux(&["display", "-p", "-t", "=app:0", "#{window_layout}"])),
        before
    );
}

#[test]
fn opening_from_inside_a_session_builds_only_the_new_one() {
    let fx = Fx::new();
    let here = fx.project("code/here");
    let there = fx.project("code/there");
    fx.pj(&[&here]);
    fs::write(
        fx.layout(&there),
        format!("root\t{there}\n0\t\t1\t1\t\t\n0\t\t1\t0\t\t\n1\t\t0\t1\t\t\n"),
    )
    .unwrap();
    let panes = |session: &str| {
        fx.tmux(&["list-panes", "-s", "-t", &format!("={session}:")])
            .lines()
            .count()
    };
    let before = panes("here");
    // A client in the session pj runs from makes it the current one for tmux.
    let mut client = fx.attach_client("here");
    fx.pj_in("here", &[&there]);
    let _ = client.kill();
    let _ = client.wait();
    assert_eq!(
        panes("here"),
        before,
        "the session pj ran from is untouched"
    );
    assert_eq!(panes("there"), 3);
}

#[test]
fn splits_that_do_not_fit_leave_a_partial_session() {
    let fx = Fx::new();
    let root = fx.project("code/app");
    fx.tmux(&["new-session", "-d", "-s", "keep"]);
    fx.tmux(&["set", "-g", "default-size", "6x4"]);
    let mut file = format!("root\t{root}\n");
    for _ in 0..6 {
        file.push_str("0\t\t1\t1\t\ttail -f /dev/null\n");
    }
    file.push_str("1\t\t0\t1\t\tsleep 300\n");
    fs::write(fx.layout(&root), file).unwrap();
    let out = fx.pj(&[&root]);
    // The whole session is built in one tmux call, which stops at the split with no room.
    assert!(
        text(&out).contains("not every pane could be restored"),
        "{}",
        text(&out)
    );
    let panes = fx
        .tmux(&["list-panes", "-s", "-t", "=app:"])
        .lines()
        .count();
    assert!((1..7).contains(&panes), "{panes} panes");
    assert_eq!(
        fx.root_of("app"),
        root,
        "the partial session is still a project"
    );
}

#[test]
fn an_exact_name_opens_without_the_picker() {
    let fx = Fx::new();
    let root = fx.project("code/q1");
    fs::write(fx.layout(&root), format!("root\t{root}\n0\t\t1\t1\t\t\n")).unwrap();
    fx.pj_tty(None, &[], &["q1"]);
    assert_eq!(fx.root_of("q1"), root);
    assert!(!fx.p("fzf.args").exists());
}

#[test]
fn two_exact_matches_go_through_the_picker() {
    let fx = Fx::new();
    let a = fx.project("code/a/app");
    let b = fx.project("code/b/app");
    fx.pj(&[&a]);
    fs::write(fx.layout(&b), format!("root\t{b}\n0\t\t1\t1\t\t\n")).unwrap();
    fx.pj_tty(None, &[("FAKE_FZF_PICK", &b)], &["app"]);
    assert!(fx.fzf_args().contains(&"app".to_owned()));
    assert_eq!(fx.fzf_in().len(), 3, "{:?}", fx.fzf_in());
    assert_eq!(fx.root_of("app-2"), b);
}

#[test]
fn a_bare_word_without_an_exact_match_is_never_a_folder() {
    let fx = Fx::new();
    fx.project("p9");
    fx.project("code/q1");
    fx.pj(&[&fx.s("code/q1")]);
    let out = fx.pj_tty(None, &[("FAKE_FZF_STATUS", "1")], &["p9"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    for flag in ["--select-1", "--exit-0", "p9"] {
        assert!(fx.fzf_args().contains(&flag.to_owned()), "{flag}");
    }
    assert!(!fx.sessions().contains(&"p9".to_owned()));
}

#[test]
fn the_picker_rows_follow_wt_switch() {
    let fx = Fx::new();
    let p2 = fx.project("code/p2");
    let old = fx.project("code/old");
    fx.pj(&[&old]);
    fx.pj(&["stop", "old"]);
    fx.pj(&[&p2]);
    fx.tmux(&["new-session", "-d", "-s", "main⏳", "-c", &fx.s("")]);
    let out = fx.pj_tty(Some("p2"), &[("FAKE_FZF_STATUS", "130")], &[]);
    assert_eq!(out.status.code(), Some(130));

    let rows = fx.fzf_in();
    assert!(
        rows[0].starts_with('\t')
            && ["Name", "Win", "Window", "Age", "Path"]
                .iter()
                .all(|t| rows[0].contains(t))
    );
    assert!(
        rows[1].starts_with(&format!("{p2}\t")),
        "recent first: {rows:?}"
    );
    assert!(
        fx.row(&p2).starts_with(&format!("{p2}\t▸● p2 ")),
        "{}",
        fx.row(&p2)
    );
    assert!(fx.row(&p2).contains("  3  "));
    let stale = fx.row(&old);
    assert!(
        stale.contains("\x1b[2m3\x1b[0m") && stale.contains("\x1b[2mnow\x1b[0m"),
        "{stale:?}"
    );
    let other = fx.row("main");
    assert!(
        other.starts_with("main\t\x1b[2m ◇ main⏳ ") && other.ends_with("tmux session\x1b[0m"),
        "{other:?}"
    );
    assert_eq!(rows.iter().filter(|r| r.contains('▸')).count(), 1);

    let args = fx.fzf_args();
    for option in [
        "--header-lines=1",
        "--layout=reverse",
        "--height=90%",
        "--highlight-line",
        "--no-separator",
        "--ansi",
        "--with-shell=sh -c",
    ] {
        assert!(args.contains(&option.to_owned()), "{option}: {args:?}");
    }
    assert!(!args.contains(&"--select-1".to_owned()));
    assert!(
        args.iter()
            .any(|a| a.starts_with("--preview=") && a.ends_with(" --preview {1}"))
    );
}

#[test]
fn picking_without_projects_says_so() {
    let fx = Fx::new();
    let out = fx.pj_tty(None, &[], &[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out).contains("no projects yet"));
}

#[test]
fn dash_goes_back() {
    let fx = Fx::new();
    let p1 = fx.project("code/p1");
    let p2 = fx.project("code/p2");
    let gone = fx.project("code/gone");
    fx.pj(&[&p1]);
    fx.pj(&[&p2]);
    fx.pj_in("p2", &["-"]);
    assert_eq!(fx.recent().first(), Some(&p1));

    fx.pj(&[&gone]);
    fx.pj(&[&p2]);
    fs::remove_dir_all(&gone).unwrap();
    fx.tmux(&["kill-session", "-t", "=gone:"]);
    fx.pj_in("p2", &["-"]);
    assert_eq!(fx.recent().first(), Some(&p1), "skips a deleted folder");

    fs::remove_file(fx.p("state/pj/recent")).unwrap();
    fx.pj_in("p2", &[&p2]);
    assert_eq!(fx.pj_in("p2", &["-"]).status.code(), Some(1));
}

#[test]
fn dash_returns_to_a_session_pj_did_not_create() {
    let fx = Fx::new();
    let p2 = fx.project("code/p2");
    fx.tmux(&["new-session", "-d", "-s", "main⏳", "-c", &fx.s("")]);
    fx.pj_in("main⏳", &[&p2]);
    assert_eq!(fx.recent(), [p2.clone(), "main".to_owned()]);
    fx.pj_in("p2", &["-"]);
    assert_eq!(fx.recent().first().map(String::as_str), Some("main"));
    fx.tmux(&["kill-session", "-t", "=main⏳:"]);
    let out = fx.pj_in("p2", &["-"]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "a closed session is not offered"
    );
}

#[test]
fn a_name_switches_to_a_session_pj_did_not_create() {
    let fx = Fx::new();
    fx.tmux(&["new-session", "-d", "-s", "live", "-c", &fx.s("")]);
    let out = fx.pj(&["live"]);
    assert!(text(&out).contains("not a terminal"), "{}", text(&out));
    assert_eq!(fx.root_of("live"), "");
}

#[test]
fn tmux_runs_the_hooks() {
    let fx = Fx::new();
    let root = fx.project("code/app");
    fx.pj(&[&root]);
    let _ = fs::remove_file(fx.p("state/pj/recent"));
    let mut client = fx.attach_client("app");
    fx.tmux(&["detach-client", "-s", "=app:"]);
    let _ = client.wait();
    for _ in 0..30 {
        if fx.layout(&root).exists() && !fx.recent().is_empty() {
            break;
        }
        sleep(Duration::from_millis(100));
    }
    assert!(fx.layout(&root).exists(), "detach saves the layout");
    assert_eq!(fx.recent(), [root], "attaching records the project");
}

#[test]
fn awkward_folders_keep_their_names() {
    let fx = Fx::new();
    let semi = fx.project("code/semi;");
    fx.pj(&[&semi]);
    assert_eq!(fx.root_of("semi"), semi);

    let quote = fx.project("code/it's my app");
    fx.pj(&[&quote]);
    assert_eq!(fx.root_of("itsmyapp"), quote);
}

#[test]
fn a_percent_clash_is_refused() {
    let fx = Fx::new();
    let first = fx.project("code/x%y");
    let second = fx.project("code/x/y");
    fx.pj(&[&first]);
    fx.pj(&["stop", "xy"]);
    let before = fs::read_to_string(fx.layout(&first)).unwrap();
    let out = fx.pj(&[&second]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out).contains(&format!("belongs to {first}")));
    assert_eq!(fs::read_to_string(fx.layout(&second)).unwrap(), before);
}

#[test]
fn a_missing_folder_and_an_old_layout_are_reported() {
    let fx = Fx::new();
    let gone = fx.s("code/gone6");
    fs::write(fx.layout(&gone), format!("root\t{gone}\n0\t\t1\t1\t\t\n")).unwrap();
    let out = fx.pj(&["gone6"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out).contains(&gone));

    let old = fx.project("code/p7");
    fs::write(
        fx.layout(&old),
        format!("root\t{old}\n0\tname\t\t1\t1\t\t\n"),
    )
    .unwrap();
    let out = fx.pj(&[&old]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out).contains("could be restored"));
}

#[test]
fn each_command_reads_tmux_once() {
    let fx = Fx::new();
    let p1 = fx.project("code/p1");
    let p2 = fx.project("code/p2");
    fx.pj(&[&p1]);
    fx.pj(&[&p2]);
    let path = fx.recording_path("calls");
    let rec = [("PATH", path.as_str())];

    fx.pj_tty(Some("p2"), &[rec[0], ("FAKE_FZF_STATUS", "130")], &[]);
    assert_eq!(fx.calls("calls", &["tmux"]), [1], "picker");
    fx.pj_tty(Some("p2"), &rec, &[&p1]);
    assert_eq!(fx.calls("calls", &["tmux"]), [2], "switch");
    fx.pj_tty(Some("p2"), &rec, &["-"]);
    assert_eq!(fx.calls("calls", &["tmux"]), [2], "dash");

    let p3 = fx.project("code/p3");
    fx.cmd(PJ).env("PATH", &path).arg(&p3).output().unwrap();
    assert_eq!(
        fx.calls("calls", &["tmux"]),
        [4],
        "new project outside tmux: read, build, finish, attach"
    );

    // With a client in p2, the switch at the end of the finishing call succeeds.
    let mut client = fx.attach_client("p2");
    let p4 = fx.project("code/p4");
    fx.pj_tty(Some("p2"), &rec, &[&p4]);
    assert_eq!(
        fx.calls("calls", &["tmux"]),
        [3],
        "new project inside tmux: read, build, finish with the switch"
    );
    let _ = client.kill();
    let _ = client.wait();

    let n = fx
        .tmux(&["display", "-p", "-t", "=p3:", "#{session_id}"])
        .replace('$', "");
    fx.cmd(PJ)
        .env("PATH", &path)
        .args(["--hook", "detached", &n])
        .output()
        .unwrap();
    fx.cmd(PJ)
        .env("PATH", &path)
        .args(["--hook", "visited", &n])
        .output()
        .unwrap();
    assert_eq!(fx.calls("calls", &["tmux"]), [2], "hooks");
}

#[test]
fn the_preview_shows_worktrees_and_history() {
    let fx = Fx::new();
    let repo = fx.project("code/repo");
    let git = |args: &[&str]| {
        fx.cmd("git")
            .args(["-C", &repo])
            .args(args)
            .output()
            .unwrap()
    };
    git(&["init", "-q"]);
    git(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@t",
        "commit",
        "-q",
        "--allow-empty",
        "-m",
        "first commit",
    ]);
    script(
        &fx.p("bin/wt"),
        "#!/bin/sh\necho \"WT $* COLUMNS=$COLUMNS\"\n",
    );

    let out = fx
        .cmd(PJ)
        .env("FZF_PREVIEW_COLUMNS", "70")
        .args(["--preview", &repo])
        .output()
        .unwrap();
    let shown = text(&out);
    let wt = shown
        .find(&format!("WT -C {repo} list COLUMNS=70"))
        .expect(&shown);
    assert!(shown[wt..].contains("first commit"), "{shown}");

    fs::remove_file(fx.p("bin/wt")).unwrap();
    let shown = text(&fx.pj(&["--preview", &repo]));
    assert!(!shown.contains("WT ") && shown.contains("first commit"));

    let plain = fx.project("code/plain");
    assert!(text(&fx.pj(&["--preview", &plain])).contains("Not a git repository"));

    fx.tmux(&["new-session", "-d", "-s", "other", "-c", &repo]);
    assert!(text(&fx.pj(&["--preview", "other"])).contains("first commit"));
}

#[test]
fn completions_list_projects_and_sessions() {
    let fx = Fx::new();
    let app = fx.project("code/app");
    let gone = fx.s("code/gone");
    fs::write(fx.layout(&gone), format!("root\t{gone}\n0\t\t1\t1\t\t\n")).unwrap();
    fx.pj(&[&app]);
    let projects = text(&fx.pj(&["--complete", "projects"]));
    assert_eq!(projects.trim_end(), format!("app\t{app}"));
    assert_eq!(text(&fx.pj(&["--complete", "sessions"])).trim_end(), "app");
}

#[test]
fn the_omp_example_resumes_the_conversation_it_holds() {
    let fx = Fx::new();
    let root = fx.project("code/app");
    fs::create_dir_all(fx.p("config/pj/resume")).unwrap();
    fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/examples/resume/omp"),
        fx.p("config/pj/resume/omp"),
    )
    .unwrap();
    // A fake omp. macOS kills a copied sleep, so it is a link.
    std::os::unix::fs::symlink("/bin/sleep", fx.p("bin/omp")).unwrap();
    let sessions = fx.p("home/.omp/agent/sessions/-app");
    fs::create_dir_all(sessions.join("s1")).unwrap();
    fs::write(sessions.join("s1.jsonl"), "").unwrap();
    fs::write(sessions.join("s1/sub.jsonl"), "").unwrap();
    let file = sessions.join("s1.jsonl").display().to_string();
    let sub = sessions.join("s1/sub.jsonl").display().to_string();
    fx.pj(&[&root]);
    // A login shell may reorder PATH, so the fake omp runs by its full path.
    let omp = fx.s("bin/omp");
    fx.run_in("=app:1", &format!("{omp} 300 3>>{file} 4>>{sub}"));
    fx.run_in("=app:2", &format!("{omp} 300"));
    fx.pj(&["save", "app"]);
    let lines = layout_lines(&fx.layout(&root));
    let command = |w: &str| {
        lines
            .iter()
            .find(|f| f[0] == w)
            .map(|f| f[5].clone())
            .unwrap()
    };
    assert_eq!(command("1"), format!("omp --resume '{file}'"));
    assert_eq!(command("2"), "omp -c");
}

#[test]
fn bash_completes_commands_projects_folders_and_sessions() {
    let fx = Fx::new();
    let app = fx.project("code/my app");
    // Projects come from pj, folders from the working directory, which holds only coffee/.
    fx.project("cwd/coffee");
    fx.pj(&[&app]);
    let bin = Path::new(PJ).parent().unwrap().display().to_string();
    let complete = |line: &str| {
        let script = format!(
            "source {}/completions/pj.bash\n\
             COMP_LINE={line:?}; read -ra COMP_WORDS <<<\"$COMP_LINE\"\n\
             [[ $COMP_LINE == *' ' ]] && COMP_WORDS+=('')\n\
             COMP_CWORD=$((${{#COMP_WORDS[@]}} - 1))\n\
             _pj; printf '%s\\n' \"${{COMPREPLY[@]}}\"",
            env!("CARGO_MANIFEST_DIR")
        );
        let out = fx
            .cmd("bash")
            .env("PATH", format!("{bin}:{}", fx.path))
            .current_dir(fx.p("cwd"))
            .args(["--norc", "-c", &script])
            .output()
            .unwrap();
        let mut words: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(String::from)
            .collect();
        words.sort();
        words
    };
    assert_eq!(complete("pj s"), ["save", "stop"]);
    assert_eq!(complete("pj my"), ["my app"]);
    assert_eq!(complete("pj co"), ["coffee"]);
    assert_eq!(complete("pj -"), ["-"]);
    assert_eq!(complete("pj stop "), ["myapp"]);
}
