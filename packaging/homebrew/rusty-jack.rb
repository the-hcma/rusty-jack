# Homebrew formula for thehcma/homebrew-tap, e.g. Formula/rusty-jack.rb
#
#   brew tap thehcma/tap
#   brew install rusty-jack
#
class RustyJack < Formula
  desc "Route macOS audio to HDMI/dock outputs with launchd automation"
  homepage "https://github.com/thehcma/rusty-jack"
  license "MIT"
  head "https://github.com/thehcma/rusty-jack.git", branch: "main"

  # Stable tap releases use a versioned URL + sha256:
  # url "https://github.com/thehcma/rusty-jack/archive/refs/tags/v0.1.0.tar.gz"
  # sha256 "..."

  depends_on macos: :monterey
  depends_on "rust" => :build

  def install
    ENV["MACOSX_DEPLOYMENT_TARGET"] = "12.0"
    system "cargo", "install", *std_cargo_args, "--locked"
    pkgshare.install "config.example.json", "config.example.sony.json", "launchd"
  end

  def caveats
    <<~EOS
      To create a starter config:
        mkdir -p ~/.config/rusty-jack
        cp #{opt_pkgshare}/config.example.json ~/.config/rusty-jack/config.json

      To install the per-user LaunchAgent after configuring:
        rusty-jack install

      Before uninstalling the formula, stop and remove the LaunchAgent:
        rusty-jack uninstall --yes --purge --no-restore-audio
    EOS
  end

  test do
    assert_match "rusty-jack", shell_output("#{bin}/rusty-jack --help")
  end
end
