//! Region/endpoint resolution shared by every DashScope (阿里云百炼 Model
//! Studio) client. Only regions where this app's actual features — realtime
//! audio and CosyVoice cloning/design — are available are offered; Model
//! Studio has other regions (Tokyo, Frankfurt, Virginia) but those only
//! serve text/chat models. See https://help.aliyun.com/zh/model-studio/regions/.

use serde::Serialize;

pub const DEFAULT_REGION: &str = "cn-beijing";

pub const REGION_IDS: &[&str] = &["cn-beijing", "ap-southeast-1"];

/// A region as the Settings dropdown sees it. The label is resolved against
/// the current display language at call time, so it is an owned `String`
/// rather than part of a `const` table.
#[derive(Serialize, Clone)]
pub struct RegionOption {
    pub id: &'static str,
    pub label: String,
}

pub fn region_label(id: &str) -> String {
    match id {
        "ap-southeast-1" => crate::tr!("Singapore", "新加坡").to_string(),
        _ => crate::tr!("Chinese Mainland (Beijing)", "中国大陆（北京）").to_string(),
    }
}

pub fn regions() -> Vec<RegionOption> {
    REGION_IDS
        .iter()
        .map(|id| RegionOption {
            id,
            label: region_label(id),
        })
        .collect()
}

/// How much of an upstream response to keep when it ends up in an error
/// message. Enough for the API's own explanation and its request id, and
/// short of pasting a whole response into the UI — which would bury the part
/// that matters, and would be putting text this app did not write, from
/// whichever host actually answered, in front of the user verbatim.
const SNIPPET_CHARS: usize = 500;

/// Trims an upstream response body down to something that belongs in an
/// error message, saying so when it had to cut.
pub fn snippet(body: &str) -> String {
    let body = body.trim();
    let total = body.chars().count();
    if total <= SNIPPET_CHARS {
        return body.to_string();
    }
    let kept: String = body.chars().take(SNIPPET_CHARS).collect();
    crate::tr!(
        format!("{kept}… (truncated, {total} characters in total)"),
        format!("{kept}…（已截断，共 {total} 个字符）"),
    )
}

/// Workspace ids issued by Model Studio look like `llm-0a1b2c3d4e5f`. The
/// value is interpolated straight into a hostname by `host` below, so any
/// character that can terminate the authority component of a URL — `/`, `?`,
/// `#`, `@`, `:` — would send every request, `Authorization` header
/// included, to a host of the input's choosing instead of to Alibaba.
/// Allowing only the characters real ids are made of rules out that whole
/// class, rather than blacklisting separators one at a time and hoping the
/// list is complete.
pub fn is_valid_workspace_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Regions are a closed set — the two this app's features exist in — so an
/// unrecognized one is never something to pass through into a URL.
pub fn is_valid_region(id: &str) -> bool {
    REGION_IDS.contains(&id)
}

fn normalize(region: Option<&str>) -> &str {
    match region {
        Some(r) if is_valid_region(r) => r,
        _ => DEFAULT_REGION,
    }
}

/// The shared, non-workspace domain serving a region.
fn shared_host(region: &str) -> String {
    match region {
        "ap-southeast-1" => "dashscope-intl.aliyuncs.com".to_string(),
        _ => "dashscope.aliyuncs.com".to_string(),
    }
}

/// Host (no scheme, no path) for a given workspace/region combination. With
/// a workspace id this is the workspace-dedicated MaaS domain; without one
/// it falls back to the legacy shared domain for that region.
///
/// `app::commands::set_connection_settings` refuses anything that doesn't
/// validate before it can be stored, so a bad value only reaches here from a
/// settings row written by an older build or edited by hand. That case fails
/// closed — back to the shared Alibaba domain — because the alternative is
/// building a hostname out of it, which is the one thing that must not
/// happen: the API key travels on every one of these requests.
pub fn host(workspace_id: Option<&str>, region: Option<&str>) -> String {
    let region = normalize(region);
    match workspace_id.filter(|w| !w.is_empty()) {
        Some(w) if is_valid_workspace_id(w) => format!("{w}.{region}.maas.aliyuncs.com"),
        Some(w) => {
            tracing::warn!("ignoring malformed workspace id {w:?}; using the shared endpoint");
            shared_host(region)
        }
        None => shared_host(region),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_keeps_short_bodies_and_cuts_long_ones() {
        assert_eq!(
            snippet("  {\"error\":\"bad key\"}  "),
            "{\"error\":\"bad key\"}"
        );

        let long = "x".repeat(SNIPPET_CHARS * 3);
        let cut = snippet(&long);
        assert!(cut.chars().count() < long.chars().count());
        assert!(cut.starts_with(&"x".repeat(SNIPPET_CHARS)));
        assert!(!cut.contains(&"x".repeat(SNIPPET_CHARS + 1)));
    }

    #[test]
    fn accepts_the_shape_real_workspace_ids_have() {
        assert!(is_valid_workspace_id("llm-0a1b2c3d4e5f"));
        assert!(is_valid_workspace_id("ws_123"));
        assert!(!is_valid_workspace_id(""));
        assert!(!is_valid_workspace_id(&"a".repeat(65)));
    }

    /// Every one of these, interpolated into `https://{host}/...`, would
    /// have moved the request off Alibaba's domain and taken the bearer
    /// token with it.
    #[test]
    fn rejects_anything_that_could_terminate_the_host() {
        for hostile in [
            "evil.com/#",
            "evil.com/",
            "attacker.io/x?",
            "x@evil.com",
            "evil.com:8443",
            "evil.com\\x",
            "a b",
        ] {
            assert!(
                !is_valid_workspace_id(hostile),
                "{hostile:?} must not be accepted as a workspace id"
            );
        }
    }

    #[test]
    fn a_rejected_workspace_id_never_reaches_the_hostname() {
        assert_eq!(
            host(Some("evil.com/#"), Some("cn-beijing")),
            "dashscope.aliyuncs.com",
            "a malformed id must fall back to the shared domain, not be interpolated"
        );
        assert_eq!(
            host(Some("evil.com/#"), Some("ap-southeast-1")),
            "dashscope-intl.aliyuncs.com"
        );
    }

    #[test]
    fn an_unknown_region_falls_back_to_the_default() {
        assert_eq!(
            host(Some("llm-abc"), Some("evil.com/#")),
            format!("llm-abc.{DEFAULT_REGION}.maas.aliyuncs.com")
        );
        assert_eq!(host(None, Some("us-east-1")), "dashscope.aliyuncs.com");
    }

    #[test]
    fn still_builds_the_endpoints_it_is_supposed_to() {
        assert_eq!(host(None, None), "dashscope.aliyuncs.com");
        assert_eq!(host(None, Some("")), "dashscope.aliyuncs.com");
        assert_eq!(
            host(None, Some("ap-southeast-1")),
            "dashscope-intl.aliyuncs.com"
        );
        assert_eq!(
            host(Some("llm-abc"), Some("ap-southeast-1")),
            "llm-abc.ap-southeast-1.maas.aliyuncs.com"
        );
    }
}
