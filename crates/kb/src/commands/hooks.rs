//! `kb git install-hooks` / `kb git uninstall-hooks`: a pre-commit hook that runs the secret scan
//! on staged files, chained into any hook the user already has.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Subcommand;
use kb_core::git::GitRunner;

use crate::output::CliError;
use crate::platform::SystemGitRunner;

const BEGIN: &str = "# >>> kb pre-commit (managed by kb) >>>";
const END: &str = "# <<< kb pre-commit <<<";

#[derive(Subcommand, Debug, Clone)]
pub enum GitCommand {
    /// Install (or chain into) a pre-commit hook that blocks staged secrets.
    InstallHooks,
    /// Remove the hook block kb installed, leaving other hook content alone.
    UninstallHooks,
}

fn io(e: impl std::fmt::Display) -> CliError {
    CliError::new("io", e.to_string())
}

fn hook_path(root: &Path) -> Result<PathBuf, CliError> {
    let out = SystemGitRunner
        .run_git(root, &["rev-parse", "--git-path", "hooks/pre-commit"])
        .map_err(|_| CliError::new("usage", "not a git repository"))?;
    let p = PathBuf::from(out.trim());
    Ok(if p.is_absolute() { p } else { root.join(p) })
}

fn block(exe: &Path) -> String {
    let exe = exe.to_string_lossy().replace('\'', r"'\''");
    format!("{BEGIN}\n'{exe}' --root \"$PWD\" lint --staged --secrets-only || exit 1\n{END}\n")
}

/// `content` with the kb block removed.
fn strip(content: &str) -> String {
    let mut out = String::new();
    let mut skipping = false;
    for line in content.lines() {
        if line == BEGIN {
            skipping = true;
        } else if line == END {
            skipping = false;
        } else if !skipping {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// `content` with the kb block (re)inserted right after the shebang, so it runs before any
/// existing hook logic that might `exit` or `exec`.
fn install(content: &str, exe: &Path) -> String {
    let rest = strip(content);
    let (shebang, body) = match rest.split_once('\n') {
        Some((first, body)) if first.starts_with("#!") => (format!("{first}\n"), body.to_string()),
        _ if rest.starts_with("#!") => (format!("{rest}\n"), String::new()),
        _ => ("#!/bin/sh\n".to_string(), rest),
    };
    format!("{shebang}{}{body}", block(exe))
}

pub fn run_git(root: &Path, cmd: &GitCommand) -> Result<ExitCode, CliError> {
    let path = hook_path(root)?;
    let existing = fs::read_to_string(&path).unwrap_or_default();
    match cmd {
        GitCommand::InstallHooks => {
            let exe = std::env::current_exe().map_err(io)?;
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir).map_err(io)?;
            }
            fs::write(&path, install(&existing, &exe)).map_err(io)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).map_err(io)?;
            }
            println!("kb: installed pre-commit hook at {}", path.display());
        }
        GitCommand::UninstallHooks => {
            if existing.contains(BEGIN) {
                let rest = strip(&existing);
                // nothing but a shebang left: the hook was ours alone
                if rest
                    .lines()
                    .all(|l| l.trim().is_empty() || l.starts_with("#!"))
                {
                    fs::remove_file(&path).map_err(io)?;
                } else {
                    fs::write(&path, rest).map_err(io)?;
                }
                println!("kb: removed pre-commit hook block from {}", path.display());
            } else {
                println!("kb: no kb pre-commit hook installed");
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_chains_and_strip_restores() {
        let exe = Path::new("/bin/kb");
        let user = "#!/bin/sh\necho user\n";
        let once = install(user, exe);
        assert!(once.starts_with("#!/bin/sh\n# >>> kb"));
        assert!(once.ends_with("echo user\n"));
        assert_eq!(install(&once, exe), once, "idempotent");
        assert_eq!(strip(&once), user);
    }
}
