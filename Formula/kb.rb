class Kb < Formula
  desc "Terminal knowledge base for operational snippets"
  homepage "https://github.com/cortiz/kb"
  license any_of: ["MIT", "Apache-2.0"]

  on_macos do
    on_arm do
      url "https://github.com/cortiz/kb/releases/download/v0.2.0/kb-0.2.0-aarch64-apple-darwin.tar.gz"
      sha256 "766e5408531970075c10ea868b55600837247f369764f3c2527709b2df8eabb2"
    end
    on_intel do
      url "https://github.com/cortiz/kb/releases/download/v0.2.0/kb-0.2.0-x86_64-apple-darwin.tar.gz"
      sha256 "01c5bc6d3581332f5a25b7ce654b1560bde596dd4e87ce4cfb3a195f8c034842"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/cortiz/kb/releases/download/v0.2.0/kb-0.2.0-aarch64-unknown-linux-musl.tar.gz"
      sha256 "e9e738df1151228d1667ad9f0fbb446f8d8022f84ddaf1726ccd464ea452f8f2"
    end
    on_intel do
      url "https://github.com/cortiz/kb/releases/download/v0.2.0/kb-0.2.0-x86_64-unknown-linux-musl.tar.gz"
      sha256 "290584f41dbc4ab2dbea8b4266c40574e87a7d5da5d25c0b9c51a75ce174b7e2"
    end
  end

  def install
    bin.install "kb"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/kb --version")
  end
end
