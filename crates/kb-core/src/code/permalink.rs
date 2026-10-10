//! Web permalink generation for GitHub, GitLab, Bitbucket, and custom hosts.

use super::clone::{LocalClone, RemoteHost, normalize_remote_url};
use crate::config::Settings;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermalinkError {
    InvalidRef(String),
    MissingTemplateField(String),
}

impl std::fmt::Display for PermalinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PermalinkError::InvalidRef(s) => write!(f, "invalid ref: {s}"),
            PermalinkError::MissingTemplateField(s) => write!(f, "missing template field: {s}"),
        }
    }
}

impl std::error::Error for PermalinkError {}

pub fn build_permalink_impl(
    clone: Option<&LocalClone>,
    repo_id: &str,
    commit_sha: &str,
    path: &str,
    line: usize,
    end_line: Option<usize>,
    settings: &Settings,
) -> Result<String, PermalinkError> {
    let sha = commit_sha.trim();
    if sha.is_empty() || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(PermalinkError::InvalidRef(commit_sha.to_string()));
    }

    let (host, canonical_repo) = if let Some(c) = clone {
        (c.host.clone(), c.repo_identifier.clone())
    } else {
        normalize_remote_url(repo_id)
    };

    match host {
        RemoteHost::GitHub => {
            let anchor = match end_line {
                Some(el) if el > line => format!("#L{line}-L{el}"),
                _ => format!("#L{line}"),
            };
            Ok(format!(
                "https://github.com/{canonical_repo}/blob/{sha}/{path}{anchor}"
            ))
        }
        RemoteHost::GitLab => {
            let anchor = match end_line {
                Some(el) if el > line => format!("#L{line}-{el}"),
                _ => format!("#L{line}"),
            };
            Ok(format!(
                "https://gitlab.com/{canonical_repo}/-/blob/{sha}/{path}{anchor}"
            ))
        }
        RemoteHost::Bitbucket => {
            let anchor = match end_line {
                Some(el) if el > line => format!("#lines-{line}:{el}"),
                _ => format!("#lines-{line}"),
            };
            Ok(format!(
                "https://bitbucket.org/{canonical_repo}/src/{sha}/{path}{anchor}"
            ))
        }
        RemoteHost::Custom(_) => {
            if let Some(tpl) = settings.code_web_url() {
                if tpl.is_empty() {
                    return Err(PermalinkError::MissingTemplateField(
                        "code.web_url is empty".to_string(),
                    ));
                }
                let end_line_str = end_line
                    .map(|l| l.to_string())
                    .unwrap_or_else(|| line.to_string());
                let result = tpl
                    .replace("{repo}", &canonical_repo)
                    .replace("{ref}", sha)
                    .replace("{path}", path)
                    .replace("{line}", &line.to_string())
                    .replace("{end_line}", &end_line_str);
                Ok(result)
            } else {
                Err(PermalinkError::MissingTemplateField(
                    "code.web_url template required for custom host".to_string(),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_github_permalinks() {
        let settings = Settings::default();
        let sha = "dbde2191484fd5f051b5fe629936185b5760afbd";

        // Single line
        let url = build_permalink_impl(
            None,
            "owner/repo",
            sha,
            "src/Auth.java",
            12,
            None,
            &settings,
        )
        .unwrap();
        assert_eq!(
            url,
            "https://github.com/owner/repo/blob/dbde2191484fd5f051b5fe629936185b5760afbd/src/Auth.java#L12"
        );

        // Range
        let url_range = build_permalink_impl(
            None,
            "owner/repo",
            sha,
            "src/Auth.java",
            12,
            Some(20),
            &settings,
        )
        .unwrap();
        assert_eq!(
            url_range,
            "https://github.com/owner/repo/blob/dbde2191484fd5f051b5fe629936185b5760afbd/src/Auth.java#L12-L20"
        );
    }

    #[test]
    fn test_gitlab_permalinks() {
        let settings = Settings::default();
        let sha = "dbde2191484fd5f051b5fe629936185b5760afbd";

        let url = build_permalink_impl(
            None,
            "https://gitlab.com/group/repo.git",
            sha,
            "src/main.rs",
            42,
            Some(45),
            &settings,
        )
        .unwrap();
        assert_eq!(
            url,
            "https://gitlab.com/group/repo/-/blob/dbde2191484fd5f051b5fe629936185b5760afbd/src/main.rs#L42-45"
        );
    }

    #[test]
    fn test_bitbucket_permalinks() {
        let settings = Settings::default();
        let sha = "dbde2191484fd5f051b5fe629936185b5760afbd";

        let url = build_permalink_impl(
            None,
            "git@bitbucket.org:team/service.git",
            sha,
            "pkg/app.go",
            15,
            Some(18),
            &settings,
        )
        .unwrap();
        assert_eq!(
            url,
            "https://bitbucket.org/team/service/src/dbde2191484fd5f051b5fe629936185b5760afbd/pkg/app.go#lines-15:18"
        );
    }

    #[test]
    fn test_custom_template_permalinks() {
        let mut settings = Settings::default();
        settings.set_text(
            "code.web_url",
            "https://source.internal.net/{repo}/src/{ref}/{path}#L{line}-L{end_line}",
        );
        let sha = "dbde2191484fd5f051b5fe629936185b5760afbd";

        let url = build_permalink_impl(
            None,
            "git@source.internal.net:org/custom.git",
            sha,
            "lib/util.py",
            5,
            Some(10),
            &settings,
        )
        .unwrap();
        assert_eq!(
            url,
            "https://source.internal.net/org/custom/src/dbde2191484fd5f051b5fe629936185b5760afbd/lib/util.py#L5-L10"
        );

        // Without template returns error
        let clean_settings = Settings::default();
        let err = build_permalink_impl(
            None,
            "git@source.internal.net:org/custom.git",
            sha,
            "lib/util.py",
            5,
            Some(10),
            &clean_settings,
        )
        .unwrap_err();
        assert!(matches!(err, PermalinkError::MissingTemplateField(_)));
    }

    #[test]
    fn test_invalid_ref_error() {
        let settings = Settings::default();
        let err = build_permalink_impl(
            None,
            "owner/repo",
            "not-a-valid-sha!@#",
            "file.txt",
            1,
            None,
            &settings,
        )
        .unwrap_err();
        assert_eq!(
            err,
            PermalinkError::InvalidRef("not-a-valid-sha!@#".to_string())
        );
    }
}
