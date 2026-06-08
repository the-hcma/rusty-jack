# Homebrew formula reference for the-hcma/homebrew-tap.
#
# Stable releases are rendered from packaging/homebrew/rusty-jack.formula.in:
#   make render-homebrew-formula ARCHIVE_URL=... ARCHIVE_SHA256=...
#
#   brew tap the-hcma/tap
#   brew install rusty-jack
#
class RustyJack < Formula
  desc "Route HDMI audio for volume keys and wake ScalarWebAPI-compatible speakers"
  homepage "https://github.com/the-hcma/rusty-jack"
  license "MIT"
  head "https://github.com/the-hcma/rusty-jack.git", branch: "main"

  # Stable tap releases use a versioned URL + sha256:
  # url "https://github.com/the-hcma/rusty-jack/archive/refs/tags/v0.1.1.tar.gz"
  # sha256 "..."

  depends_on "rust" => :build
  depends_on macos: :monterey

  def install
    ENV["MACOSX_DEPLOYMENT_TARGET"] = "12.0"
    system "cargo", "install", *std_cargo_args
    system "make", "driver-bundle"
    pkgshare.install "config.example.json", "config.example.scalar-webapi-device.json", "launchd"
    pkgshare.install "target/share/rusty-jack/RustyJack.driver"
  end

  def uninstall
    safe_system bin/"rusty-jack", "disable", "--json"
  end

  def caveats
    <<~EOS
      After installing the formula, set up config and the per-user LaunchAgent:
        rusty-jack install

      Check routing and daemon state (including log paths):
        rusty-jack status

      Before uninstalling the formula, stop and remove the LaunchAgent:
        rusty-jack uninstall --keep-config

      To remove the default config too:
        rusty-jack uninstall --remove-config
    EOS
  end

  test do
    assert_match "rusty-jack", shell_output("#{bin}/rusty-jack --help")
  end
end
