//! Secret detection (FR-013, constitution VI). Patterns are in research.md R4.

use std::sync::LazyLock;

use regex::Regex;

use crate::lint::RuleCode;

/// A secret pattern. `group` is the capture group holding the secret itself.
struct Pattern {
    code: RuleCode,
    re: Regex,
    group: usize,
    /// Only report values with at least this much entropy (bits per character).
    min_entropy: Option<f64>,
}

/// Minimum entropy for the high-entropy assignment rule (research R4).
pub const MIN_ENTROPY: f64 = 3.5;

/// In priority order: when matches overlap, the earlier pattern wins.
static PATTERNS: LazyLock<Vec<Pattern>> = LazyLock::new(|| {
    let p = |code, re: &str, group, min_entropy| Pattern {
        code,
        re: Regex::new(re).expect("valid secret pattern"),
        group,
        min_entropy,
    };
    vec![
        p(
            RuleCode::SecretPrivateKey,
            r"-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY(?: BLOCK)?-----",
            0,
            None,
        ),
        p(
            RuleCode::SecretAwsAccessKey,
            r"\b(?:AKIA|ASIA|ABIA|ACCA)[A-Z0-9]{16}\b",
            0,
            None,
        ),
        p(
            RuleCode::SecretAwsSecretKey,
            r#"(?i)aws_?secret_?(?:access_?)?key["']?\s*[:=]\s*["']?([A-Za-z0-9/+=]{40})"#,
            1,
            None,
        ),
        p(
            RuleCode::SecretJwt,
            r"\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}",
            0,
            None,
        ),
        p(
            RuleCode::SecretBearerToken,
            r"(?i)\bbearer\s+([A-Za-z0-9._~+/-]{20,}=*)",
            1,
            None,
        ),
        p(
            RuleCode::SecretHighEntropy,
            r#"(?i)\b\w*(?:password|passwd|pwd|token|secret|api_?key|access_?key)\w*["']?\s*[:=]\s*["']?([^\s"'{}$]{12,})"#,
            1,
            Some(MIN_ENTROPY),
        ),
    ]
});

/// The marker that silences secret findings on its line.
pub const ALLOW_MARKER: &str = "kb:allow-secret";

/// A likely secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretHit {
    pub code: RuleCode,
    /// Byte offset of the match in the scanned text.
    pub offset: usize,
    /// Never contains more than the first 4 characters of the secret.
    pub message: String,
    pub fix: String,
}

/// The result of scanning one text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SecretScan {
    pub hits: Vec<SecretHit>,
    /// Matches on lines ending in `kb:allow-secret`: (byte offset, code).
    pub suppressed: Vec<(usize, RuleCode)>,
}

/// Scans `text` for likely secrets.
pub fn scan_secrets(text: &str) -> SecretScan {
    let mut accepted: Vec<(std::ops::Range<usize>, RuleCode)> = Vec::new();
    for pattern in PATTERNS.iter() {
        for caps in pattern.re.captures_iter(text) {
            let Some(m) = caps.get(pattern.group) else {
                continue;
            };
            if pattern
                .min_entropy
                .is_some_and(|min| entropy(m.as_str()) < min)
            {
                continue;
            }
            let range = m.range();
            if accepted
                .iter()
                .any(|(r, _)| r.start < range.end && range.start < r.end)
            {
                continue;
            }
            accepted.push((range, pattern.code));
        }
    }
    accepted.sort_by_key(|(r, _)| r.start);
    let mut scan = SecretScan::default();
    for (range, code) in accepted {
        let line_end = text[range.start..]
            .find('\n')
            .map_or(text.len(), |i| range.start + i);
        let line_start = text[..range.start].rfind('\n').map_or(0, |i| i + 1);
        if text[line_start..line_end]
            .trim_end()
            .ends_with(ALLOW_MARKER)
        {
            scan.suppressed.push((range.start, code));
            continue;
        }
        let shown = redact(&text[range.clone()]);
        let (message, fix) = match code {
            RuleCode::SecretPrivateKey => (
                "a private key block is in the entry".to_string(),
                "remove the key from the entry; keep keys in a file or a secret manager"
                    .to_string(),
            ),
            _ => {
                let what = match code {
                    RuleCode::SecretAwsAccessKey => "AWS access key ID",
                    RuleCode::SecretAwsSecretKey => "AWS secret access key",
                    RuleCode::SecretBearerToken => "bearer token",
                    RuleCode::SecretJwt => "JSON Web Token",
                    _ => "secret (high-entropy value)",
                };
                (
                    format!("possible {what} `{shown}`"),
                    "use a `secret` variable read from `env` instead (for example `{{token}}` with \
                     `token: { kind: secret, env: API_TOKEN }`), or end the line with `kb:allow-secret` if \
                     it is not a secret"
                        .to_string(),
                )
            }
        };
        scan.hits.push(SecretHit {
            code,
            offset: range.start,
            message,
            fix,
        });
    }
    scan
}

/// The first 4 characters of `secret` followed by `…`.
pub fn redact(secret: &str) -> String {
    let mut shown: String = secret.chars().take(4).collect();
    shown.push('…');
    shown
}

/// Shannon entropy in bits per character.
pub fn entropy(s: &str) -> f64 {
    let mut counts = std::collections::HashMap::new();
    let mut total = 0usize;
    for c in s.chars() {
        *counts.entry(c).or_insert(0usize) += 1;
        total += 1;
    }
    if total == 0 {
        return 0.0;
    }
    #[allow(clippy::cast_precision_loss)] // counts are far below 2^52
    let n = total as f64;
    counts
        .values()
        .map(|&count| {
            #[allow(clippy::cast_precision_loss)]
            let p = count as f64 / n;
            -p * p.log2()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    const AWS_ID: &str = "AKIAIOSFODNN7EXAMPLE";
    const AWS_SECRET: &str = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
    const JWT: &str = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ0ZXN0In0.c2lnbmF0dXJlZmFrZQ";
    const BEARER: &str = "abcdefghijklmnopqrstuvwxyz012345";

    fn codes(text: &str) -> Vec<RuleCode> {
        scan_secrets(text).hits.iter().map(|h| h.code).collect()
    }

    #[test]
    fn each_pattern() {
        assert_eq!(
            codes(&format!("key {AWS_ID} here")),
            vec![RuleCode::SecretAwsAccessKey]
        );
        assert_eq!(
            codes(&format!("aws_secret_access_key = {AWS_SECRET}")),
            vec![RuleCode::SecretAwsSecretKey]
        );
        assert_eq!(
            codes(&format!("AWS_SECRET_KEY: \"{AWS_SECRET}\"")),
            vec![RuleCode::SecretAwsSecretKey]
        );
        assert_eq!(
            codes(&format!("-H 'Authorization: Bearer {BEARER}'")),
            vec![RuleCode::SecretBearerToken]
        );
        assert_eq!(codes(&format!("token {JWT}")), vec![RuleCode::SecretJwt]);
        assert_eq!(
            codes(&format!("Authorization: Bearer {JWT}")),
            vec![RuleCode::SecretJwt]
        );
        for header in [
            "-----BEGIN PRIVATE KEY-----",
            "-----BEGIN RSA PRIVATE KEY-----",
            "-----BEGIN OPENSSH PRIVATE KEY-----",
        ] {
            assert_eq!(
                codes(&format!("{header}\nMIIabc\n-----END PRIVATE KEY-----")),
                vec![RuleCode::SecretPrivateKey]
            );
        }
        assert_eq!(
            codes("api_key = \"Zx9Qp2Lm7Rt4Vb8N\""),
            vec![RuleCode::SecretHighEntropy]
        );
        assert_eq!(
            codes("DB_PASSWORD=Zx9Qp2Lm7Rt4Vb8N"),
            vec![RuleCode::SecretHighEntropy]
        );
    }

    #[test]
    fn near_misses() {
        for text in [
            "AKIA1234",
            "Bearer {{token}}",
            "Bearer $TOKEN",
            "Authorization: Bearer short",
            "password: {{password}}",
            "token=${API_TOKEN}",
            "token=$TOKEN_FROM_ENV",
            "password: changeme_please",
            "api_key: short",
            "WHERE sha = 'a94a8fe5ccb19ba61c4c0873d391e987982fbbd3'",
            "-----BEGIN PUBLIC KEY-----",
            "eyJ.not.a.jwt",
        ] {
            assert_eq!(codes(text), vec![], "{text}");
        }
    }

    #[test]
    fn messages_never_contain_the_secret() {
        for text in [
            AWS_ID.to_string(),
            format!("aws_secret_access_key = {AWS_SECRET}"),
            format!("Bearer {BEARER}"),
            JWT.to_string(),
            "api_key = Zx9Qp2Lm7Rt4Vb8N".to_string(),
        ] {
            let scan = scan_secrets(&text);
            assert_eq!(scan.hits.len(), 1, "{text}");
            let hit = &scan.hits[0];
            for secret in [AWS_ID, AWS_SECRET, BEARER, JWT, "Zx9Qp2Lm7Rt4Vb8N"] {
                assert!(!hit.message.contains(&secret[4..]), "{}", hit.message);
                assert!(!hit.fix.contains(&secret[4..]), "{}", hit.fix);
            }
            assert!(hit.message.contains('…'), "{}", hit.message);
        }
    }

    #[test]
    fn offsets() {
        let scan = scan_secrets(&format!("line one\nkey {AWS_ID}\n"));
        assert_eq!(scan.hits[0].offset, 13);
        let scan = scan_secrets("x\n-----BEGIN PRIVATE KEY-----\n");
        assert_eq!(scan.hits[0].offset, 2);
    }

    #[test]
    fn suppression() {
        let text = format!(
            "curl -H 'Bearer {BEARER}' -- kb:allow-secret\nkey {AWS_ID} # kb:allow-secret  \nkey {AWS_ID}\n"
        );
        let scan = scan_secrets(&text);
        assert_eq!(scan.hits.len(), 1);
        assert_eq!(scan.hits[0].code, RuleCode::SecretAwsAccessKey);
        assert_eq!(
            scan.suppressed.iter().map(|(_, c)| *c).collect::<Vec<_>>(),
            vec![RuleCode::SecretBearerToken, RuleCode::SecretAwsAccessKey]
        );
        let mid = format!("kb:allow-secret {AWS_ID}");
        assert_eq!(codes(&mid), vec![RuleCode::SecretAwsAccessKey]);
    }

    #[test]
    fn crlf_suppression() {
        let scan = scan_secrets(&format!("key {AWS_ID} # kb:allow-secret\r\n"));
        assert!(scan.hits.is_empty());
        assert_eq!(scan.suppressed.len(), 1);
    }

    #[test]
    fn redaction_and_entropy() {
        assert_eq!(redact("AKIAIOSFODNN7EXAMPLE"), "AKIA…");
        assert_eq!(redact("ab"), "ab…");
        assert!((entropy("aaaa") - 0.0).abs() < 1e-9);
        assert!((entropy("abcd") - 2.0).abs() < 1e-9);
        assert!(entropy("Zx9Qp2Lm7Rt4Vb8N") >= 3.5);
        assert!(entropy("changeme_please") < 3.5);
    }
}
