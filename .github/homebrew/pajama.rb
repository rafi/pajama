class Pajama < Formula
  desc "Open a tmux session per project and restore its pane layout"
  homepage "https://github.com/rafi/pajama"
  license "MIT"

  head do
    url "https://github.com/rafi/pajama.git", branch: "master"
    depends_on "rust" => :build
  end

  depends_on "fzf"
  depends_on "tmux"

  on_macos do
    on_arm do
      url "https://github.com/rafi/pajama/releases/download/@TAG@/pajama-@TAG@-darwin-arm64.tar.gz"
      sha256 "@DARWIN_ARM64@"
    end
    on_intel do
      url "https://github.com/rafi/pajama/releases/download/@TAG@/pajama-@TAG@-darwin-amd64.tar.gz"
      sha256 "@DARWIN_AMD64@"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/rafi/pajama/releases/download/@TAG@/pajama-@TAG@-linux-arm64.tar.gz"
      sha256 "@LINUX_ARM64@"
    end
    on_intel do
      url "https://github.com/rafi/pajama/releases/download/@TAG@/pajama-@TAG@-linux-amd64.tar.gz"
      sha256 "@LINUX_AMD64@"
    end
  end

  def install
    if build.head?
      system "cargo", "install", *std_cargo_args
    else
      bin.install "pj"
    end
    man1.install "man/pj.1"
    bash_completion.install "completions/pj.bash" => "pj"
    zsh_completion.install "completions/_pj"
    fish_completion.install "completions/pj.fish"
    pkgshare.install "examples"
  end

  def caveats
    <<~EOS
      To bring omp conversations back with their sessions, copy the example:
        mkdir -p ~/.config/pj/resume
        cp #{opt_pkgshare}/examples/resume/omp ~/.config/pj/resume/
    EOS
  end

  test do
    assert_match "pajama #{version}", shell_output("#{bin}/pj --version")
    # pj reads tmux sessions first, so it gets a tmux folder with no server in it.
    ENV.delete("TMUX")
    ENV["TMUX_TMPDIR"] = testpath
    assert_match "not a directory", shell_output("#{bin}/pj #{testpath}/missing/ 2>&1", 1)
  end
end
