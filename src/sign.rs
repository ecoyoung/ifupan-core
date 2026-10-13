//! Local Douyin WSS signature via bundled `fp.data` + Node (`frontierSign`).
//! Optional remote SignWss remains behind IFUPAN_SIGN_URL/KEY (not required).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

static LOAD: Once = Once::new();

const SDK: &str = "1.0.12";
const UA_VER: &str = "5.0+(Windows+NT+10.0%3b+Win64%3b+x64)+AppleWebKit%2f537.36+(KHTML%2c+like+Gecko)+Chrome%2f126.0.0.0+Safari%2f537.36";

pub fn load_env() {
    LOAD.call_once(|| {
        for p in [".env", "data/sign.env", "./data/sign.env"] {
            absorb_file(Path::new(p));
        }
    });
}

fn absorb_file(path: &Path) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let k = k.trim();
        let v = v.trim().trim_matches('"').trim_matches('\'');
        if k.starts_with("IFUPAN_") && std::env::var(k).is_err() {
            std::env::set_var(k, v);
        }
    }
}

pub fn configured() -> bool {
    load_env();
    fp_path().is_some() || remote_configured()
}

fn remote_configured() -> bool {
    let url = std::env::var("IFUPAN_SIGN_URL").unwrap_or_default();
    let key = std::env::var("IFUPAN_SIGN_KEY").unwrap_or_default();
    url.starts_with("http") && !key.is_empty()
}

pub fn user_unique_id() -> String {
    let n = uuid::Uuid::new_v4().as_u128();
    format!("{}", 1_000_000_000_000_000_000u128 + (n % 8_999_999_999_999_999_999u128))
}

/// Comma list hashed as X-MS-STUB (BarrageGrab / webcast SDK).
pub fn stub_params(room_id: &str, user_unique_id: &str) -> String {
    format!(
        "live_id=1,aid=6383,version_code=180800,webcast_sdk_version={SDK},room_id={room_id},sub_room_id=,sub_channel_id=,did_rule=3,user_unique_id={user_unique_id},device_platform=web,device_type=,ac=,identity=audience"
    )
}

pub fn x_ms_stub(params: &str) -> String {
    format!("{:x}", md5::compute(params.as_bytes()))
}

pub fn unsigned_wss(room_id: &str, user_unique_id: &str) -> String {
    format!(
        "wss://webcast5-ws-web-lf.douyin.com/webcast/im/push/v2/?app_name=douyin_web&version_code=180800&webcast_sdk_version={SDK}&update_version_code={SDK}&compress=gzip&device_platform=web&cookie_enabled=true&screen_width=1440&screen_height=960&browser_language=zh-CN&browser_platform=Win32&browser_name=Mozilla&browser_version={UA_VER}&browser_online=true&tz_name=Asia/Shanghai&host=https://live.douyin.com&aid=6383&live_id=1&did_rule=3&endpoint=live_pc&support_wrds=1&user_unique_id={user_unique_id}&im_path=/webcast/im/fetch/&identity=audience&room_id={room_id}&heartbeatDuration=0"
    )
}

pub fn sign_wss(room_id: &str) -> Result<String, String> {
    load_env();
    if room_id.is_empty() {
        return Err("room_id required".into());
    }
    match sign_local(room_id) {
        Ok(u) => return Ok(u),
        Err(e) => {
            if !remote_configured() {
                return Err(e);
            }
            eprintln!("barrage local sign skipped: {e}");
        }
    }
    sign_remote(room_id)
}

pub fn sign_local(room_id: &str) -> Result<String, String> {
    let fp = fp_path().ok_or_else(|| "fp.data not found (copy original fp/fp.data to data/fp.data)".to_string())?;
    let script = frontier_script().ok_or_else(|| "scripts/frontier_sign.js not found".to_string())?;
    let node = which("node").ok_or_else(|| "node not on PATH".to_string())?;
    let uid = user_unique_id();
    let params = stub_params(room_id, &uid);
    let stub = x_ms_stub(&params);
    let out = Command::new(node)
        .arg(&script)
        .arg(&stub)
        .arg(&fp)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!(
            "frontier_sign: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let sig = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if sig.len() < 8 {
        return Err("empty local signature".into());
    }
    Ok(format!("{}&signature={sig}", unsigned_wss(room_id, &uid)))
}

fn sign_remote(room_id: &str) -> Result<String, String> {
    let url = std::env::var("IFUPAN_SIGN_URL").unwrap_or_default();
    let key = std::env::var("IFUPAN_SIGN_KEY").unwrap_or_default();
    let uid = user_unique_id();
    let body = json!({
        "ApiKey": key,
        "RoomId": room_id,
        "UserUniqueId": uid,
    });
    let resp = crate::detector::agent()
        .post(&url)
        .set("Content-Type", "application/json")
        .send_json(body)
        .map_err(|e| e.to_string())?;
    let v: Value = resp.into_json().map_err(|e| e.to_string())?;
    parse_wss_url(&v)
}

pub fn parse_wss_url(v: &Value) -> Result<String, String> {
    let code = v
        .get("Code")
        .or_else(|| v.get("code"))
        .and_then(|x| x.as_i64())
        .unwrap_or(-1);
    if code != 0 {
        let msg = v
            .get("Msg")
            .or_else(|| v.get("msg"))
            .and_then(|x| x.as_str())
            .unwrap_or("sign failed");
        return Err(msg.into());
    }
    let data = v.get("Data").or_else(|| v.get("data")).cloned().unwrap_or(Value::Null);
    let wss = data
        .get("WssUrl")
        .or_else(|| data.get("wssUrl"))
        .or_else(|| data.get("wss_url"))
        .and_then(|x| x.as_str())
        .unwrap_or("");
    if !wss.starts_with("wss://") {
        return Err("no WssUrl".into());
    }
    Ok(wss.to_string())
}

pub fn redact_wss(url: &str) -> String {
    match url.split_once("signature=") {
        Some((head, rest)) => {
            let sig: String = rest.chars().take_while(|c| *c != '&').collect();
            format!("{head}signature=<{} chars>", sig.len())
        }
        None => url.to_string(),
    }
}

fn fp_path() -> Option<PathBuf> {
    load_env();
    let mut cands: Vec<PathBuf> = Vec::new();
    if let Ok(p) = std::env::var("IFUPAN_FP") {
        cands.push(PathBuf::from(p));
    }
    cands.extend([
        PathBuf::from("data/fp.data"),
        PathBuf::from("./data/fp.data"),
        PathBuf::from("fp.data"),
        PathBuf::from("../extracted/app/fp/fp.data"),
    ]);
    cands.into_iter().find(|p| p.is_file())
}

fn frontier_script() -> Option<PathBuf> {
    let mut cands = vec![
        PathBuf::from("scripts/frontier_sign.js"),
        PathBuf::from("./scripts/frontier_sign.js"),
    ];
    if let Ok(m) = std::env::var("CARGO_MANIFEST_DIR") {
        cands.push(PathBuf::from(m).join("scripts/frontier_sign.js"));
    }
    cands.into_iter().find(|p| p.is_file())
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

#[allow(dead_code)]
fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}
