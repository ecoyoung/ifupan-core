use serde_json::Value;

use crate::models::{LiveRoom, PlatformValue};
use crate::platforms::{match_url, ParsedLiveUrl};

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .user_agent(UA)
        .timeout(std::time::Duration::from_secs(12))
        .build()
}

fn http_get(url: &str) -> Result<(u16, String, String), String> {
    match agent().get(url).call() {
        Ok(resp) => {
            let status = resp.status();
            let final_url = resp.get_url().to_string();
            let body = resp.into_string().unwrap_or_default();
            Ok((status, body, final_url))
        }
        Err(ureq::Error::Status(code, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            Ok((code, body, url.to_string()))
        }
        Err(e) => Err(e.to_string()),
    }
}

pub fn detect(url: &str) -> LiveRoom {
    let parsed = match match_url(url) {
        Ok(p) => p,
        Err(e) => {
            return LiveRoom {
                error: e,
                ..LiveRoom::default()
            }
        }
    };
    detect_parsed(&parsed)
}

pub fn detect_parsed(parsed: &ParsedLiveUrl) -> LiveRoom {
    match parsed.spec.platform_value {
        x if x == PlatformValue::Douyin as i32 => detect_douyin(parsed),
        x if x == PlatformValue::Kuaishou as i32 => detect_kuaishou(parsed),
        x if x == PlatformValue::Tiktok as i32 => detect_generic(parsed, "LiveRoom"),
        x if x == PlatformValue::Xiaohongshu as i32 => detect_generic(parsed, "live"),
        _ => LiveRoom {
            error: "unknown platform".into(),
            ..LiveRoom::default()
        },
    }
}

fn detect_douyin(parsed: &ParsedLiveUrl) -> LiveRoom {
    let mut web_rid = parsed.web_rid.clone();
    if web_rid.is_empty() {
        if let Ok((_, body, _)) = http_get(&parsed.source_url) {
            if let Some(i) = body.find("\"web_rid\"") {
                let slice = &body[i..i.saturating_add(40).min(body.len())];
                web_rid = slice
                    .chars()
                    .filter(|c| c.is_ascii_digit())
                    .collect();
            }
        }
        if web_rid.is_empty() {
            return LiveRoom {
                error: "no web_rid".into(),
                sec_uid: parsed.sec_uid.clone(),
                ..LiveRoom::default()
            };
        }
    }
    let api = format!(
        "https://live.douyin.com/webcast/room/web/enter/?aid=6383&app_name=douyin_web&live_id=1&device_platform=web&language=zh-CN&enter_from=web_live&cookie_enabled=true&screen_width=1920&screen_height=1080&browser_language=zh-CN&browser_platform=Win32&browser_name=Chrome&browser_version=120.0.0.0&web_rid={web_rid}"
    );
    let (code, body, _) = match http_get(&api) {
        Ok(v) => v,
        Err(e) => {
            return LiveRoom {
                web_rid,
                error: e,
                ..LiveRoom::default()
            }
        }
    };
    if code != 200 {
        return LiveRoom {
            web_rid,
            error: format!("enter http {code}"),
            ..LiveRoom::default()
        };
    }
    let payload: Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => {
            return LiveRoom {
                web_rid,
                error: "bad json".into(),
                ..LiveRoom::default()
            }
        }
    };
    let data = payload.get("data").cloned().unwrap_or(Value::Null);
    let room = data
        .get("data")
        .and_then(|d| d.as_array())
        .and_then(|a| a.first())
        .cloned()
        .or_else(|| data.get("room").cloned())
        .unwrap_or(Value::Null);
    let status = room.get("status").and_then(|v| v.as_i64()).unwrap_or(0);
    let owner = room
        .get("owner")
        .cloned()
        .or_else(|| data.get("user").cloned())
        .unwrap_or(Value::Null);
    let stream = room.get("stream_url").cloned().unwrap_or(Value::Null);
    let flv_map = stream.get("flv_pull_url").cloned().unwrap_or(Value::Null);
    let flv = ["FULL_HD1", "HD1", "SD1", "SD2"]
        .iter()
        .find_map(|k| flv_map.get(*k).and_then(|v| v.as_str()))
        .unwrap_or("")
        .to_string();
    let hls = stream
        .get("hls_pull_url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let avatar = owner
        .get("avatar_thumb")
        .and_then(|a| a.get("url_list"))
        .and_then(|u| u.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let online = room
        .get("user_count")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    LiveRoom {
        success: true,
        live: status == 2 && (!flv.is_empty() || !hls.is_empty()),
        title: room
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        nickname: owner
            .get("nickname")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        avatar,
        sec_uid: owner
            .get("sec_uid")
            .and_then(|v| v.as_str())
            .unwrap_or(&parsed.sec_uid)
            .to_string(),
        web_rid,
        room_id: room
            .get("id_str")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        stream_url: flv,
        hls_url: hls,
        online,
        cover: String::new(),
        error: String::new(),
    }
}

fn detect_kuaishou(parsed: &ParsedLiveUrl) -> LiveRoom {
    match http_get(&parsed.source_url) {
        Ok((code, body, _)) => LiveRoom {
            success: code == 200,
            live: body.contains("\"living\":true") || body.contains("liveStreamId"),
            nickname: String::new(),
            web_rid: parsed.username.clone(),
            error: if code == 200 {
                String::new()
            } else {
                format!("http {code}")
            },
            ..LiveRoom::default()
        },
        Err(e) => LiveRoom {
            error: e,
            web_rid: parsed.username.clone(),
            ..LiveRoom::default()
        },
    }
}

fn detect_generic(parsed: &ParsedLiveUrl, marker: &str) -> LiveRoom {
    match http_get(&parsed.source_url) {
        Ok((code, body, _)) => LiveRoom {
            success: code == 200,
            live: body.contains(marker),
            nickname: parsed.username.clone(),
            web_rid: parsed.username.clone(),
            error: if code == 200 {
                String::new()
            } else {
                format!("http {code}")
            },
            ..LiveRoom::default()
        },
        Err(e) => LiveRoom {
            error: e,
            web_rid: parsed.username.clone(),
            ..LiveRoom::default()
        },
    }
}
