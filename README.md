# pajama

Slip into your projects. `pj` opens a tmux session per project, by path, folder name,
fuzzy picker or history. Each project keeps its window and pane layout, and the programs
you choose come back when it reopens.

![pj opening a project from the picker, then going back with pj -](https://raw.githubusercontent.com/rafi/assets/main/repo/pajama/demo.gif)

It has no dependencies beyond the standard library, and every command reads tmux once and
acts in one more call.

## Install

```sh
brew install rafi/tap/pajama
```

Homebrew also installs tmux, fzf, the man page and the completions. The omp resume example
lands in `$(brew --prefix)/share/pajama/examples`.

### Manual install

Download a build for Linux or macOS from the
[releases](https://github.com/rafi/pajama/releases). Each archive holds `pj`, the man
page, the completions and the examples. Or build it yourself:

```sh
cargo install --path .
```

Completions, for the shell you use:

```sh
cp completions/pj.fish ~/.config/fish/completions/   # fish
cp completions/pj.bash ~/.local/share/bash-completion/completions/pj # bash
cp completions/_pj ~/.zfunc/                         # zsh, with ~/.zfunc on $fpath before compinit
```

The man page:

```sh
cp man/pj.1 ~/.local/share/man/man1/
```

Needs tmux, and fzf 0.66 or newer for the picker.

## Usage

```sh
pj ~/code/app   # open the project in that folder
pj app          # open it by folder name
pj              # pick a project from a list
pj -            # go back to the previous one
```

### Opening a project

Each project gets its own tmux session. When you open one:

- **It's already running:** pj switches to its session.
- **It isn't running:** pj creates the session from the project's saved layout. A project
  opened for the first time uses `default.tsv` from the config folder, or one shell window
  without it.
- **Inside tmux** pj switches your client. **Outside tmux** it attaches.

pj reads the argument as a path when it contains `/`, is `.` or `..`, or starts with `~`.
Anything else is a name:

- A name matches project folder names and running session names.
- Only letters, digits, `.`, `_` and `-` count. Status marks that agent tools add to a
  session name are ignored, so `main⏳` matches `main`.
- When several projects match, the picker opens with just those.

### Going back

`pj -` returns to the project or session you were in before. Repeat it to flip between
two. The history is shared by all your terminals and survives a tmux restart. If the
previous project has been closed, `pj -` opens it again.

To flip with a key, add this to your tmux config:

```tmux
bind - run-shell "pj -"
```

### Saving and closing

A session's layout is saved:

- when you detach from it
- on `pj save`, for the current session or the one you name
- on `pj stop`, which then closes the session

A name after `save` and `stop` is a session name as `tmux ls` shows it.

### Sessions pj didn't create

pj can switch to any tmux session, including one started with `tmux new`:

- Its layout is never saved, because it has no project folder.
- `pj -` can go back to it if you left it with pj.
- If it shares a name with a project, `pj NAME` opens the project.

### The picker

![The picker with running, stopped and plain tmux sessions, and a project's commits](https://raw.githubusercontent.com/rafi/assets/main/repo/pajama/picker.png)

Running `pj` with no argument lists your projects and running sessions, most recent
first. Each row shows the name, window count, active window, time since last use and
path.

| Mark | Meaning |
|---|---|
| `▸` | the session you are in |
| `●` | a running project |
| `◇` | a session pj didn't create |
| dimmed | a stopped project, with the window count and age of its saved layout |

The preview shows the folder's worktrees with `wt list` when
[worktrunk](https://worktrunk.dev) is installed, then its last 15 commits. `alt-p` hides
the preview and `ctrl-u`/`ctrl-d` scroll it.

### All commands

| Command | Does |
|---|---|
| `pj PATH` | Open the project in that folder. |
| `pj NAME` | Open the project or session by name. |
| `pj` | Pick a project or session. |
| `pj -` | Go back to the previous project or session. |
| `pj save [NAME]` | Save a session's layout. |
| `pj stop [NAME]` | Save a session's layout and close it. |
| `pj --version` | Print the version. |

To notice when you detach or switch sessions, pj sets the global tmux hooks
`client-detached[42]` and `client-session-changed[42]`. They act only in pj's sessions
and leave your other hooks running.

## Files

| What | Where |
|---|---|
| Saved layouts | `$XDG_STATE_HOME/pj/<path with / as %>.tsv` |
| History | `$XDG_STATE_HOME/pj/recent` |
| Layout for new projects | `$XDG_CONFIG_HOME/pj/default.tsv`, else one shell window |
| Programs to rerun | `$XDG_CONFIG_HOME/pj/programs` |
| Resume scripts | `$XDG_CONFIG_HOME/pj/resume/<program>` |

## Restoring running programs

When pj saves a session, it looks at the program running in each pane, and decides
whether to start it again when the session comes back:

- `$XDG_CONFIG_HOME/pj/programs` lists program names, one per line, with `#` for
  comments. A listed program is started again with its full command line.
- An executable `$XDG_CONFIG_HOME/pj/resume/NAME` gets the PID of program `NAME` and
  prints the command to run instead. See [examples/resume/omp](examples/resume/omp).

## Tests

```sh
cargo test --release
```

Every integration test runs its own tmux server with its own state and config folders.

## License

MIT, see [LICENSE](LICENSE).
