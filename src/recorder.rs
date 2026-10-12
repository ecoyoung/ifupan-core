use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::models::{now_str, Anchor, AnchorVideo, ClientConfig, RecordStatus};

pub struct RecordSession {
    pub video_id: String,
    pub stop: Arc<Mutex<bool>>,
    pub child: Mutex<Option<Child>>,
}

pub struct Recorder {
    pub cfg: ClientConfig,
    sessions: Mutex<HashMap<i64, Arc<RecordSession>>>,
    ffmpeg: Option<PathBuf>,
}

impl Recorder {
    pub fn new(cfg: ClientConfig) -> Self {
        let ffmpeg = which("ffmpeg");
        Self {
            cfg,
            sessions: Mutex::new(HashMap::new()),
            ffmpeg,
        }
    }

    pub fn is_recording(&self, anchor_id: i64) -> bool {
        self.sessions.lock().unwrap().contains_key(&anchor_id)
    }

    pub fn start(&self, anchor: &Anchor, stream_url: &str, live_title: &str) -> Result<AnchorVideo, String> {
        if stream_url.is_empty() {
            return Err("empty stream_url".into());
        }
        let save_root = PathBuf::from(if self.cfg.save_path.is_empty() {
            "./recordings"
        } else {
            &self.cfg.save_path
        });
        let aid = anchor.id.unwrap_or(0);
        let dir = save_root.join(aid.to_string());
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let batch = chrono::Local::now().format("%Y%m%d%H%M%S").to_string();
        let video_id = uuid::Uuid::new_v4().simple().to_string();
        let name = format!(
            "{}_{}_p1.flv",
            if anchor.anchor_name.is_empty() {
                &anchor.web_rid
            } else {
                &anchor.anchor_name
            },
            batch
        );
        let out = dir.join(&name);
        let video = AnchorVideo {
            id: None,
            video_id: video_id.clone(),
            video_name: name,
            start_time: now_str(),
            paragraph: 1,
            duration: "0".into(),
            video_size: String::new(),
            end_time: String::new(),
            batch_number: batch,
            storage_path: out.to_string_lossy().into(),
            live_url: anchor.live_url.clone(),
            source_url: anchor.source_url.clone(),
            anchor_id: aid,
            is_recording: RecordStatus::Recording as i64,
            live_title: live_title.into(),
            analysis_status: 1,
            error_reason: String::new(),
            sec_uid: anchor.sec_uid.clone(),
            platform_type: anchor.platform_value.to_string(),
        };
        let stop = Arc::new(Mutex::new(false));
        let session = Arc::new(RecordSession {
            video_id: video_id.clone(),
            stop: stop.clone(),
            child: Mutex::new(None),
        });
        self.sessions.lock().unwrap().insert(aid, session.clone());
        let slice_sec = self.cfg.slice_minutes() * 60;
        let ffmpeg = self.ffmpeg.clone();
        let stream = stream_url.to_string();
        thread::spawn(move || run_record(ffmpeg, stream, out, slice_sec, session));
        Ok(video)
    }

    pub fn stop(&self, anchor_id: i64) {
        if let Some(s) = self.sessions.lock().unwrap().remove(&anchor_id) {
            *s.stop.lock().unwrap() = true;
            if let Some(mut child) = s.child.lock().unwrap().take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

fn run_record(
    ffmpeg: Option<PathBuf>,
    stream: String,
    out: PathBuf,
    slice_sec: u64,
    session: Arc<RecordSession>,
) {
    if let Some(bin) = ffmpeg {
        let child = Command::new(bin)
            .args(["-y", "-hide_banner", "-loglevel", "error", "-i", &stream, "-c", "copy", "-t"])
            .arg(slice_sec.to_string())
            .arg(&out)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match child {
            Ok(c) => {
                *session.child.lock().unwrap() = Some(c);
                loop {
                    if *session.stop.lock().unwrap() {
                        if let Some(mut ch) = session.child.lock().unwrap().take() {
                            let _ = ch.kill();
                            let _ = ch.wait();
                        }
                        break;
                    }
                    let done = session
                        .child
                        .lock()
                        .unwrap()
                        .as_mut()
                        .map(|c| c.try_wait().ok().flatten().is_some())
                        .unwrap_or(true);
                    if done {
                        break;
                    }
                    thread::sleep(Duration::from_millis(400));
                }
            }
            Err(_) => {
                let _ = std::fs::write(&out, b"FLV\x01dry-run");
            }
        }
    } else {
        let _ = std::fs::write(&out, b"FLV\x01dry-run");
        for _ in 0..15 {
            if *session.stop.lock().unwrap() {
                break;
            }
            thread::sleep(Duration::from_millis(200));
        }
    }
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
        #[cfg(windows)]
        {
            let p = dir.join(format!("{name}.exe"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    let _ = Path::new(name);
    None
}
