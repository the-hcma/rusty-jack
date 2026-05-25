# Homebrew formula template for the-hcma/homebrew-tap, e.g. Formula/rusty-jack.rb
#
#   brew tap the-hcma/tap
#   brew install rusty-jack
#
class RustyJack < Formula
  desc "Route HDMI audio for volume keys and wake Sony-like speakers"
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
    system "cargo", "install", *std_cargo_args, "--locked"
    pkgshare.install "config.example.json", "config.example.sony.json", "launchd"
  end

  def caveats
    <<~EOS
      To create config and install the per-user LaunchAgent:
        rusty-jack install

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
