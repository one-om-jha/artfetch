class Artfetch < Formula
  desc "Browse and download museum art from the terminal"
  homepage "https://github.com/one-om-jha/artfetch"
  url "https://github.com/one-om-jha/artfetch.git", tag: "v0.2.0"
  head "https://github.com/one-om-jha/artfetch.git", branch: "main"

  depends_on "pkgconf" => :build
  depends_on "rust" => :build
  on_linux do
    depends_on "openssl@3"
  end

  def install
    system "cargo", "install", *std_cargo_args
  end

  test do
    assert_match "artfetch #{version}", shell_output("#{bin}/artfetch --version") unless build.head?
    assert_match "Usage:", shell_output("#{bin}/artfetch --help")
  end
end
