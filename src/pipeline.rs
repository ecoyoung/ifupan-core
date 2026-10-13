use std::path::Path;
use std::sync::Arc;

use crate::db::Store;
use crate::detector::detect;
use crate::models::Anchor;
use crate::platforms::match_url;
use crate::scheduler::Scheduler;

pub struct Pipeline {
    pub store: Arc<Store>,
    pub scheduler: Arc<Scheduler>,
}

impl Pipeline {
    pub fn new(db_path: &str, save_path: &str) -> Result<Self, String> {
        if let Some(parent) = Path::new(db_path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::create_dir_all(save_path);
        let store = Arc::new(Store::open(db_path).map_err(|e| e.to_string())?);
        crate::session::install(store.clone());
        let mut cfg = store.get_config().map_err(|e| e.to_string())?;
        cfg.save_path = save_path.to_string();
        store.save_config(&cfg).map_err(|e| e.to_string())?;
        let scheduler = Arc::new(Scheduler::new(store.clone(), cfg));
        Ok(Self { store, scheduler })
    }

    pub fn default_paths() -> Result<Self, String> {
        Self::new("./data/review_analysis.sqlite", "./recordings")
    }

    pub fn add(&self, url: &str) -> Result<Anchor, String> {
        self.scheduler.add_anchor(url, 1, 1)
    }

    pub fn probe(&self, url: &str) -> Result<serde_json::Value, String> {
        let parsed = match_url(url)?;
        let room = detect(url);
        Ok(serde_json::json!({
            "success": room.success,
            "live": room.live,
            "title": room.title,
            "nickname": room.nickname,
            "avatar": room.avatar,
            "sec_uid": room.sec_uid,
            "web_rid": room.web_rid,
            "room_id": room.room_id,
            "stream_url": room.stream_url,
            "hls_url": room.hls_url,
            "online": room.online,
            "cover": room.cover,
            "error": room.error,
            "wss_url": room.wss_url,
            "qualities": room.qualities,
            "bean": parsed.spec.bean,
            "platform": parsed.spec.platform_name,
        }))
    }
}
