//! Command execution plans, destructive pattern scanning, and secret environment isolation.

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use regex::Regex;

use crate::config::CustomActionConfig;
use crate::entry::{Entry, TypeFields};
use crate::varres::{self, Values};
use crate::vars::VarKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretEnvVar {
    pub env_key: String,
    pub secret_value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DestructiveWarning {
    pub pattern: String,
    pub description: String,
}

impl fmt::Display for DestructiveWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.pattern, self.description)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunPlan {
    pub resolved_command: String,
    pub shell: String,
    pub cwd: Option<PathBuf>,
    pub secret_env_vars: Vec<SecretEnvVar>,
    pub destructive_warnings: Vec<DestructiveWarning>,
    pub requires_typed_run: bool,
    pub stdin_payload: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunResult {
    pub exit_code: i32,
    pub duration: Duration,
    pub killed_by_signal: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunPlanError {
    MissingCwd(PathBuf),
    MissingPlaceholderField(String),
    VarResolution(String),
}

impl fmt::Display for RunPlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RunPlanError::MissingCwd(p) => {
                write!(f, "working directory '{}' does not exist", p.display())
            }
            RunPlanError::MissingPlaceholderField(k) => {
                write!(
                    f,
                    "entry is missing required field '{k}' for action placeholder"
                )
            }
            RunPlanError::VarResolution(s) => write!(f, "variable resolution failed: {s}"),
        }
    }
}

impl std::error::Error for RunPlanError {}

struct BuiltinRule {
    pattern: &'static str,
    regex: &'static str,
    description: &'static str,
}

const BUILTIN_RULES: &[BuiltinRule] = &[
    BuiltinRule {
        pattern: "rm -r / -rf",
        regex: r"(?i)(?:^|[;&|]\s*|\b)rm\s+.*?(?:-(?:[a-zA-Z0-9]*r[a-zA-Z0-9]*)|--recursive)\b",
        description: "recursive file deletion",
    },
    BuiltinRule {
        pattern: "kubectl delete",
        regex: r"(?i)(?:^|[;&|]\s*|\b)kubectl\s+delete\b",
        description: "deleting Kubernetes resources",
    },
    BuiltinRule {
        pattern: "terraform destroy",
        regex: r"(?i)(?:^|[;&|]\s*|\b)terraform\s+destroy\b",
        description: "destroying Terraform infrastructure",
    },
    BuiltinRule {
        pattern: "terraform apply",
        regex: r"(?i)(?:^|[;&|]\s*|\b)terraform\s+apply\b",
        description: "applying Terraform state changes",
    },
    BuiltinRule {
        pattern: "helm uninstall",
        regex: r"(?i)(?:^|[;&|]\s*|\b)helm\s+uninstall\b",
        description: "uninstalling Helm release",
    },
    BuiltinRule {
        pattern: "aws delete/terminate/remove",
        regex: r"(?i)(?:^|[;&|]\s*|\b)aws\s+.*?\b(?:delete-[a-z0-9_-]+|terminate-[a-z0-9_-]+|remove-[a-z0-9_-]+)\b",
        description: "destructive AWS API operation",
    },
    BuiltinRule {
        pattern: "aws s3 rm --recursive",
        regex: r"(?i)(?:^|[;&|]\s*|\b)aws\s+s3\s+rm\s+.*?--recursive\b",
        description: "recursive S3 deletion",
    },
    BuiltinRule {
        pattern: "git push --force",
        regex: r"(?i)(?:^|[;&|]\s*|\b)git\s+push\s+.*?(?:--force|-f\b|\+[a-zA-Z0-9_./-]+:[a-zA-Z0-9_./-]+)",
        description: "force pushing to remote git repository",
    },
    BuiltinRule {
        pattern: "git reset --hard",
        regex: r"(?i)(?:^|[;&|]\s*|\b)git\s+reset\s+.*?--hard\b",
        description: "discarding uncommitted and working changes via hard reset",
    },
    BuiltinRule {
        pattern: "DROP",
        regex: r"(?i)\bDROP\s+(?:DATABASE|TABLE|SCHEMA)\b",
        description: "dropping database objects",
    },
    BuiltinRule {
        pattern: "TRUNCATE",
        regex: r"(?i)\bTRUNCATE\s+(?:TABLE\s+)?[a-zA-Z0-9_]+",
        description: "truncating database tables",
    },
    BuiltinRule {
        pattern: "DELETE FROM",
        regex: r"(?i)\bDELETE\s+FROM\b",
        description: "deleting rows from database tables",
    },
    BuiltinRule {
        pattern: "device redirection",
        regex: r"(?i)(?:(?:>|>>)\s*|of=)/dev/(?:sd[a-z][0-9]*|nvme[0-9]+n[0-9]+(?:p[0-9]+)?|hd[a-z][0-9]*|vd[a-z][0-9]*)\b",
        description: "raw block device overwrite or redirection",
    },
];

pub struct RunService;

impl RunService {
    /// Scans a command string against built-in and user destructive patterns.
    pub fn check_destructive(command: &str, extra_patterns: &[String]) -> Vec<DestructiveWarning> {
        let mut warnings = Vec::new();
        for rule in BUILTIN_RULES {
            if let Ok(re) = Regex::new(rule.regex)
                && re.is_match(command)
            {
                warnings.push(DestructiveWarning {
                    pattern: rule.pattern.to_string(),
                    description: rule.description.to_string(),
                });
            }
        }
        for pattern in extra_patterns {
            if let Ok(re) = Regex::new(pattern)
                && re.is_match(command)
            {
                warnings.push(DestructiveWarning {
                    pattern: pattern.clone(),
                    description: "matched custom destructive pattern from configuration"
                        .to_string(),
                });
            }
        }
        warnings
    }

    /// Builds a RunPlan for a `cmd` entry or a custom action,
    /// resolving environment secrets and detecting destructive commands.
    pub fn build_run_plan(
        entry: &Entry,
        values: &Values,
        custom_action: Option<&CustomActionConfig>,
        extra_destructive_patterns: &[String],
    ) -> Result<RunPlan, RunPlanError> {
        let shell = if custom_action.is_some() {
            "sh".to_string()
        } else if let TypeFields::Cmd { shell, .. } = entry.fields() {
            shell.as_str().to_string()
        } else {
            "sh".to_string()
        };

        let raw_cwd = if let TypeFields::Cmd { cwd: Some(c), .. } = entry.fields() {
            Some(c.clone())
        } else {
            None
        };

        let plan = varres::plan_vars(entry);

        // Validate values
        for p in &plan.vars {
            if let Some(v) = values.get(&p.var.name) {
                varres::validate(&p.var, v)
                    .map_err(|e| RunPlanError::VarResolution(e.to_string()))?;
            }
        }

        let mut secret_env_vars = Vec::new();
        let mut secret_names = std::collections::BTreeSet::new();

        for p in &plan.vars {
            let is_sec = p.var.kind == VarKind::Secret;
            if is_sec {
                let env_key = format!("KB_SECRET_{}", p.var.name.to_uppercase().replace('-', "_"));
                let secret_value = values
                    .get(&p.var.name)
                    .map(|v| v.reveal().to_string())
                    .unwrap_or_default();
                secret_names.insert(p.var.name.clone());
                secret_env_vars.push(SecretEnvVar {
                    env_key,
                    secret_value,
                });
            }
        }

        let resolved = crate::varres::resolve(&plan, values).map_err(|errs| {
            let msg = errs
                .into_iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            RunPlanError::VarResolution(msg)
        })?;

        // Resolve cwd if given
        let cwd = if raw_cwd.is_some() {
            let resolved_cwd = resolved
                .get(&crate::varres::Target::Cwd)
                .map(|o| o.text.clone())
                .or(raw_cwd)
                .unwrap_or_default();
            let home = std::env::var_os("HOME").map(PathBuf::from);
            let path =
                crate::root::expand_tilde(std::path::Path::new(&resolved_cwd), home.as_deref());
            if !path.exists() {
                return Err(RunPlanError::MissingCwd(path));
            }
            Some(path)
        } else {
            None
        };

        // Command template
        let mut stdin_payload = None;
        let resolved_command = if let Some(action) = custom_action {
            let cmd = crate::action::ActionService::resolve_custom_action_command(
                &action.command,
                entry,
                Some(values),
                action.input,
            )
            .map_err(|e| {
                if let Some(f) = e
                    .strip_prefix("missing field '")
                    .and_then(|s| s.strip_suffix('\''))
                {
                    RunPlanError::MissingPlaceholderField(f.to_string())
                } else {
                    RunPlanError::VarResolution(e)
                }
            })?;

            if action.input == crate::config::CustomActionInput::Stdin {
                let resolved_payload = resolved
                    .get(&crate::varres::Target::Payload)
                    .map(|o| o.text.clone())
                    .or_else(|| entry.payload().map(|p| p.text.clone()))
                    .unwrap_or_default();
                stdin_payload = Some(resolved_payload);
            }

            cmd
        } else {
            let mut cmd_plan = plan.clone();
            let mut values_for_cmd = values.clone();
            for p in &mut cmd_plan.vars {
                if p.var.kind == crate::vars::VarKind::Secret {
                    let env_key =
                        format!("KB_SECRET_{}", p.var.name.to_uppercase().replace('-', "_"));
                    p.var.escape = crate::vars::Escape::None;
                    values_for_cmd.insert(
                        p.var.name.clone(),
                        crate::varres::Value::new(format!("${env_key}"), false),
                    );
                }
            }
            let cmd_resolved =
                crate::varres::resolve(&cmd_plan, &values_for_cmd).map_err(|errs| {
                    let msg = errs
                        .into_iter()
                        .map(|e| e.to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    RunPlanError::VarResolution(msg)
                })?;
            let cmd_text = cmd_resolved
                .get(&crate::varres::Target::Payload)
                .map(|o| o.text.clone())
                .unwrap_or_default();
            cmd_text.trim_end_matches('\n').to_string()
        };

        let destructive_warnings =
            Self::check_destructive(&resolved_command, extra_destructive_patterns);
        let requires_typed_run = !destructive_warnings.is_empty();

        Ok(RunPlan {
            resolved_command,
            shell,
            cwd,
            secret_env_vars,
            destructive_warnings,
            requires_typed_run,
            stdin_payload,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::parse_entry;
    use crate::varres::Value;

    #[test]
    fn all_30_hostile_commands_are_detected_as_destructive() {
        let fixture_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/actions/hostile_commands.txt"
        );
        let content = std::fs::read_to_string(fixture_path).expect("hostile_commands.txt exists");
        let mut count = 0;
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let warnings = RunService::check_destructive(line, &[]);
            assert!(
                !warnings.is_empty(),
                "Hostile command failed destructive detection: {line}"
            );
            count += 1;
        }
        assert_eq!(count, 30, "Expected exactly 30 hostile commands tested");
    }

    #[test]
    fn non_destructive_commands_produce_no_warnings() {
        let safe = [
            "ls -la",
            "cat README.md",
            "git status",
            "git log -n 5",
            "git push origin feature/my-branch",
            "rm file.txt",
            "kubectl get pods -n default",
            "terraform plan",
            "helm status auth-service",
            "aws ec2 describe-instances",
            "aws s3 ls s3://prod-backups",
            "SELECT * FROM users WHERE active = 1;",
        ];
        for cmd in safe {
            let warnings = RunService::check_destructive(cmd, &[]);
            assert!(
                warnings.is_empty(),
                "Safe command unexpectedly flagged as destructive: {cmd} ({warnings:?})"
            );
        }
    }

    #[test]
    fn user_extra_patterns_are_additive() {
        let cmd = "custom-deploy --danger";
        assert!(RunService::check_destructive(cmd, &[]).is_empty());

        let extra = vec![r"\bcustom-deploy\s+--danger\b".to_string()];
        let warnings = RunService::check_destructive(cmd, &extra);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].pattern, extra[0]);
    }

    #[test]
    fn run_plan_isolates_secrets_into_env_vars() {
        let text = r#"---
kb_format: 1
type: cmd
title: Tail Logs With Token
shell: bash
vars:
  env: { kind: enum, options: [prod, stage] }
  token: { kind: secret }
---

```
curl -H "Authorization: Bearer {{token}}" https://logs.{{env}}.internal
```
"#;
        let outcome = parse_entry("cmd-token", "cmd-token.md", text.as_bytes().to_vec());
        let entry = outcome.entry.expect("entry parses");

        let mut values = Values::new();
        values.insert("env".into(), Value::plain("prod"));
        values.insert("token".into(), Value::secret("SUPER_SECRET_VALUE_12345"));

        let plan = RunService::build_run_plan(&entry, &values, None, &[]).expect("build run plan");

        // The secret value MUST NOT be anywhere in resolved_command
        assert!(!plan.resolved_command.contains("SUPER_SECRET_VALUE_12345"));
        assert!(plan.resolved_command.contains("$KB_SECRET_TOKEN"));
        assert!(plan.resolved_command.contains("https://logs.prod.internal"));

        // Secret MUST be in secret_env_vars
        assert_eq!(plan.secret_env_vars.len(), 1);
        assert_eq!(plan.secret_env_vars[0].env_key, "KB_SECRET_TOKEN");
        assert_eq!(
            plan.secret_env_vars[0].secret_value,
            "SUPER_SECRET_VALUE_12345"
        );

        assert_eq!(plan.shell, "bash");
        assert_eq!(plan.cwd, None);
        assert!(!plan.requires_typed_run);
    }

    #[test]
    fn run_plan_rejects_missing_cwd() {
        let text = r#"---
kb_format: 1
type: cmd
title: Command With Nonexistent Cwd
cwd: /nonexistent/directory/that/does/not/exist
---

```
pwd
```
"#;
        let outcome = parse_entry("cmd-cwd", "cmd-cwd.md", text.as_bytes().to_vec());
        let entry = outcome.entry.expect("entry parses");
        let values = Values::new();

        let err = RunService::build_run_plan(&entry, &values, None, &[]).unwrap_err();
        assert!(matches!(err, RunPlanError::MissingCwd(_)));
    }
}
