use regex::Regex;
use serde::Serialize;

use crate::models::{PlatformSpec, PlatformValue};

pub fn default_specs() -> Vec<PlatformSpec> {
    vec![
        spec("https://live.douyin.com/", "DouYinLive", "抖音直播地", "抖音直播", 0),
        spec(
            "https://v.douyin.com/",
            "DouYinAppShareLive",
            "抖音app分享地址",
            "抖音直播",
            0,
        ),
        spec(
            "https://www.douyin.com/user/",
            "DouYinHomeLive",
            "抖音个人主页分享地址",
            "抖音直播",
            0,
        ),
        spec("https://www.tiktok.com/", "TiktokLive", "tiktok地址", "tiktok直播", 1),
        spec(
            "https://live.kuaishou.com/",
            "KuaiShouLive",
            "快手直播地址",
            "快手直播",
            2,
        ),
        spec(
            "https://www.redelight.cn/",
            "RedeLightLive",
            "小红书直播地址",
            "小红书直播",
            4,
        ),
    ]
}

fn spec(url: &str, bean: &str, platform: &str, name: &str, value: i32) -> PlatformSpec {
    PlatformSpec {
        url_prefix: url.into(),
        bean: bean.into(),
        platform: platform.into(),
        platform_name: name.into(),
        platform_value: value,
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ParsedLiveUrl {
    pub spec: PlatformSpec,
    pub source_url: String,
    pub web_rid: String,
    pub sec_uid: String,
    pub username: String,
}

pub fn match_url(url: &str) -> Result<ParsedLiveUrl, String> {
    let specs = default_specs();
    let mut normalized = url.trim().to_string();
    if !normalized.starts_with("http") {
        normalized = format!("https://{normalized}");
    }
    let mut hit = specs.iter().find(|s| {
        normalized.starts_with(&s.url_prefix) || normalized.contains(s.url_prefix.trim_end_matches('/'))
    });
    if hit.is_none() {
        let host = normalized
            .split("://")
            .nth(1)
            .unwrap_or("")
            .split('/')
            .next()
            .unwrap_or("")
            .to_lowercase();
        hit = if host.contains("douyin.com") {
            specs.get(0)
        } else if host.contains("tiktok.com") {
            specs.get(3)
        } else if host.contains("kuaishou.com") {
            specs.get(4)
        } else if host.contains("xiaohongshu.com")
            || host.contains("redelight.cn")
            || host.contains("xhslink.com")
        {
            specs.get(5)
        } else {
            None
        };
    }
    let spec = hit.cloned().ok_or_else(|| format!("unsupported live url: {url}"))?;
    let mut out = ParsedLiveUrl {
        spec: spec.clone(),
        source_url: normalized.clone(),
        web_rid: String::new(),
        sec_uid: String::new(),
        username: String::new(),
    };
    match spec.platform_value {
        x if x == PlatformValue::Douyin as i32 => {
            if let Some(c) = Regex::new(r"live\.douyin\.com/(\d+)")
                .unwrap()
                .captures(&normalized)
            {
                out.web_rid = c[1].to_string();
            }
            if let Some(c) = Regex::new(r"douyin\.com/user/([^/?#]+)")
                .unwrap()
                .captures(&normalized)
            {
                out.sec_uid = c[1].to_string();
            }
        }
        x if x == PlatformValue::Kuaishou as i32 => {
            if let Some(c) = Regex::new(r"live\.kuaishou\.com/u/([^/?#]+)")
                .unwrap()
                .captures(&normalized)
            {
                out.username = c[1].to_string();
            }
        }
        x if x == PlatformValue::Tiktok as i32 => {
            if let Some(c) = Regex::new(r"tiktok\.com/@([^/?#]+)")
                .unwrap()
                .captures(&normalized)
            {
                out.username = c[1].to_string();
            }
        }
        x if x == PlatformValue::Xiaohongshu as i32 => {
            if let Some(c) = Regex::new(r"redelight\.cn/[^/]+/([^/?#]+)")
                .unwrap()
                .captures(&normalized)
            {
                out.username = c[1].to_string();
            }
        }
        _ => {}
    }
    Ok(out)
}
