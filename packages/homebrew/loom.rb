class Loom < Formula
  desc "Unified agentic ecosystem for sharing skills, sessions, and MCP servers"
  homepage "https://github.com/tusharmewara/loom"
  version "0.12.0"
  license "MIT"

  url "https://github.com/tusharmewara/loom/archive/refs/tags/v0.12.0.tar.gz"
  sha256 "PLACEHOLDER_SHA256"

  bottle do
    root_url "https://github.com/tusharmewara/homebrew-loom/releases/download/v0.12.0-bottles"
    sha256 cellar: :any, arm64_sonoma: "PLACEHOLDER_ARM64"
    sha256 cellar: :any, sonoma: "PLACEHOLDER_INTEL"
    sha256 cellar: :any_skip_relocation, x86_64_linux: "PLACEHOLDER_LINUX"
  end

  depends_on "rust" => :build

  def install
    bin.install "bin/loom"
    bin.install "bin/loomd"
  end

  service do
    run [opt_bin/"loomd"]
    working_dir Dir.home
    keep_alive true
  end

  test do
    assert_match "Loom", shell_output("#{bin}/loom --help")
  end
end