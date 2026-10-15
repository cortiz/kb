class Kb < Formula
  desc "Terminal knowledge base for operational snippets"
  homepage "https://github.com/cortiz/kb"
  license any_of: ["MIT", "Apache-2.0"]

  on_macos do
    on_arm do
      url "https://github.com/cortiz/kb/releases/download/v0.3.1/kb-0.3.1-macos-apple-silicon.tar.gz"
      sha256 "f65992e7c871b09cdbdbdba326d1be70b8f702e1626b1121088dd6349384d1fe"
    end
    on_intel do
      url "https://github.com/cortiz/kb/releases/download/v0.3.1/kb-0.3.1-macos-intel.tar.gz"
      sha256 "a430ff5c46f64a30be2af1d43e94e0faf036d14199dc9c2b15d601c9f3f5ffb6"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/cortiz/kb/releases/download/v0.3.1/kb-0.3.1-linux-arm64.tar.gz"
      sha256 "27e5b419a3643397c07d0cbdd4509a2fc6d0bb8db8646368031f92940286a3f7"
    end
    on_intel do
      url "https://github.com/cortiz/kb/releases/download/v0.3.1/kb-0.3.1-linux-x86_64.tar.gz"
      sha256 "725fae2d93b27184f58f6581b408067ec23ad7264e4935c323e28da2fd5b6695"
    end
  end

  def install
    bin.install "kb"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/kb --version")
  end
end
