# Where `just demo` copies the finished recording. Override to keep it here:
#   just DEMO_ASSETS=demo demo
DEMO_ASSETS := env('DEMO_ASSETS', home_directory() / 'code/rafi/assets/repo/pajama')

# build the release binary
build:
  cargo build --release

# run the tests
test:
  cargo test --release

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
