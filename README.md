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

| Command | Does |
|---|---|
| `pj PATH` | Open the project at that directory, creating its session if needed. |
| `pj NAME` | Open the project whose folder is named `NAME`, or picker if several match. |
| `pj` | Pick a project or session, most recent first. |
| `pj -` | Go back to the previous project or session. |
| `pj save [NAME]` | Save a session's layout, the current one by default. |
| `pj stop [NAME]` | Save the layout and close the session. |
| `pj --version` | Print the version. |

An argument is a path when it contains `/`, is `.` or `..`, or starts with `~`. `NAME`
after `save` and `stop` is a session name as `tmux ls` shows it.

`pj NAME` matches project folder names and running session names. Only letters, digits,
`.`, `_` and `-` count, so status marks that agent tools might add to a session name
are ignored: `main⏳` matches `main`.

pj also switches to tmux sessions it did not create, such as one started with `tmux new`.
When such a session has the same name as a project, `pj NAME` opens the project. pj never
saves the layout of these sessions, because they have no project folder, but `pj -` goes
back to one you left with pj.

Inside tmux pj switches the client, outside it attaches. Layouts are saved when you
detach, and on `pj save` and `pj stop`.

### The picker

A table of your projects and running sessions, most recent first, with columns for the
name, the window count, the active window, the time since last use, and the path. `▸`
marks the session you are in, `●` a running project, and `◇` a session pj did not create.
A stopped project is dimmed and shows the window count and age of its saved layout.

The preview shows the `wt list` table of the folder's worktrees when
[worktrunk](https://worktrunk.dev) is installed, then the last 15 commits. `alt-p` hides
it and `ctrl-u`/`ctrl-d` scroll it.

![The picker with running, stopped and plain tmux sessions, and a project's commits](https://raw.githubusercontent.com/rafi/assets/main/repo/pajama/picker.png)

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

## Demo

`just demo` records the GIF and the picker screenshot with
[VHS](https://github.com/charmbracelet/vhs) against a throwaway sandbox with its own
projects and tmux server, and copies them to `~/code/rafi/assets/repo/pajama`.

## License

MIT, see [LICENSE](LICENSE).
