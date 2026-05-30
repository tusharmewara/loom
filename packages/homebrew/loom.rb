class Loom < Formula
  desc "The Shared Agentic Ecosystem — orchestrate tools, MCP servers, skills, and sessions"
  homepage "https://github.com/tusharmewara/loom"
  url "https://github.com/tusharmewara/loom/archive/refs/tags/v0.3.0.tar.gz"
  sha256 "HEAD" # brew will compute this on install
  license "MIT"
  head "https://github.com/tusharmewara/loom.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", "--root", prefix, "--path", "crates/loom-cli"
    bin.install "target/release/loom"

    # Install daemon binary
    system "cargo", "install", "--root", prefix, "--path", "crates/loomd"
    bin.install "target/release/loomd"

    # Shell completions
    bash_completion.install "completions/loom.bash" if File.exist?("completions/loom.bash")
    zsh_completion.install "completions/_loom" if File.exist?("completions/_loom")
    fish_completion.install "completions/loom.fish" if File.exist?("completions/loom.fish")
  end

  test do
    output = shell_output("#{bin}/loom version 2>&1")
    assert_match "0.3.0", output
  end
end
