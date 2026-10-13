use flate2::read::GzDecoder;
use std::io::Read;

use crate::models::{now_str, DanmuEvent, DanmuType};
use crate::proto_wire::{
    encode_len, encode_string, encode_varint_field, field_bytes, field_repeated_bytes, field_str,
    field_varint, walk,
};

#[derive(Clone, Debug, Default)]
pub struct PushFrame {
    pub payload_encoding: String,
    pub payload_type: String,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct ImResponse {
    pub messages: Vec<ImMessage>,
    pub cursor: String,
    pub internal_ext: String,
    pub heartbeat_ms: u64,
    pub need_ack: bool,
}

#[derive(Clone, Debug)]
pub struct ImMessage {
    pub method: String,
    pub payload: Vec<u8>,
    pub msg_id: u64,
}

pub fn decode_push_frame(buf: &[u8]) -> PushFrame {
    let f = walk(buf);
    PushFrame {
        payload_encoding: field_str(&f, 6).unwrap_or_default(),
        payload_type: field_str(&f, 7).unwrap_or_default(),
        payload: field_bytes(&f, 8).unwrap_or(&[]).to_vec(),
    }
}

pub fn encode_ack(internal_ext: &str) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(encode_string(7, "ack"));
    out.extend(encode_len(8, internal_ext.as_bytes()));
    out
}

pub fn encode_heartbeat() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(encode_string(7, "hb"));
    out.extend(encode_varint_field(3, 1));
    out
}

pub fn decode_im_response(buf: &[u8]) -> ImResponse {
    let f = walk(buf);
    let messages = field_repeated_bytes(&f, 1)
        .into_iter()
        .map(decode_im_message)
        .collect();
    ImResponse {
        messages,
        cursor: field_str(&f, 2).unwrap_or_default(),
        internal_ext: field_str(&f, 5).unwrap_or_default(),
        heartbeat_ms: field_varint(&f, 8).unwrap_or(0),
        need_ack: field_varint(&f, 9).unwrap_or(0) != 0,
    }
}

fn decode_im_message(buf: &[u8]) -> ImMessage {
    let f = walk(buf);
    ImMessage {
        method: field_str(&f, 1).unwrap_or_default(),
        payload: field_bytes(&f, 2).unwrap_or(&[]).to_vec(),
        msg_id: field_varint(&f, 3).unwrap_or(0),
    }
}

pub fn unzip_payload(encoding: &str, payload: &[u8]) -> Vec<u8> {
    let gzip = encoding.eq_ignore_ascii_case("gzip")
        || (payload.len() >= 2 && payload[0] == 0x1f && payload[1] == 0x8b);
    if !gzip {
        return payload.to_vec();
    }
    let mut d = GzDecoder::new(payload);
    let mut out = Vec::new();
    if d.read_to_end(&mut out).is_ok() {
        out
    } else {
        payload.to_vec()
    }
}

pub fn events_from_frame(buf: &[u8], video_id: &str, batch: &str) -> (ImResponse, Vec<DanmuEvent>) {
    events_from_bytes(buf, video_id, batch)
}

pub fn events_from_bytes(buf: &[u8], video_id: &str, batch: &str) -> (ImResponse, Vec<DanmuEvent>) {
    let frame = decode_push_frame(buf);
    if !frame.payload.is_empty() && frame.payload_type != "ack" && frame.payload_type != "hb" {
        let inner = unzip_payload(&frame.payload_encoding, &frame.payload);
        let resp = decode_im_response(&inner);
        let events: Vec<DanmuEvent> = resp
            .messages
            .iter()
            .filter_map(|m| message_to_event(m, video_id, batch))
            .collect();
        if !events.is_empty() || !resp.cursor.is_empty() || !resp.messages.is_empty() {
            return (resp, events);
        }
    }
    let inner = unzip_payload("", buf);
    let resp = decode_im_response(&inner);
    let events: Vec<DanmuEvent> = resp
        .messages
        .iter()
        .filter_map(|m| message_to_event(m, video_id, batch))
        .collect();
    (resp, events)
}

fn message_to_event(m: &ImMessage, video_id: &str, batch: &str) -> Option<DanmuEvent> {
    let kind = map_method(&m.method)?;
    let f = walk(&m.payload);
    let user_n = match kind {
        DanmuType::Gift => 7,
        DanmuType::Like => 5,
        DanmuType::FansClub => 4,
        _ => 2,
    };
    let (user_id, nick_name, sec_uid) = parse_user(field_bytes(&f, user_n).unwrap_or(&[]));
    let mut gift_name = String::new();
    let mut gift_count = 0i64;
    let mut online = 0i64;
    let content = match kind {
        DanmuType::Chat => {
            let c = text_field(&f, 3);
            if c.is_empty() {
                return None;
            }
            c
        }
        DanmuType::FansClub => text_field(&f, 3),
        DanmuType::Gift => {
            let gift = walk(field_bytes(&f, 15).unwrap_or(&[]));
            gift_name = text_field(&gift, 16);
            if gift_name.is_empty() {
                gift_name = text_field(&gift, 2);
            }
            gift_count = field_varint(&f, 6)
                .or_else(|| field_varint(&f, 5))
                .or_else(|| field_varint(&f, 4))
                .unwrap_or(1) as i64;
            gift_name.clone()
        }
        DanmuType::Like => {
            let n = field_varint(&f, 2).unwrap_or(1);
            gift_count = n as i64;
            format!("+{n}")
        }
        DanmuType::Enter => {
            online = field_varint(&f, 3).unwrap_or(0) as i64;
            let c = text_field(&f, 11);
            if c.is_empty() {
                "进入直播间".into()
            } else {
                c
            }
        }
        DanmuType::Follow => "关注了主播".into(),
        DanmuType::Online => {
            online = field_varint(&f, 3)
                .or_else(|| field_varint(&f, 5))
                .unwrap_or(0) as i64;
            let c = text_field(&f, 6);
            if c.is_empty() {
                text_field(&f, 4)
            } else {
                c
            }
        }
    };
    Some(DanmuEvent {
        msg_type: kind as i32,
        content,
        nick_name,
        user_id,
        gift_name,
        gift_count: if matches!(kind, DanmuType::Gift | DanmuType::Like) {
            gift_count
        } else {
            0
        },
        online: if matches!(kind, DanmuType::Online | DanmuType::Enter) {
            online
        } else {
            0
        },
        event_time: now_str(),
        msg_id: if m.msg_id == 0 {
            String::new()
        } else {
            m.msg_id.to_string()
        },
        batch_number: batch.into(),
        video_id: video_id.into(),
        method: m.method.clone(),
        sec_uid,
    })
}

fn parse_user(buf: &[u8]) -> (String, String, String) {
    if buf.is_empty() {
        return (String::new(), String::new(), String::new());
    }
    let u = walk(buf);
    let id = field_varint(&u, 1)
        .map(|v| v.to_string())
        .unwrap_or_default();
    let nick = text_field(&u, 3);
    let mut sec = String::new();
    for n in [12u32, 18, 21, 38, 46] {
        let s = text_field(&u, n);
        if s.len() > 8 {
            sec = s;
            break;
        }
    }
    (id, nick, sec)
}

fn text_field(fields: &[(u32, crate::proto_wire::Wire)], n: u32) -> String {
    let Some(raw) = field_str(fields, n) else {
        return String::new();
    };
    let t = raw.trim();
    if t.is_empty() || t.chars().filter(|c| *c == '\u{fffd}').count() > 2 {
        return String::new();
    }
    if t.chars().any(|c| c.is_control()) {
        return String::new();
    }
    t.to_string()
}

pub fn map_method(method: &str) -> Option<DanmuType> {
    Some(match method {
        "WebcastChatMessage" | "chat" => DanmuType::Chat,
        "WebcastLikeMessage" | "like" => DanmuType::Like,
        "WebcastMemberMessage" | "enter" => DanmuType::Enter,
        "WebcastSocialMessage" | "follow" => DanmuType::Follow,
        "WebcastGiftMessage" | "gift" => DanmuType::Gift,
        "WebcastRoomUserSeqMessage" | "online" => DanmuType::Online,
        "WebcastFansclubMessage" => DanmuType::FansClub,
        _ => return None,
    })
}

pub fn danmu_stats(events: &[DanmuEvent]) -> serde_json::Value {
    let mut users = std::collections::HashSet::new();
    let mut chat = 0u64;
    let mut gift = 0u64;
    let mut gift_pcs = 0i64;
    let mut likes = 0i64;
    let mut enters = 0u64;
    let mut follows = 0u64;
    let mut peak = 0i64;
    for e in events {
        if !e.user_id.is_empty() {
            users.insert(e.user_id.clone());
        }
        match e.msg_type {
            x if x == DanmuType::Chat as i32 => chat += 1,
            x if x == DanmuType::Gift as i32 => {
                gift += 1;
                gift_pcs += e.gift_count.max(1);
            }
            x if x == DanmuType::Like as i32 => likes += e.gift_count.max(1),
            x if x == DanmuType::Enter as i32 => enters += 1,
            x if x == DanmuType::Follow as i32 => follows += 1,
            x if x == DanmuType::Online as i32 => peak = peak.max(e.online),
            _ => {}
        }
        peak = peak.max(e.online);
    }
    serde_json::json!({
        "events": events.len(),
        "unique_users": users.len(),
        "chat": chat,
        "gift": gift,
        "gift_pieces": gift_pcs,
        "likes": likes,
        "enters": enters,
        "follows": follows,
        "peak_online": peak,
    })
}

#[allow(dead_code)]
pub fn encode_chat_frame(nick: &str, content: &str, gzip: bool) -> Vec<u8> {
    let mut user = Vec::new();
    user.extend(encode_varint_field(1, 42));
    user.extend(encode_string(3, nick));
    let mut chat = Vec::new();
    chat.extend(encode_len(2, &user));
    chat.extend(encode_string(3, content));
    let mut msg = Vec::new();
    msg.extend(encode_string(1, "WebcastChatMessage"));
    msg.extend(encode_len(2, &chat));
    msg.extend(encode_varint_field(3, 7));
    wrap_msg_frame(msg, gzip)
}

#[allow(dead_code)]
pub fn encode_gift_frame(nick: &str, gift: &str, count: u64, gzip: bool) -> Vec<u8> {
    let mut user = Vec::new();
    user.extend(encode_varint_field(1, 99));
    user.extend(encode_string(3, nick));
    let mut gs = Vec::new();
    gs.extend(encode_string(16, gift));
    let mut payload = Vec::new();
    payload.extend(encode_len(7, &user));
    payload.extend(encode_varint_field(6, count));
    payload.extend(encode_len(15, &gs));
    let mut msg = Vec::new();
    msg.extend(encode_string(1, "WebcastGiftMessage"));
    msg.extend(encode_len(2, &payload));
    msg.extend(encode_varint_field(3, 8));
    wrap_msg_frame(msg, gzip)
}

#[allow(dead_code)]
pub fn encode_unknown_frame(method: &str, gzip: bool) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend(encode_string(3, "InteractEffectSyncData"));
    let mut msg = Vec::new();
    msg.extend(encode_string(1, method));
    msg.extend(encode_len(2, &payload));
    wrap_msg_frame(msg, gzip)
}

fn wrap_msg_frame(msg: Vec<u8>, gzip: bool) -> Vec<u8> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;
    let mut resp = Vec::new();
    resp.extend(encode_len(1, &msg));
    resp.extend(encode_string(5, "ext"));
    resp.extend(encode_varint_field(9, 1));
    let payload = if gzip {
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        let _ = enc.write_all(&resp);
        enc.finish().unwrap_or(resp)
    } else {
        resp
    };
    let mut frame = Vec::new();
    if gzip {
        frame.extend(encode_string(6, "gzip"));
    }
    frame.extend(encode_string(7, "msg"));
    frame.extend(encode_len(8, &payload));
    frame
}

pub fn find_wss(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) if s.starts_with("wss://") => Some(s.clone()),
        serde_json::Value::Array(a) => a.iter().find_map(find_wss),
        serde_json::Value::Object(m) => m.values().find_map(find_wss),
        _ => None,
    }
}
