use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::Value;

use crate::barrage_ws::{self, BarrageStart};
use crate::models::{now_str, DanmuEvent, DanmuType};

pub fn ingest(raw: &Value, video_id: &str, batch: &str) -> DanmuEvent {
    let method = raw
        .get("method")
        .or_else(|| raw.get("type"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let user = raw.get("user").cloned().unwrap_or(Value::Null);
    DanmuEvent {
        msg_type: map_type(method) as i32,
        content: str_of(raw, "content")
            .or_else(|| str_of(raw, "text"))
            .unwrap_or_default(),
        nick_name: user
            .get("nickname")
            .and_then(|v| v.as_str())
            .or_else(|| raw.get("nick_name").and_then(|v| v.as_str()))
            .unwrap_or("")
            .to_string(),
        user_id: user
            .get("id")
            .map(|v| v.to_string().trim_matches('"').to_string())
            .or_else(|| str_of(raw, "user_id"))
            .unwrap_or_default(),
        gift_name: str_of(raw, "gift_name").unwrap_or_default(),
        gift_count: raw.get("gift_count").and_then(|v| v.as_i64()).unwrap_or(0),
        online: raw.get("online").and_then(|v| v.as_i64()).unwrap_or(0),
        event_time: now_str(),
        msg_id: str_of(raw, "msg_id")
            .or_else(|| str_of(raw, "msgId"))
            .unwrap_or_default(),
        batch_number: batch.into(),
        video_id: video_id.into(),
        method: method.to_string(),
        sec_uid: user
            .get("secUid")
            .or_else(|| user.get("sec_uid"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    }
}

fn str_of(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).map(|s| s.to_string())
}

pub(crate) fn map_type(method: &str) -> DanmuType {
    match method {
        "WebcastChatMessage" | "chat" => DanmuType::Chat,
        "WebcastLikeMessage" | "like" => DanmuType::Like,
        "WebcastMemberMessage" | "enter" => DanmuType::Enter,
        "WebcastSocialMessage" | "follow" => DanmuType::Follow,
        "WebcastGiftMessage" | "gift" => DanmuType::Gift,
        "WebcastRoomUserSeqMessage" | "online" => DanmuType::Online,
        "WebcastFansclubMessage" => DanmuType::FansClub,
        _ => DanmuType::Chat,
    }
}

/// One persistent WSS worker per anchor.
pub struct BarrageHub {
    sessions: Arc<Mutex<HashMap<i64, Arc<AtomicBool>>>>,
}

impl Default for BarrageHub {
    fn default() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl BarrageHub {
    pub fn start<F>(&self, req: BarrageStart, sink: F)
    where
        F: Fn(DanmuEvent) + Send + Sync + 'static,
    {
        if req.room_id.is_empty() && !req.wss_url.starts_with("wss://") {
            return;
        }
        let mut map = self.sessions.lock().unwrap();
        if map.contains_key(&req.anchor_id) {
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        map.insert(req.anchor_id, stop.clone());
        drop(map);
        let sink = Arc::new(sink);
        let sessions = self.sessions.clone();
        let id = req.anchor_id;
        thread::spawn(move || {
            barrage_ws::run_loop(req, stop, sink);
            sessions.lock().unwrap().remove(&id);
        });
    }

    pub fn stop(&self, anchor_id: i64) {
        if let Some(flag) = self.sessions.lock().unwrap().remove(&anchor_id) {
            flag.store(true, Ordering::SeqCst);
        }
    }

    pub fn stop_all(&self) {
        let mut map = self.sessions.lock().unwrap();
        for (_, flag) in map.drain() {
            flag.store(true, Ordering::SeqCst);
        }
    }

    pub fn active(&self) -> Vec<i64> {
        self.sessions.lock().unwrap().keys().copied().collect()
    }
}
