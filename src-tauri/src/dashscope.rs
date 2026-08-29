//! Region/endpoint resolution shared by every DashScope (阿里云百炼 Model
//! Studio) client. Only regions where this app's actual features — realtime
//! audio and CosyVoice cloning/design — are available are offered; Model
//! Studio has other regions (Tokyo, Frankfurt, Virginia) but those only
//! serve text/chat models. See https://help.aliyun.com/zh/model-studio/regions/.

use serde::Serialize;

pub const DEFAULT_REGION: &str = "cn-beijing";

#[derive(Serialize, Clone, Copy)]
pub struct RegionOption {
    pub id: &'static str,
    pub label: &'static str,
}

pub const REGIONS: &[RegionOption] = &[
    RegionOption {
        id: "cn-beijing",
        label: "中国大陆（北京）",
    },
    RegionOption {
        id: "ap-southeast-1",
        label: "新加坡",
    },
];

fn normalize(region: Option<&str>) -> &str {
    match region {
        Some(r) if !r.is_empty() => r,
        _ => DEFAULT_REGION,
    }
}

/// Host (no scheme, no path) for a given workspace/region combination. With
/// a workspace id this is the workspace-dedicated MaaS domain; without one
/// it falls back to the legacy shared domain for that region.
pub fn host(workspace_id: Option<&str>, region: Option<&str>) -> String {
    let region = normalize(region);
    match workspace_id.filter(|w| !w.is_empty()) {
        Some(w) => format!("{w}.{region}.maas.aliyuncs.com"),
        None => match region {
            "ap-southeast-1" => "dashscope-intl.aliyuncs.com".to_string(),
            _ => "dashscope.aliyuncs.com".to_string(),
        },
    }
}
