//! Douyin WSS URL signer. Talks to an external SignWss HTTP API.
//!
//! Not 爱复盘 login. Set locally:
//!   IFUPAN_SIGN_URL  POST endpoint
//!   IFUPAN_SIGN_KEY  ApiKey
//! Files (gitignored): `.env` or `data/sign.env`

use std::path::Path;
use std::sync::Once;

use serde_json::{json, Value};

static LOAD: Once = Once::new();

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
        if k.starts_with("IFUPAN_SIGN_") && std::env::var(k).is_err() {
            std::env::set_var(k, v);
        }
    }
}

pub fn configured() -> bool {
    load_env();
    let url = std::env::var("IFUPAN_SIGN_URL").unwrap_or_default();
    let key = std::env::var("IFUPAN_SIGN_KEY").unwrap_or_default();
    url.starts_with("http") && !key.is_empty()
}

pub fn user_unique_id() -> String {
    let n = uuid::Uuid::new_v4().as_u128();
    format!("{}", 1_000_000_000_000_000_000u128 + (n % 8_999_999_999_999_999_999u128))
}

pub fn sign_wss(room_id: &str) -> Result<String, String> {
    load_env();
    if room_id.is_empty() {
        return Err("room_id required".into());
    }
    let url = std::env::var("IFUPAN_SIGN_URL").unwrap_or_default();
    let key = std::env::var("IFUPAN_SIGN_KEY").unwrap_or_default();
    if !url.starts_with("http") || key.is_empty() {
        return Err("IFUPAN_SIGN_URL / IFUPAN_SIGN_KEY not set".into());
    }
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
