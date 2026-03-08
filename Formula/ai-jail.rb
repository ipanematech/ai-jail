class AiJail < Formula
  desc "Sandbox wrapper for AI coding agents"
  homepage "https://github.com/ipanematech/ai-jail"
  url "https://github.com/ipanematech/ai-jail.git",
      tag: "v0.5.4",
      revision: "598c98866917a65e0fd1c52a7b74ff371b4cbd27"
  version "0.5.4"
  license "GPL-3.0-only"

  depends_on "rust" => :build

  on_linux do
    depends_on "bubblewrap"
  end

  def install
    system "cargo", "install", *std_cargo_args(path: ".")
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/ai-jail --version")
  end
end
