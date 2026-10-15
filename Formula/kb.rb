class Kb < Formula
  desc "Terminal knowledge base for operational snippets"
  homepage "https://github.com/cortiz/kb"
  license any_of: ["MIT", "Apache-2.0"]

  on_macos do
    on_arm do
      url "https://github.com/cortiz/kb/releases/download/v0.4.0/kb-0.4.0-macos-apple-silicon.tar.gz"
      sha256 "7db7be83fd19d68e20d3baf950514d5c4554b000c663332d144b65d98f43cc16"
    end
    on_intel do
      url "https://github.com/cortiz/kb/releases/download/v0.4.0/kb-0.4.0-macos-intel.tar.gz"
      sha256 "db0c138174c322f0e94b42221b4675608ee6b79240f0ce875591dd21cb573224"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/cortiz/kb/releases/download/v0.4.0/kb-0.4.0-linux-arm64.tar.gz"
      sha256 "37f9c59b6cecc72e4476a8793d00afa678e5417eb31c2a95f3e2fb1eb067b0f8"
    end
    on_intel do
      url "https://github.com/cortiz/kb/releases/download/v0.4.0/kb-0.4.0-linux-x86_64.tar.gz"
      sha256 "b04540d9d937883c3c7a0609c801394d4262c4b542198f85f8734761aa40ed4d"
    end
  end

  def install
    bin.install "kb"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/kb --version")
  end
end
