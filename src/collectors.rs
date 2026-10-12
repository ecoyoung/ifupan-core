use serde_json::json;

use crate::models::{Anchor, CollectResult, LiveRoom, now_str};

pub fn collect_all(anchor: &Anchor, room: Option<&LiveRoom>) -> Vec<CollectResult> {
    vec![live_meta(anchor, room), juliang(anchor), qianchuan()]
}

fn live_meta(anchor: &Anchor, room: Option<&LiveRoom>) -> CollectResult {
    match room {
        None => CollectResult {
            success: false,
            platform_id: "anchor_live".into(),
            raw: json!({}),
            error: "no room".into(),
        },
        Some(r) => CollectResult {
            success: true,
            platform_id: "anchor_live".into(),
            raw: json!({
                "secUid": r.sec_uid,
                "webRid": r.web_rid,
                "title": r.title,
                "online": r.online,
                "live": r.live,
                "streamUrl": r.stream_url,
                "anchorId": anchor.id,
                "collectTime": now_str(),
            }),
            error: String::new(),
        },
    }
}

fn juliang(anchor: &Anchor) -> CollectResult {
    CollectResult {
        success: true,
        platform_id: "juliang".into(),
        raw: json!({
            "authorized": false,
            "note": "needs compass cookie; endpoints from binary literals",
            "reviewCommentApi": "https://compass.jinritemai.com/compass_api/content_live/author/live_screen/review_comment",
            "blendTrendApi": "https://compass.jinritemai.com/compass_api/author/live/live_screen/blend_trend_v2",
            "secUid": anchor.sec_uid,
        }),
        error: String::new(),
    }
}

fn qianchuan() -> CollectResult {
    CollectResult {
        success: true,
        platform_id: "qianchuan".into(),
        raw: json!({"authorized": false}),
        error: String::new(),
    }
}
