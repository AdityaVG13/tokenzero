class Tokenzero < Formula
  desc "Recovery-aware context compression runtime for AI coding agents"
  homepage "https://github.com/AdityaVG13/tokenzero"
  version "0.1.1"
  license "MIT"

  url "file://#{File.expand_path("../../target/release/tokenzero", __dir__)}"
  sha256 :no_check

  def install
    bin.install "tokenzero"
  end

  test do
    assert_match "tokenzero", shell_output("#{bin}/tokenzero --version")
    system "#{bin}/tokenzero", "doctor", "--json"
  end
end
