use std::path::Path;
use std::process::Command;

use serde_json::json;

use crate::models::{AnalysisJob, AnalysisStatus, DanmuEvent};

const ARK_CHAT: &str = "https://ark.cn-beijing.volces.com/api/v3/chat/completions";

pub struct AnalysisPipeline {
    pub ark_key: String,
    pub model: String,
}

impl Default for AnalysisPipeline {
    fn default() -> Self {
        Self {
            ark_key: std::env::var("ARK_API_KEY").unwrap_or_default(),
            model: std::env::var("ARK_MODEL").unwrap_or_else(|_| "doubao-pro-32k".into()),
        }
    }
}

impl AnalysisPipeline {
    pub fn run(&self, video_path: &str, title: &str) -> AnalysisJob {
        self.run_with_danmu(video_path, title, &[])
    }

    pub fn run_with_danmu(&self, video_path: &str, title: &str, danmu: &[DanmuEvent]) -> AnalysisJob {
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
        if Path::new(video_path).exists() {
            extract_audio(video_path, &wav);
        }
        let extra = load_sidecar_danmu(video_path);
        let events: Vec<DanmuEvent> = if danmu.is_empty() { extra } else { danmu.to_vec() };
        job.transcript = transcribe(&wav);
        let stats = crate::webcast::danmu_stats(&events);
        let local = local_report(title, &job.transcript, &stats, &events);
        let llm = self.diagnose(&job.transcript, title, &stats);
        job.report = json!({
            "title": title,
            "stats": stats,
            "local": local,
            "llm": llm,
        })
        .to_string();
        if job.transcript.is_empty() && events.is_empty() {
            job.error = "no transcript and no danmu".into();
            job.status = AnalysisStatus::Failed as i32;
        } else {
            job.status = AnalysisStatus::Completed as i32;
        }
        job
    }

    fn diagnose(&self, transcript: &str, title: &str, stats: &serde_json::Value) -> serde_json::Value {
        if self.ark_key.is_empty() {
            return json!({"note": "no ARK_API_KEY"});
        }
        let clipped: String = transcript.chars().take(8000).collect();
        let prompt = format!(
            "你是直播复盘助手。根据口播、弹幕统计，输出 JSON：highlights[], issues[], script[], next_actions[]。\n标题:{title}\n统计:{stats}\n口播:\n{clipped}"
        );
        let body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": "只输出 JSON。"},
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
                        .map(|s| serde_json::from_str::<serde_json::Value>(s).unwrap_or(json!({"text": s})))
                })
                .unwrap_or(json!({"note": "empty ark"})),
            Err(e) => json!({"error": e.to_string()}),
        }
    }
}

fn transcribe(wav: &Path) -> String {
    if !wav.exists() {
        return String::new();
    }
    let bins = [
        std::env::var("WHISPER_BIN").unwrap_or_default(),
        "whisper-cli".into(),
        "whisper".into(),
        "whisper.cpp".into(),
    ];
    for bin in bins {
        if bin.is_empty() {
            continue;
        }
        let txt = wav.with_extension("txt");
        let st = Command::new(&bin)
            .args(["-l", "zh", "-f"])
            .arg(wav)
            .arg("-otxt")
            .arg("-of")
            .arg(wav.with_extension(""))
            .status();
        if st.map(|s| s.success()).unwrap_or(false) && txt.exists() {
            return std::fs::read_to_string(txt).unwrap_or_default();
        }
        let out = Command::new(&bin)
            .args(["--language", "zh", "--model", "base", "--output_format", "txt"])
            .arg(wav)
            .output();
        if let Ok(o) = out {
            if o.status.success() {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if !s.is_empty() {
                    return s;
                }
            }
        }
    }
    ffmpeg_silence_sketch(wav)
}

fn ffmpeg_silence_sketch(wav: &Path) -> String {
    let out = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-i",
            &wav.to_string_lossy(),
            "-af",
            "silencedetect=noise=-30dB:d=0.6",
            "-f",
            "null",
            "-",
        ])
        .output();
    let Ok(o) = out else {
        return format!(
            "[audio] {} bytes",
            std::fs::metadata(wav).map(|m| m.len()).unwrap_or(0)
        );
    };
    let log = String::from_utf8_lossy(&o.stderr);
    let mut segs = Vec::new();
    for line in log.lines() {
        if let Some(i) = line.find("silence_end:") {
            segs.push(line[i..].trim().to_string());
        }
    }
    if segs.is_empty() {
        format!(
            "[audio] {} bytes (no silence map)",
            std::fs::metadata(wav).map(|m| m.len()).unwrap_or(0)
        )
    } else {
        format!("[speech-windows]\n{}", segs.into_iter().take(80).collect::<Vec<_>>().join("\n"))
    }
}

fn extract_audio(video: &str, wav: &Path) {
    let _ = Command::new("ffmpeg")
        .args(["-y", "-hide_banner", "-loglevel", "error", "-i", video, "-vn", "-ac", "1", "-ar", "16000"])
        .arg(wav)
        .status();
}

fn load_sidecar_danmu(video_path: &str) -> Vec<DanmuEvent> {
    let p = Path::new(video_path).with_extension("danmu.jsonl");
    let Ok(text) = std::fs::read_to_string(p) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .map(|v| crate::barrage::ingest(&v, "video", "sidecar"))
        .collect()
}

fn local_report(title: &str, transcript: &str, stats: &serde_json::Value, events: &[DanmuEvent]) -> serde_json::Value {
    let chat = stats.get("chat").and_then(|v| v.as_u64()).unwrap_or(0);
    let gift = stats.get("gift").and_then(|v| v.as_u64()).unwrap_or(0);
    let peak = stats.get("peak_online").and_then(|v| v.as_i64()).unwrap_or(0);
    let mut highlights = Vec::new();
    let mut issues = Vec::new();
    if gift > 0 {
        highlights.push(format!("礼物 {gift} 笔"));
    }
    if peak > 0 {
        highlights.push(format!("在线峰值 {peak}"));
    }
    if chat == 0 {
        issues.push("几乎没有评论文本，检查弹幕解析或场次是否过冷".to_string());
    }
    if transcript.is_empty() {
        issues.push("没有口播转写：安装 whisper 或设置 WHISPER_BIN".to_string());
    }
    let top: Vec<String> = events
        .iter()
        .filter(|e| e.msg_type == 1 && !e.content.is_empty())
        .take(12)
        .map(|e| format!("{}: {}", e.nick_name, e.content))
        .collect();
    json!({
        "title": title,
        "highlights": highlights,
        "issues": issues,
        "sample_chat": top,
        "next_actions": [
            "把高频评论做成固定话术卡",
            "礼物峰值前后 60 秒单独切片复盘"
        ],
    })
}
