//! Git snippet extraction.

use serde::{Deserialize, Serialize};

use super::clone::{GitError, GitRunner, LocalClone};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeSnippet {
    pub lines: Vec<(usize, String)>,
    pub target_line: usize,
    pub target_end_line: Option<usize>,
}

pub fn get_snippet_impl(
    clone: &LocalClone,
    pinned_ref: &str,
    path: &str,
    line: usize,
    end_line: Option<usize>,
    context_before: usize,
    context_after: usize,
    git_runner: &impl GitRunner,
) -> Result<CodeSnippet, GitError> {
    let show_arg = format!("{pinned_ref}:{path}");
    let content = git_runner.run_git(&clone.local_path, &["show", &show_arg])?;

    let all_lines: Vec<&str> = content.lines().collect();
    let total_lines = all_lines.len();

    let target_start = line.max(1);
    let target_end = end_line.unwrap_or(target_start).max(target_start);

    let snippet_start = target_start.saturating_sub(context_before).max(1);
    let snippet_end = (target_end + context_after).min(total_lines);

    let mut snippet_lines = Vec::new();
    for l_num in snippet_start..=snippet_end {
        if l_num <= total_lines {
            let line_str = all_lines[l_num - 1].to_string();
            snippet_lines.push((l_num, line_str));
        }
    }

    Ok(CodeSnippet {
        lines: snippet_lines,
        target_line: target_start,
        target_end_line: end_line,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use crate::code::clone::RemoteHost;

    struct MockRunner {
        content: String,
    }

    impl GitRunner for MockRunner {
        fn run_git(&self, _cwd: &Path, args: &[&str]) -> Result<String, GitError> {
            if args.len() == 2 && args[0] == "show" {
                Ok(self.content.clone())
            } else {
                Err(GitError::NotFound("command not mocked".to_string()))
            }
        }
    }

    #[test]
    fn test_snippet_extraction_with_context() {
        // 20 lines of code
        let file_content = (1..=20)
            .map(|i| format!("Line {i} content"))
            .collect::<Vec<_>>()
            .join("\n");

        let clone = LocalClone {
            repo_identifier: "org/repo".to_string(),
            local_path: Path::new("/mock/repo").to_path_buf(),
            remote_url: "git@github.com:org/repo.git".to_string(),
            host: RemoteHost::GitHub,
            tracking_branch: "origin/main".to_string(),
            has_local_changes: false,
        };

        let runner = MockRunner {
            content: file_content,
        };

        // Pin line 10 with 3 lines context before and 8 lines after
        let snippet = get_snippet_impl(
            &clone,
            "abcdef1",
            "src/file.rs",
            10,
            None,
            3,
            8,
            &runner,
        )
        .unwrap();

        assert_eq!(snippet.target_line, 10);
        assert_eq!(snippet.target_end_line, None);
        // Start line: 10 - 3 = 7, end line: 10 + 8 = 18
        assert_eq!(snippet.lines.first().unwrap().0, 7);
        assert_eq!(snippet.lines.last().unwrap().0, 18);
        assert_eq!(snippet.lines.len(), 12);
        assert_eq!(snippet.lines[0].1, "Line 7 content");
        assert_eq!(snippet.lines[3].1, "Line 10 content");
        assert_eq!(snippet.lines[11].1, "Line 18 content");
    }

    #[test]
    fn test_snippet_extraction_range_clamping() {
        let file_content = (1..=10)
            .map(|i| format!("Line {i}"))
            .collect::<Vec<_>>()
            .join("\n");

        let clone = LocalClone {
            repo_identifier: "org/repo".to_string(),
            local_path: Path::new("/mock/repo").to_path_buf(),
            remote_url: "git@github.com:org/repo.git".to_string(),
            host: RemoteHost::GitHub,
            tracking_branch: "origin/main".to_string(),
            has_local_changes: false,
        };

        let runner = MockRunner {
            content: file_content,
        };

        // Pin line 2 with context_before 5 (should clamp to line 1) and end_line 8 with context_after 5 (should clamp to 10)
        let snippet = get_snippet_impl(
            &clone,
            "abcdef1",
            "src/file.rs",
            2,
            Some(8),
            5,
            5,
            &runner,
        )
        .unwrap();

        assert_eq!(snippet.target_line, 2);
        assert_eq!(snippet.target_end_line, Some(8));
        assert_eq!(snippet.lines.first().unwrap().0, 1);
        assert_eq!(snippet.lines.last().unwrap().0, 10);
        assert_eq!(snippet.lines.len(), 10);
    }
}
