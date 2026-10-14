//! Console deep link builders for Sumo Logic and AWS CloudWatch Logs Insights.

use crate::config::Settings;
use crate::entry::{Entry, TypeFields};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MAX_CONSOLE_URL_LENGTH: usize = 8000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsoleTarget {
    SumoLogic {
        base_url: String,
    },
    CloudWatch {
        region: String,
        log_groups: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleLink {
    pub target: ConsoleTarget,
    pub url: String,
    pub query_included: bool,
    pub character_count: usize,
}

pub trait EnvProvider {
    fn get_var(&self, key: &str) -> Option<String>;
}

pub struct OsEnv;
impl EnvProvider for OsEnv {
    fn get_var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}

impl<F> EnvProvider for F
where
    F: Fn(&str) -> Option<String>,
{
    fn get_var(&self, key: &str) -> Option<String> {
        self(key)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingRegionError;

impl std::fmt::Display for MissingRegionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "no AWS region configured. Set `region:` on the entry, configure `aws.default_region`, or set `AWS_REGION`"
        )
    }
}
impl std::error::Error for MissingRegionError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsoleLinkError {
    MissingSumoBaseUrl,
    InvalidRange { field: String, value: String },
    MissingRegion(MissingRegionError),
}

impl std::fmt::Display for ConsoleLinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSumoBaseUrl => write!(
                f,
                "Sumo deployment URL is not configured. Set `sumo.base_url` in config or via `kb config set sumo.base_url <url>`"
            ),
            Self::InvalidRange { field, value } => write!(
                f,
                "field `{field}`: invalid range '{value}'. Accepted forms: -15m, -2h, -7d (relative to now)"
            ),
            Self::MissingRegion(e) => write!(f, "{e}"),
        }
    }
}
impl std::error::Error for ConsoleLinkError {}

impl From<MissingRegionError> for ConsoleLinkError {
    fn from(e: MissingRegionError) -> Self {
        Self::MissingRegion(e)
    }
}

/// Parses relative range token (e.g. `-15m`, `-2h`, `-7d`) into (seconds, milliseconds).
pub fn parse_range(range_str: &str) -> Result<(u64, u64), ConsoleLinkError> {
    let s = range_str.trim();
    if !s.starts_with('-') {
        return Err(ConsoleLinkError::InvalidRange {
            field: "range".into(),
            value: range_str.to_string(),
        });
    }
    let rest = &s[1..];
    if rest.len() < 2 {
        return Err(ConsoleLinkError::InvalidRange {
            field: "range".into(),
            value: range_str.to_string(),
        });
    }
    let (num_part, unit) = rest.split_at(rest.len() - 1);
    let n: u64 = num_part
        .parse()
        .map_err(|_| ConsoleLinkError::InvalidRange {
            field: "range".into(),
            value: range_str.to_string(),
        })?;
    if n == 0 {
        return Err(ConsoleLinkError::InvalidRange {
            field: "range".into(),
            value: range_str.to_string(),
        });
    }
    let secs = match unit {
        "m" => n.checked_mul(60),
        "h" => n.checked_mul(3600),
        "d" => n.checked_mul(86400),
        _ => {
            return Err(ConsoleLinkError::InvalidRange {
                field: "range".into(),
                value: range_str.to_string(),
            });
        }
    }
    .ok_or_else(|| ConsoleLinkError::InvalidRange {
        field: "range".into(),
        value: range_str.to_string(),
    })?;

    let ms = secs
        .checked_mul(1000)
        .ok_or_else(|| ConsoleLinkError::InvalidRange {
            field: "range".into(),
            value: range_str.to_string(),
        })?;

    Ok((secs, ms))
}

/// Percent-encodes a string according to RFC 3986 for URL components.
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3 / 2);
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            other => {
                use std::fmt::Write;
                let _ = write!(out, "%{:02X}", other);
            }
        }
    }
    out
}

/// Encodes query for CloudWatch Logs Insights console using AWS rison-like format (`*<HEX>`).
pub fn encode_cw_query(query: &str) -> String {
    let mut out = String::with_capacity(query.len() * 2);
    for &b in query.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                out.push(b as char);
            }
            other => {
                use std::fmt::Write;
                let _ = write!(out, "*{:02X}", other);
            }
        }
    }
    out
}

pub struct ConsoleService;

impl ConsoleService {
    /// Constructs a Sumo Logic search deep link.
    pub fn build_sumo_link(
        base_url: &str,
        resolved_query: &str,
        resolved_range: Option<&str>,
    ) -> Result<ConsoleLink, ConsoleLinkError> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Self::build_sumo_link_with_now(base_url, resolved_query, resolved_range, now_ms)
    }

    /// Constructs a Sumo Logic search deep link given a specific reference timestamp in epoch milliseconds.
    pub fn build_sumo_link_with_now(
        base_url: &str,
        resolved_query: &str,
        resolved_range: Option<&str>,
        now_epoch_ms: u64,
    ) -> Result<ConsoleLink, ConsoleLinkError> {
        let base = base_url.trim().trim_end_matches('/');
        if base.is_empty() {
            return Err(ConsoleLinkError::MissingSumoBaseUrl);
        }
        let (_, range_ms) = match resolved_range {
            Some(r) => parse_range(r)?,
            None => (900, 900_000),
        };
        let from_epoch_ms = now_epoch_ms.saturating_sub(range_ms);
        let to_epoch_ms = now_epoch_ms;
        let encoded_query = percent_encode(resolved_query);
        let full_url = format!(
            "{base}/ui/index.html#section/search/{from_epoch_ms},{to_epoch_ms},{encoded_query}"
        );
        let (url, query_included) = if full_url.len() > MAX_CONSOLE_URL_LENGTH {
            let truncated_url =
                format!("{base}/ui/index.html#section/search/{from_epoch_ms},{to_epoch_ms}");
            (truncated_url, false)
        } else {
            (full_url, true)
        };
        let character_count = url.len();
        Ok(ConsoleLink {
            target: ConsoleTarget::SumoLogic {
                base_url: base.to_string(),
            },
            url,
            query_included,
            character_count,
        })
    }

    /// Constructs an AWS CloudWatch Logs Insights console deep link.
    pub fn build_cw_link(
        region: &str,
        log_groups: &[String],
        resolved_query: &str,
        resolved_range: Option<&str>,
    ) -> Result<ConsoleLink, ConsoleLinkError> {
        let trimmed_region = region.trim();
        if trimmed_region.is_empty() {
            return Err(ConsoleLinkError::MissingRegion(MissingRegionError));
        }
        let (range_secs, _) = match resolved_range {
            Some(r) => parse_range(r)?,
            None => (900, 900_000),
        };
        let range_seconds = format!("-{range_secs}");
        let mut source_str = String::new();
        for lg in log_groups {
            source_str.push_str(&format!("~'{lg}'"));
        }
        let encoded_query = encode_cw_query(resolved_query);
        let full_url = format!(
            "https://{trimmed_region}.console.aws.amazon.com/cloudwatch/home?region={trimmed_region}#logsV2:logs-insights$3FqueryDetail$3D~(end~0~start~'{range_seconds}'~timeType~'RELATIVE'~unit~'seconds'~editorString~'{encoded_query}'~source~({source_str}))"
        );
        let (url, query_included) = if full_url.len() > MAX_CONSOLE_URL_LENGTH {
            let truncated_url = format!(
                "https://{trimmed_region}.console.aws.amazon.com/cloudwatch/home?region={trimmed_region}#logsV2:logs-insights$3FqueryDetail$3D~(end~0~start~'{range_seconds}'~timeType~'RELATIVE'~unit~'seconds'~editorString~''~source~({source_str}))"
            );
            (truncated_url, false)
        } else {
            (full_url, true)
        };
        let character_count = url.len();
        Ok(ConsoleLink {
            target: ConsoleTarget::CloudWatch {
                region: trimmed_region.to_string(),
                log_groups: log_groups.to_vec(),
            },
            url,
            query_included,
            character_count,
        })
    }

    /// Resolves AWS region using the precedence chain:
    /// entry.region -> config.aws.default_region -> AWS_REGION -> AWS_DEFAULT_REGION
    pub fn resolve_aws_region(
        entry: &Entry,
        config: &Settings,
        env_provider: &impl EnvProvider,
    ) -> Result<String, MissingRegionError> {
        if let TypeFields::Cw {
            region: Some(r), ..
        } = entry.fields()
        {
            let trimmed = r.trim();
            if !trimmed.is_empty() {
                return Ok(trimmed.to_string());
            }
        }
        if let Some(r) = config.aws_default_region() {
            let trimmed = r.trim();
            if !trimmed.is_empty() {
                return Ok(trimmed.to_string());
            }
        }
        if let Some(r) = env_provider.get_var("AWS_REGION") {
            let trimmed = r.trim();
            if !trimmed.is_empty() {
                return Ok(trimmed.to_string());
            }
        }
        if let Some(r) = env_provider.get_var("AWS_DEFAULT_REGION") {
            let trimmed = r.trim();
            if !trimmed.is_empty() {
                return Ok(trimmed.to_string());
            }
        }
        Err(MissingRegionError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::parse_entry;

    #[test]
    fn sumo_link_basic_construction_and_percent_encoding() {
        let base = "https://service.us2.sumologic.com";
        let query = "_sourceCategory=prod/authorizer | count by status";
        let now = 1_700_000_000_000u64; // arbitrary epoch ms
        let link =
            ConsoleService::build_sumo_link_with_now(base, query, Some("-15m"), now).unwrap();

        assert_eq!(link.character_count, link.url.len());
        assert!(link.query_included);
        // -15m is 900,000 ms -> 1,699,999,100,000
        let expected_from = now - 900_000;
        let expected_url = format!(
            "https://service.us2.sumologic.com/ui/index.html#section/search/{expected_from},{now},_sourceCategory%3Dprod%2Fauthorizer%20%7C%20count%20by%20status"
        );
        assert_eq!(link.url, expected_url);
    }

    #[test]
    fn sumo_link_relative_range_variations() {
        let base = "https://service.us2.sumologic.com/";
        let query = "error";
        let now = 1_700_000_000_000u64;

        // -2h: 7,200,000 ms
        let link_2h =
            ConsoleService::build_sumo_link_with_now(base, query, Some("-2h"), now).unwrap();
        assert_eq!(
            link_2h.url,
            format!(
                "https://service.us2.sumologic.com/ui/index.html#section/search/{},{now},error",
                now - 7_200_000
            )
        );

        // -7d: 604,800,000 ms
        let link_7d =
            ConsoleService::build_sumo_link_with_now(base, query, Some("-7d"), now).unwrap();
        assert_eq!(
            link_7d.url,
            format!(
                "https://service.us2.sumologic.com/ui/index.html#section/search/{},{now},error",
                now - 604_800_000
            )
        );
    }

    #[test]
    fn sumo_link_invalid_range_rejected() {
        let base = "https://service.us2.sumologic.com";
        let err =
            ConsoleService::build_sumo_link_with_now(base, "q", Some("15m"), 1000).unwrap_err();
        assert!(matches!(err, ConsoleLinkError::InvalidRange { .. }));
        assert!(err.to_string().contains("Accepted forms: -15m, -2h, -7d"));
    }

    #[test]
    fn sumo_link_missing_base_url_error() {
        let err = ConsoleService::build_sumo_link("", "q", Some("-15m")).unwrap_err();
        assert_eq!(err, ConsoleLinkError::MissingSumoBaseUrl);
    }

    #[test]
    fn sumo_link_omits_query_when_url_exceeds_8000_chars() {
        let base = "https://service.us2.sumologic.com";
        let huge_query = "x".repeat(8100);
        let now = 1_700_000_000_000u64;
        let link =
            ConsoleService::build_sumo_link_with_now(base, &huge_query, Some("-15m"), now).unwrap();

        assert!(!link.query_included);
        assert_eq!(link.character_count, link.url.len());
        assert!(link.url.len() < MAX_CONSOLE_URL_LENGTH);
        let expected_url = format!(
            "https://service.us2.sumologic.com/ui/index.html#section/search/{},{}",
            now - 900_000,
            now
        );
        assert_eq!(link.url, expected_url);
    }

    #[test]
    fn cw_link_basic_construction_and_aws_encoding() {
        let region = "us-east-1";
        let log_groups = vec![
            "/aws/lambda/prod-authorizer".to_string(),
            "/aws/lambda/shared-auth".to_string(),
        ];
        let query = "filter @duration > 200\n| stats count() by bin(5m)";
        let link = ConsoleService::build_cw_link(region, &log_groups, query, Some("-30m")).unwrap();

        assert!(link.query_included);
        assert_eq!(link.character_count, link.url.len());
        // -30m is 1800s
        assert!(link.url.starts_with("https://us-east-1.console.aws.amazon.com/cloudwatch/home?region=us-east-1#logsV2:logs-insights$3FqueryDetail$3D~("));
        assert!(link.url.contains("start~'-1800'"));
        assert!(link.url.contains("timeType~'RELATIVE'~unit~'seconds'"));
        assert!(
            link.url
                .contains("source~(~'/aws/lambda/prod-authorizer'~'/aws/lambda/shared-auth')")
        );
        // Query should encode spaces as *20, newlines as *0A, @ as *40, > as *3E, | as *7C, ( as *28, ) as *29
        assert!(link.url.contains("editorString~'filter*20*40duration*20*3E*20200*0A*7C*20stats*20count*28*29*20by*20bin*285m*29'"));
    }

    #[test]
    fn cw_link_empty_log_groups_allowed() {
        let link =
            ConsoleService::build_cw_link("eu-west-1", &[], "fields @message", None).unwrap();
        assert!(link.url.contains("source~()"));
        assert!(link.url.contains("start~'-900'")); // default -15m
    }

    #[test]
    fn cw_link_omits_query_when_exceeding_8000_chars() {
        let huge_query = "fields @message | filter ".to_string() + &"a".repeat(8000);
        let link = ConsoleService::build_cw_link(
            "us-east-1",
            &["/aws/lambda/foo".into()],
            &huge_query,
            Some("-1h"),
        )
        .unwrap();
        assert!(!link.query_included);
        assert!(link.url.contains("editorString~''"));
        assert!(link.url.len() < MAX_CONSOLE_URL_LENGTH);
    }

    #[test]
    fn cw_region_precedence_resolution() {
        let empty_settings =
            crate::config::load(&[], &|_| None, &crate::config::Flags::default()).settings;

        // 1. entry.region wins
        let raw_entry = "---\nkb_format: 1\ntype: cw\ntitle: Test CW\nregion: us-west-2\n---\n```\nfields @message\n```\n";
        let entry = parse_entry("test", "test.md", raw_entry.as_bytes().to_vec())
            .entry
            .unwrap();
        let env = |k: &str| {
            if k == "AWS_REGION" {
                Some("us-east-2".into())
            } else {
                None
            }
        };
        let reg = ConsoleService::resolve_aws_region(&entry, &empty_settings, &env).unwrap();
        assert_eq!(reg, "us-west-2");

        // 2. config.aws.default_region wins over env
        let raw_no_reg =
            "---\nkb_format: 1\ntype: cw\ntitle: Test CW\n---\n```\nfields @message\n```\n";
        let entry_no_reg = parse_entry("test2", "test2.md", raw_no_reg.as_bytes().to_vec())
            .entry
            .unwrap();
        let env_cfg = |k: &str| {
            if k == "KB_AWS_DEFAULT_REGION" {
                Some("eu-central-1".into())
            } else {
                None
            }
        };
        let settings_with_reg =
            crate::config::load(&[], &env_cfg, &crate::config::Flags::default()).settings;
        let reg2 =
            ConsoleService::resolve_aws_region(&entry_no_reg, &settings_with_reg, &env).unwrap();
        assert_eq!(reg2, "eu-central-1");

        // 3. AWS_REGION wins over AWS_DEFAULT_REGION
        let env_both = |k: &str| match k {
            "AWS_REGION" => Some("ap-southeast-1".into()),
            "AWS_DEFAULT_REGION" => Some("ap-northeast-1".into()),
            _ => None,
        };
        let reg3 =
            ConsoleService::resolve_aws_region(&entry_no_reg, &empty_settings, &env_both).unwrap();
        assert_eq!(reg3, "ap-southeast-1");

        // 4. AWS_DEFAULT_REGION used if AWS_REGION is empty
        let env_def = |k: &str| {
            if k == "AWS_DEFAULT_REGION" {
                Some("sa-east-1".into())
            } else {
                None
            }
        };
        let reg4 =
            ConsoleService::resolve_aws_region(&entry_no_reg, &empty_settings, &env_def).unwrap();
        assert_eq!(reg4, "sa-east-1");

        // 5. MissingRegionError if none set
        let env_none = |_: &str| None;
        let err = ConsoleService::resolve_aws_region(&entry_no_reg, &empty_settings, &env_none)
            .unwrap_err();
        assert_eq!(err, MissingRegionError);
    }
}
