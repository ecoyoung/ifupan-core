//! Local Douyin WSS signature: MD5 X-MS-STUB + `fp.data` frontierSign (Node).

use std::path::PathBuf;
use std::process::Command;

const SDK: &str = "1.0.12";
const UA_VER: &str = "5.0+(Windows+NT+10.0%3b+Win64%3b+x64)+AppleWebKit%2f537.36+(KHTML%2c+like+Gecko)+Chrome%2f126.0.0.0+Safari%2f537.36";

pub fn configured() -> bool {
    fp_path().is_some() && frontier_script().is_some() && which("node").is_some()
}

pub fn user_unique_id() -> String {
    let n = uuid::Uuid::new_v4().as_u128();
    format!("{}", 1_000_000_000_000_000_000u128 + (n % 8_999_999_999_999_999_999u128))
}

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
    if room_id.is_empty() {
        return Err("room_id required".into());
    }
    let fp = fp_path().ok_or_else(|| "fp.data not found (copy fp/fp.data to data/fp.data)".to_string())?;
    let script = frontier_script().ok_or_else(|| "scripts/frontier_sign.js not found".to_string())?;
    let node = which("node").ok_or_else(|| "node not on PATH".to_string())?;
    let uid = user_unique_id();
    let stub = x_ms_stub(&stub_params(room_id, &uid));
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
