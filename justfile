# Where `just demo` copies the finished recording. Override to keep it here:
#   just DEMO_ASSETS=demo demo
DEMO_ASSETS := env('DEMO_ASSETS', home_directory() / 'code/rafi/assets/repo/pajama')

# build the release binary
build:
  cargo build --release

# run the tests
test:
  cargo test --release

# check, push master and tag the version in Cargo.toml, which starts the release build
release:
  #!/usr/bin/env bash
  set -euo pipefail
  tag="v$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
  fail() { echo "release: $*" >&2; exit 1; }
  [ "$(git branch --show-current)" = master ] || fail "not on master"
  [ -z "$(git status --porcelain --untracked-files=no)" ] || fail "uncommitted changes"
  git rev-parse -q --verify "refs/tags/$tag" >/dev/null && fail "$tag exists here"
  git ls-remote --exit-code --tags origin "$tag" >/dev/null && fail "$tag exists on origin"
  # --locked fails when Cargo.lock still holds the old version.
  cargo fmt --check
  cargo clippy --release --all-targets --locked -- -D warnings
  cargo test --release --locked
  git push origin master
  git tag "$tag"
  git push origin "$tag"
  echo "pushed $tag, follow the build with: gh run watch -R rafi/pajama"

# record the demo GIF and screenshots against a temp directory
demo: build
  #!/usr/bin/env bash
  set -euo pipefail
  sandbox=$(mktemp -d /tmp/pajama-demo.XXXX)
  # The sandbox runs its own tmux server, which would outlive the recording. tmux follows
  # $TMUX before TMUX_TMPDIR, so inside your tmux only -S reaches the sandbox server.
  trap 'tmux -S "$sandbox/tmux/tmux-$(id -u)/default" kill-server 2>/dev/null || true; rm -rf "$sandbox"' EXIT

  ./demo/fixture.sh "$sandbox" target/release/pj

  rm -f demo/demo.gif demo/*.png
  DEMO_SANDBOX="$sandbox" PATH="$sandbox/bin:$PATH" vhs demo/demo.tape

  dest='{{ DEMO_ASSETS }}'
  if [ "$dest" != "demo" ]; then
    mkdir -p "$dest"
    mv demo/demo.gif demo/picker.png "$dest/"
  fi
  echo "demo written to $dest: demo.gif, picker.png"
