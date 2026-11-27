# pj

Open a tmux session per project, by path, folder name, fuzzy picker or history. Each
project keeps its window and pane layout, and the programs you choose come back when it
reopens.

It has no dependencies beyond the standard library, and every command reads tmux once and
acts in one more call.

## Install

```sh
cargo install --path .
cp completions/pj.fish ~/.config/fish/completions/
```

Needs tmux, and fzf 0.66 or newer for the picker.

## Usage

| Command | Does |
|---|---|
| `pj PATH` | Open the project at that directory, creating its session if needed. |
| `pj NAME` | Open the project whose folder is named `NAME`. Several or none: pick with fzf, `NAME` typed in. |
| `pj` | Pick a project or session, most recent first. |
| `pj -` | Go back to the previous project or session. |
| `pj save [NAME]` | Save a session's layout, the current one by default. |
| `pj stop [NAME]` | Save the layout and close the session. |

An argument is a path when it contains `/`, is `.` or `..`, or starts with `~`. `NAME`
after `save` and `stop` is a session name as `tmux ls` shows it.

`pj NAME` matches project folder names and running session names, ignoring the marks agent
tools append (`main⏳` matches `main`). A session pj did not create matches only when no
project does. pj never saves such a session, but `pj -` returns to one you left with pj.

Inside tmux pj switches the client, outside it attaches. Layouts are saved when you
detach, and on `pj save` and `pj stop`.

### The picker

The layout of `wt switch` from [worktrunk](https://worktrunk.dev). `▸` marks the session
you are in, `●` a running project, and `◇` a session pj did not create, dimmed. A
stopped project shows its saved window count and the age of its layout, dimmed.

The preview shows the `wt list` table of the folder's worktrees when worktrunk is
installed, then the last 15 commits. `alt-p` hides it and `ctrl-u`/`ctrl-d` scroll it.

## Files

| What | Where |
|---|---|
| Saved layouts | `$XDG_STATE_HOME/pj/<path with / as %>.tsv` |
| History | `$XDG_STATE_HOME/pj/recent` |
| Layout for new projects | `$XDG_CONFIG_HOME/pj/default.tsv`, else one shell window |
| Programs to rerun | `$XDG_CONFIG_HOME/pj/programs` |
| Resume scripts | `$XDG_CONFIG_HOME/pj/resume/<program>` |

## Bringing programs back

When pj saves a pane, it looks at the program the pane's shell runs.

- `programs` lists program names, one per line, `#` for comments. A listed program comes
  back with its full command line.
- An executable `resume/NAME` decides for program `NAME`, listed or not. It gets the
  program's PID and prints the command to type when the session comes back, or nothing
  for a plain shell. [examples/resume/omp](examples/resume/omp) resumes the conversation
  an omp agent has open.

## Tests

```sh
cargo test --release
```

Every integration test runs its own tmux server with its own state and config folders.
