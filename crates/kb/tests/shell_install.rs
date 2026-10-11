//! Integration tests for `kb shell install` and `kb shell uninstall`.

mod support;

use std::fs;
use std::path::Path;
use support::kb;

struct ShellHome {
    dir: tempfile::TempDir,
}

impl ShellHome {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn kb(&self, args: &[&str], shell_env: &str) -> std::process::Output {
        let mut cmd = kb();
        cmd.env("HOME", self.path())
            .env("SHELL", shell_env)
            .args(args);
        cmd.output().unwrap()
    }
}

#[test]
fn test_shell_install_creates_marked_block_in_zshrc() {
    let home = ShellHome::new();
    let zshrc = home.path().join(".zshrc");
    fs::write(&zshrc, "# existing config\nexport FOO=bar\n").unwrap();

    let out = home.kb(&["shell", "install", "-y", "--shell", "zsh"], "/bin/zsh");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));

    let content = fs::read_to_string(&zshrc).unwrap();
    assert!(content.contains("# >>> kb >>>"));
    assert!(content.contains(r#"eval "$(kb shell init zsh)""#));
    assert!(content.contains("# <<< kb <<<"));
    assert!(content.contains("export FOO=bar"));

    // Idempotency: running again changes nothing
    let out2 = home.kb(&["shell", "install", "-y", "--shell", "zsh"], "/bin/zsh");
    assert!(out2.status.success());
    let content2 = fs::read_to_string(&zshrc).unwrap();
    assert_eq!(content, content2);
}

#[test]
fn test_shell_uninstall_removes_marked_block_cleanly() {
    let home = ShellHome::new();
    let zshrc = home.path().join(".zshrc");
    fs::write(&zshrc, "# header\n\n# >>> kb >>>\neval \"$(kb shell init zsh)\"\n# <<< kb <<<\n\n# footer\n").unwrap();

    let out = home.kb(&["shell", "uninstall", "-y", "--shell", "zsh"], "/bin/zsh");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));

    let content = fs::read_to_string(&zshrc).unwrap();
    assert!(!content.contains("# >>> kb >>>"));
    assert!(!content.contains("eval \"$(kb shell init zsh)\""));
    assert!(!content.contains("# <<< kb <<<"));
    assert!(content.contains("# header"));
    assert!(content.contains("# footer"));
}

#[test]
fn test_shell_install_fish_config() {
    let home = ShellHome::new();
    let fish_config = home.path().join(".config/fish/config.fish");
    fs::create_dir_all(fish_config.parent().unwrap()).unwrap();
    fs::write(&fish_config, "# fish existing\n").unwrap();

    let out = home.kb(&["shell", "install", "-y", "--shell", "fish"], "/usr/bin/fish");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));

    let content = fs::read_to_string(&fish_config).unwrap();
    assert!(content.contains("# >>> kb >>>"));
    assert!(content.contains("kb shell init fish | source"));
    assert!(content.contains("# <<< kb <<<"));
}

#[test]
fn test_shell_init_cli_outputs_script() {
    let home = ShellHome::new();
    let out = home.kb(&["shell", "init", "zsh"], "/bin/zsh");
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("_kb_widget"));
    assert!(text.contains("zle reset-prompt"));
}

