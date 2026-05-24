# Homebrew formula (template) — use in a personal tap, e.g. homebrew-tap/Formula/rusty-jack.rb
#
#   brew tap YOUR_USER/tap
#   brew install rusty-jack
#
class RustyJack < Formula
  desc "macOS daemon: route system audio to your HDMI/dock output"
  homepage "https://github.com/YOUR_USER/rusty-jack"
  license "MIT"
  head "https://github.com/YOUR_USER/rusty-jack.git", branch: "main"

  # After first release, prefer versioned URL + sha256 bottles:
  # url "https://github.com/YOUR_USER/rusty-jack/archive/refs/tags/v0.1.0.tar.gz"
  # sha256 "..."

  depends_on :macos
  depends_on macos: :monterey
  depends_on "rust" => :build

  def install
    ENV["MACOSX_DEPLOYMENT_TARGET"] = "12.0"
    system "cargo", "install", *std_cargo_args, "--locked"
  end

  def uninstall
    # Full cleanup: agent, state, logs. Do not change audio output on brew uninstall.
    safe_system bin/"rusty-jack", "uninstall", "--yes", "--purge", "--no-restore-audio"
  rescue StandardError
    opoo "rusty-jack uninstall hook failed; remove ~/Library/LaunchAgents/*rusty-jack*.plist manually"
  end

  test do
    assert_match "rusty-jack", shell_output("#{bin}/rusty-jack --help")
  end
end
