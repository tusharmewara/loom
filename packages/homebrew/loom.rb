class Loom < Formula
  desc "The Shared Agentic Ecosystem — orchestrate tools, MCP servers, skills, and sessions"
  homepage "https://github.com/tusharmewara/loom"
  url "https://github.com/tusharmewara/loom/archive/refs/tags/v0.9.1.tar.gz"
  sha256 "0019dfc4b32d63c1392aa264aed2253c1e0c2fb09216f8e2cc269bbfb8bb49b5"
  license "MIT"
  head "https://github.com/tusharmewara/loom.git", branch: "main"

  depends_on "rust" => :build

  bottle do
    root_url "https://github.com/tusharmewara/loom/releases/download/v0.9.1"
    sha256 arm64_sonoma: "358c43bebf087789c357356c297e21339dbe91d61939fee2e90f71b575c7541f"
  end

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
    assert_match "0.9.1", output
  end
end
