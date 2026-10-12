use serde_json::Value;

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
        content: str_of(raw, "content").or_else(|| str_of(raw, "text")).unwrap_or_default(),
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
    }
}

fn str_of(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).map(|s| s.to_string())
}

fn map_type(method: &str) -> DanmuType {
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
