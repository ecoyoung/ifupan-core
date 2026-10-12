use std::path::Path;
use std::process::Command;

use serde_json::json;

use crate::models::{AnalysisJob, AnalysisStatus};

const ARK_CHAT: &str = "https://ark.cn-beijing.volces.com/api/v3/chat/completions";

pub struct AnalysisPipeline {
    pub ark_key: String,
    pub model: String,
}

impl Default for AnalysisPipeline {
    fn default() -> Self {
        Self {
            ark_key: std::env::var("ARK_API_KEY").unwrap_or_default(),
            model: "doubao-pro-32k".into(),
        }
    }
}

impl AnalysisPipeline {
    pub fn run(&self, video_path: &str, title: &str) -> AnalysisJob {
        let video_id = Path::new(video_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("video")
            .to_string();
        let mut job = AnalysisJob {
            video_id,
            storage_path: video_path.into(),
            status: AnalysisStatus::Processing as i32,
            transcript: String::new(),
            report: String::new(),
            error: String::new(),
        };
        let wav = Path::new(video_path).with_extension("wav");
        extract_audio(video_path, &wav);
        job.transcript = if wav.exists() {
            format!(
                "[asr-placeholder] audio={} bytes={}",
                wav.file_name().unwrap_or_default().to_string_lossy(),
                std::fs::metadata(&wav).map(|m| m.len()).unwrap_or(0)
            )
        } else {
            String::new()
        };
        job.report = self.diagnose(&job.transcript, title);
        job.status = AnalysisStatus::Completed as i32;
        job
    }

    fn diagnose(&self, transcript: &str, title: &str) -> String {
        let clipped: String = transcript.chars().take(8000).collect();
        let prompt = format!(
            "你是直播复盘助手。根据口播和场次信息，输出：亮点、问题、话术改进、下播动作。\n标题:{title}\n口播:\n{clipped}"
        );
        if self.ark_key.is_empty() {
            return json!({
                "highlights": [],
                "issues": [],
                "script": [],
                "note": "no ark key",
                "prompt_chars": prompt.len(),
            })
            .to_string();
        }
        let body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": "你是直播复盘助手。"},
                {"role": "user", "content": prompt}
            ]
        });
        match ureq::post(ARK_CHAT)
            .set("Content-Type", "application/json")
            .set("Authorization", &format!("Bearer {}", self.ark_key))
            .send_json(body)
        {
            Ok(resp) => resp
                .into_json::<serde_json::Value>()
                .ok()
                .and_then(|v| {
                    v["choices"][0]["message"]["content"]
                        .as_str()
                        .map(|s| s.to_string())
                })
                .unwrap_or_else(|| "empty ark response".into()),
            Err(e) => e.to_string(),
        }
    }
}

fn extract_audio(video: &str, wav: &Path) {
    let _ = Command::new("ffmpeg")
        .args(["-y", "-hide_banner", "-loglevel", "error", "-i", video, "-vn", "-ac", "1", "-ar", "16000"])
        .arg(wav)
        .status();
}
