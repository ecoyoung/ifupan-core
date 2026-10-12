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
    let frame = decode_push_frame(buf);
    if frame.payload_type == "ack" || frame.payload_type == "hb" || frame.payload.is_empty() {
        return (ImResponse::default(), Vec::new());
    }
    let inner = unzip_payload(&frame.payload_encoding, &frame.payload);
    let resp = decode_im_response(&inner);
    let mut events = Vec::new();
    for m in &resp.messages {
        if let Some(ev) = message_to_event(m, video_id, batch) {
            events.push(ev);
        }
    }
    (resp, events)
}

fn message_to_event(m: &ImMessage, video_id: &str, batch: &str) -> Option<DanmuEvent> {
    let kind = map_method(&m.method);
    let f = walk(&m.payload);
    let user_buf = field_bytes(&f, 2).or_else(|| field_bytes(&f, 7));
    let user = user_buf.map(walk).unwrap_or_default();
    let nick = field_str(&user, 3).unwrap_or_default();
    let uid = field_varint(&user, 1)
        .map(|v| v.to_string())
        .unwrap_or_default();
    let content = field_str(&f, 3).unwrap_or_default();
    let gift = walk(field_bytes(&f, 15).unwrap_or(&[]));
    let gift_name = field_str(&gift, 16)
        .or_else(|| field_str(&gift, 2))
        .unwrap_or_default();
    let gift_count = field_varint(&f, 6)
        .or_else(|| field_varint(&f, 5))
        .or_else(|| field_varint(&f, 4))
        .unwrap_or(0) as i64;
    let online = field_varint(&f, 6)
        .or_else(|| field_varint(&f, 3))
        .unwrap_or(0) as i64;
    let ev = DanmuEvent {
        msg_type: kind as i32,
        content: if kind == DanmuType::Chat {
            content
        } else {
            content
        },
        nick_name: nick,
        user_id: uid,
        gift_name,
        gift_count: if kind == DanmuType::Gift { gift_count } else { 0 },
        online: if kind == DanmuType::Online { online } else { 0 },
        event_time: now_str(),
        msg_id: if m.msg_id == 0 {
            String::new()
        } else {
            m.msg_id.to_string()
        },
        batch_number: batch.into(),
        video_id: video_id.into(),
    };
    Some(ev)
}

pub fn map_method(method: &str) -> DanmuType {
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

#[allow(dead_code)]
pub fn encode_chat_frame(nick: &str, content: &str, gzip: bool) -> Vec<u8> {
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;
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
