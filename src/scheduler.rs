use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::collectors::collect_all;
use crate::db::Store;
use crate::detector::detect;
use crate::models::{now_str, Anchor, ClientConfig, LiveRoom, LiveStatus, RecordStatus};
use crate::platforms::match_url;
use crate::recorder::Recorder;

pub struct Scheduler {
    store: Arc<Store>,
    cfg: ClientConfig,
    recorder: Recorder,
    stop: Arc<AtomicBool>,
    pub detecting: Arc<AtomicBool>,
}

impl Scheduler {
    pub fn new(store: Arc<Store>, cfg: ClientConfig) -> Self {
        Self {
            recorder: Recorder::new(cfg.clone()),
            store,
            cfg,
            stop: Arc::new(AtomicBool::new(false)),
            detecting: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn add_anchor(&self, url: &str, auto_record: i64, barrage: i64) -> Result<Anchor, String> {
        let parsed = match_url(url)?;
        let name = if !parsed.web_rid.is_empty() {
            parsed.web_rid.clone()
        } else if !parsed.username.is_empty() {
            parsed.username.clone()
        } else if !parsed.sec_uid.is_empty() {
            parsed.sec_uid.clone()
        } else {
            url.to_string()
        };
        let mut a = Anchor {
            anchor_name: name,
            anchor_platform: parsed.spec.platform_name.clone(),
            platform_value: parsed.spec.platform_value,
            live_url: if parsed.spec.bean == "DouYinLive" {
                url.to_string()
            } else {
                String::new()
            },
            home_url: if parsed.spec.bean == "DouYinHomeLive" {
                url.to_string()
            } else {
                String::new()
            },
            app_share_url: if parsed.spec.bean == "DouYinAppShareLive" {
                url.to_string()
            } else {
                String::new()
            },
            is_auto_record: auto_record,
            sec_uid: parsed.sec_uid,
            web_rid: parsed.web_rid,
            source_url: url.to_string(),
            is_barrage_monitoring: barrage,
            add_time: now_str(),
            ..Anchor::default()
        };
        self.store
            .upsert_anchor(&mut a)
            .map_err(|e| e.to_string())?;
        Ok(a)
    }

    pub fn start_record(&self, anchor_id: i64) -> Result<serde_json::Value, String> {
        let mut a = self
            .store
            .get_anchor(anchor_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "anchor not found".to_string())?;
        let url = first_url(&a);
        let room = detect(&url);
        if !room.live || (room.stream_url.is_empty() && room.hls_url.is_empty()) {
            return Ok(serde_json::json!({
                "ok": false,
                "error": if room.error.is_empty() { "offline" } else { &room.error },
                "room": room,
            }));
        }
        apply_room(&mut a, &room);
        self.store.upsert_anchor(&mut a).map_err(|e| e.to_string())?;
        let stream = if room.stream_url.is_empty() {
            &room.hls_url
        } else {
            &room.stream_url
        };
        let video = self.recorder.start(&a, stream, &room.title)?;
        let _ = self.store.insert_video(&video);
        a.record_status = RecordStatus::Recording as i64;
        self.store.upsert_anchor(&mut a).map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "ok": true,
            "video_id": video.video_id,
            "path": video.storage_path,
        }))
    }

    pub fn stop_record(&self, anchor_id: i64) -> Result<serde_json::Value, String> {
        self.recorder.stop(anchor_id);
        if let Ok(Some(mut a)) = self.store.get_anchor(anchor_id) {
            a.record_status = RecordStatus::Idle as i64;
            let _ = self.store.upsert_anchor(&mut a);
        }
        Ok(serde_json::json!({"ok": true}))
    }

    pub fn start_detection(&self) {
        if self.detecting.swap(true, Ordering::SeqCst) {
            return;
        }
        self.stop.store(false, Ordering::SeqCst);
        let store = self.store.clone();
        let stop = self.stop.clone();
        let detecting = self.detecting.clone();
        let interval = self.cfg.detection_fre.max(2) as u64;
        thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                if let Ok(anchors) = store.list_anchors() {
                    for mut a in anchors {
                        if stop.load(Ordering::SeqCst) {
                            break;
                        }
                        let url = first_url(&a);
                        if url.is_empty() {
                            continue;
                        }
                        let room = detect(&url);
                        apply_room(&mut a, &room);
                        let _ = store.upsert_anchor(&mut a);
                        let _ = collect_all(&a, Some(&room));
                    }
                }
                thread::sleep(Duration::from_secs(interval));
            }
            detecting.store(false, Ordering::SeqCst);
        });
    }

    pub fn stop_detection(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Ok(list) = self.store.list_anchors() {
            for a in list {
                if let Some(id) = a.id {
                    self.recorder.stop(id);
                }
            }
        }
    }
}

fn first_url(a: &Anchor) -> String {
    for s in [&a.source_url, &a.live_url, &a.home_url, &a.app_share_url] {
        if !s.is_empty() {
            return s.clone();
        }
    }
    String::new()
}

fn apply_room(a: &mut Anchor, room: &LiveRoom) {
    a.live_status = if room.live {
        LiveStatus::Live as i64
    } else {
        LiveStatus::Offline as i64
    };
    if !room.stream_url.is_empty() {
        a.stream_url = room.stream_url.clone();
    } else if !room.hls_url.is_empty() {
        a.stream_url = room.hls_url.clone();
    }
    a.online_number = room.online.to_string();
    if !room.nickname.is_empty() {
        a.anchor_name = room.nickname.clone();
    }
    if !room.avatar.is_empty() {
        a.anchor_avatar = room.avatar.clone();
    }
    if !room.sec_uid.is_empty() {
        a.sec_uid = room.sec_uid.clone();
    }
    if !room.web_rid.is_empty() {
        a.web_rid = room.web_rid.clone();
    }
    if room.live {
        a.start_time = now_str();
    }
}
