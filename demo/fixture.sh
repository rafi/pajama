#!/usr/bin/env bash
# Builds a throwaway sandbox for the VHS demo: fake projects, their saved layouts, a
# history and a private tmux server with a few sessions running.
#
# $HOME, the pj state and config folders and the tmux socket all live under $1, so the
# recording never reads or changes your own sessions, history or layouts. Only the tools
# the demo uses are on PATH, which keeps worktrunk's table out of the picker preview.
#
# nvim is the exception. It reads your nvim config and plugins in place, so the editor in
# the screenshots looks like yours.
#
# Usage: fixture.sh <sandbox-dir> <pj-binary>

set -euo pipefail

usage="usage: fixture.sh <sandbox-dir> <pj-binary>"
SANDBOX=${1:?$usage}
PJ_BIN=${2:?$usage}

mkdir -p "$SANDBOX"
# pj resolves symlinks in project paths, so /tmp must already be /private/tmp here for
# the picker to show them under ~.
SANDBOX=$(cd "$SANDBOX" && pwd -P)
PJ_BIN=$(cd "$(dirname "$PJ_BIN")" && pwd -P)/$(basename "$PJ_BIN")
HERE=$(cd "$(dirname "$0")" && pwd -P)

REAL_CONFIG=${XDG_CONFIG_HOME:-$HOME/.config}
REAL_DATA=${XDG_DATA_HOME:-$HOME/.local/share}
REAL_CACHE=${XDG_CACHE_HOME:-$HOME/.cache}

HOME_DIR=$SANDBOX/home
CODE=$HOME_DIR/code
BIN=$SANDBOX/bin
mkdir -p "$BIN" "$CODE" "$SANDBOX/tmux" "$HOME_DIR/.config/pj" "$HOME_DIR/.local/state"

ln -sf "$PJ_BIN" "$BIN/pj"
# cargo and rustc let rust-analyzer load the api crate without an error popup in nvim.
for tool in fish fzf git tmux nvim lazygit cargo rustc; do
	path=$(command -v "$tool") || {
		echo "fixture: $tool is not installed" >&2
		exit 1
	}
	ln -sf "$path" "$BIN/$tool"
done

cat >"$SANDBOX/env.sh" <<EOF
export HOME='$HOME_DIR'
export PATH='$BIN:/usr/bin:/bin:/usr/sbin:/sbin'
export SHELL='$BIN/fish'
export TMUX_TMPDIR='$SANDBOX/tmux'
export XDG_CONFIG_HOME='$HOME_DIR/.config'
export XDG_STATE_HOME='$HOME_DIR/.local/state'
export XDG_DATA_HOME='$REAL_DATA'
export XDG_CACHE_HOME='$REAL_CACHE'
export GIT_CONFIG_GLOBAL='$HOME_DIR/.gitconfig'
export GIT_CONFIG_NOSYSTEM=1
export LANG=en_US.UTF-8
# The sandbox tmux server keeps this environment, and NO_COLOR would strip the colors
# from the picker preview.
unset TMUX TMUX_PANE FZF_DEFAULT_OPTS FZF_DEFAULT_OPTS_FILE NO_COLOR
EOF
# shellcheck source=/dev/null
source "$SANDBOX/env.sh"

mkdir -p "$XDG_CONFIG_HOME/tmux" "$XDG_CONFIG_HOME/fish"
ln -sfn "$REAL_CONFIG/nvim" "$XDG_CONFIG_HOME/nvim"
cp "$HERE/tmux.conf" "$XDG_CONFIG_HOME/tmux/tmux.conf"
{
	if [ -r "$REAL_CONFIG/fish/conf.d/theme.fish" ]; then
		echo "source '$REAL_CONFIG/fish/conf.d/theme.fish'"
	fi
	# fish on macOS adds /etc/paths.d to PATH, which would bring back every Homebrew tool.
	echo "set -gx PATH '$BIN' /usr/bin /bin /usr/sbin /sbin"
	cat <<'EOF'
set -g fish_greeting
# A fixed prompt that the tape can wait for.
function fish_prompt
	set_color 89b4fa; echo -n (prompt_pwd)' '
	set_color cba6f7; echo -n '❯ '
	set_color normal
end
EOF
} >"$XDG_CONFIG_HOME/fish/config.fish"

printf 'nvim\nlazygit\ntail\n' >"$XDG_CONFIG_HOME/pj/programs"
git config --global user.name 'Rafael Bodill'
git config --global user.email 'rafi@example.com'
git config --global init.defaultBranch main

# Commits are dated relative to now, so the preview reads "2 hours ago" whenever the demo
# is recorded.
NOW=$(date +%s)
commit() {
	local repo=$1 hours=$2 author=$3 message=$4
	local when="$((NOW - hours * 3600)) +0000"
	GIT_AUTHOR_NAME=$author GIT_AUTHOR_EMAIL="${author%% *}@example.com" \
		GIT_AUTHOR_DATE=$when GIT_COMMITTER_DATE=$when \
		git -C "$repo" commit -q --allow-empty -m "$message"
}
# Creates repository NAME with stdin as its README.
project() {
	local repo=$CODE/$1
	mkdir -p "$repo"
	git -C "$repo" init -q
	cat >"$repo/README.md"
	git -C "$repo" add -A
}

project api <<'EOF'
# api

The HTTP API behind the dashboard.
EOF
mkdir -p "$CODE/api/src" "$CODE/api/log"
printf '[package]\nname = "api"\nedition = "2024"\n' >"$CODE/api/Cargo.toml"
cat >"$CODE/api/src/main.rs" <<'EOF'
fn main() {
    println!("listening on 127.0.0.1:8080");
}
EOF
{
	paths=(/health /v1/projects /v1/projects/42 /v1/projects/7 /v1/projects)
	codes=(200 200 200 404 201)
	for i in $(seq 0 29); do
		j=$((i % 5))
		printf '14:%02d:%02d %-4s %-16s %s %2sms\n' $((i / 3 + 2)) $((i * 7 % 60)) \
			"$([ $j = 4 ] && echo POST || echo GET)" "${paths[$j]}" "${codes[$j]}" $((i * 13 % 40 + 2))
	done
} >"$CODE/api/log/dev.log"
git -C "$CODE/api" add -A
commit "$CODE/api" 400 'Rafael Bodill' 'Start the API with a health check'
commit "$CODE/api" 300 'Maya Chen' 'Add project routes'
commit "$CODE/api" 200 'Leo Okafor' 'Log every request with its latency'
git -C "$CODE/api" switch -q -c rate-limits
commit "$CODE/api" 30 'Maya Chen' 'Limit requests per client'
commit "$CODE/api" 26 'Maya Chen' 'Answer 429 with a Retry-After header'
git -C "$CODE/api" switch -q main
commit "$CODE/api" 20 'Leo Okafor' 'Return 404 for unknown projects'
when="$((NOW - 6 * 3600)) +0000"
GIT_AUTHOR_DATE=$when GIT_COMMITTER_DATE=$when \
	git -C "$CODE/api" merge -q --no-ff -m 'Merge rate limits' rate-limits
commit "$CODE/api" 5 'Rafael Bodill' 'Read the port from the environment'

project web <<'EOF'
# web

The dashboard.
EOF
commit "$CODE/web" 500 'Leo Okafor' 'Scaffold the dashboard'
commit "$CODE/web" 120 'Maya Chen' 'Show projects in a table'
commit "$CODE/web" 2 'Rafael Bodill' 'Sort projects by last change'

project docs <<'EOF'
# docs

Guides and the API reference.
EOF
commit "$CODE/docs" 900 'Rafael Bodill' 'Write the getting started guide'
commit "$CODE/docs" 80 'Maya Chen' 'Document rate limits'

project infra <<'EOF'
# infra

Terraform for the staging and production stacks.
EOF
commit "$CODE/infra" 2000 'Leo Okafor' 'Describe the staging stack'
commit "$CODE/infra" 400 'Leo Okafor' 'Add a database replica'

project cli <<'EOF'
# cli

The command line client.
EOF
commit "$CODE/cli" 700 'Rafael Bodill' 'Add the login command'
commit "$CODE/cli" 50 'Maya Chen' 'Print projects as JSON with --json'

# Outside tmux pj creates the session, then fails to attach without a terminal.
start() { pj "$CODE/$1" </dev/null >/dev/null 2>&1 || true; }
# pj saves only programs that already run, and nvim takes a moment to start.
settle() { sleep 1.5; }

start api
tmux send-keys -t =api: 'nvim src/main.rs' Enter
tmux split-window -h -l 45% -t =api: -c "$CODE/api"
tmux split-window -v -l 60% -t =api: -c "$CODE/api"
tmux send-keys -t =api: 'tail -f log/dev.log' Enter
tmux new-window -d -t =api: -c "$CODE/api"
tmux send-keys -t =api:1 lazygit Enter
# The shell pane is active, so pj - can be typed there after the restore.
tmux select-pane -t =api:0.1
settle
pj stop api

start docs
tmux send-keys -t =docs: 'nvim README.md' Enter
tmux split-window -h -l 40% -t =docs: -c "$CODE/docs"
settle
pj stop docs

start infra
tmux split-window -v -t =infra: -c "$CODE/infra"
pj stop infra

start cli
start web
tmux new-session -d -s notes -c "$HOME"

# The ages the picker shows for stopped projects come from their layout files.
age() { touch -t "$(date -v-"$2" +%Y%m%d%H%M)" "$XDG_STATE_HOME/pj/$(echo "$CODE/$1" | tr / %).tsv"; }
age api 5H
age docs 3d
age infra 16d

printf '%s\n' "$CODE/web" "$CODE/api" "$CODE/cli" notes "$CODE/docs" "$CODE/infra" \
	>"$XDG_STATE_HOME/pj/recent"
