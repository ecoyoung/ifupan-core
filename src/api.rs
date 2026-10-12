use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

use serde_json::{json, Value};

use crate::analysis::AnalysisPipeline;
use crate::crypto::machine_code;
use crate::db::Store;
use crate::scheduler::Scheduler;

pub struct App {
    pub store: Arc<Store>,
    pub scheduler: Arc<Scheduler>,
    pub token: std::sync::Mutex<String>,
}

impl App {
    pub fn handle(&self, method: &str, path: &str, query: &HashMap<String, String>, body: Value) -> Value {
        match (method, path) {
            ("GET", "/api/system/getMachineCode") => ok(json!(machine_code("cpu", "disk", "board"))),
            ("GET", "/api/config/getuserinfo") => ok(json!({
                "username": "local",
                "token": self.token.lock().unwrap().clone(),
                "packageLevel": 99,
                "expirationDate": "2099-12-31T23:59:59",
                "status": 0
            })),
            ("GET", "/api/config/settoken") => {
                *self.token.lock().unwrap() = query.get("token").cloned().unwrap_or_default();
                ok(json!(true))
            }
            ("GET", "/api/config/getClientVersionConfig") => ok(json!({
                "clientVersion": "2.6.3.1",
                "isVersionSelect": 1,
                "userType": 0
            })),
            ("GET", "/api/anchorinfo/GetPageAnchor") => {
                match self.store.list_anchors() {
                    Ok(v) => ok(json!(v)),
                    Err(e) => err(500, &e.to_string()),
                }
            }
            ("POST", "/api/anchorinfo/AddOrUpdateAnchor") => {
                let url = body
                    .get("url")
                    .or_else(|| body.get("live_url"))
                    .or_else(|| body.get("source_url"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if url.is_empty() {
                    return err(1, "url required");
                }
                let auto = body.get("is_auto_record").and_then(|v| v.as_i64()).unwrap_or(1);
                match self.scheduler.add_anchor(url, auto, 1) {
                    Ok(a) => ok(json!(a)),
                    Err(e) => err(1, &e),
                }
            }
            ("GET", "/api/anchorinfo/Decector") => {
                self.scheduler.start_detection();
                ok(json!(true))
            }
            ("GET", "/api/anchorinfo/StopDecector") => {
                self.scheduler.stop_detection();
                ok(json!(true))
            }
            ("GET", "/api/anchorinfo/StartRecord") => {
                let id = query.get("id").and_then(|s| s.parse().ok()).unwrap_or(0);
                match self.scheduler.start_record(id) {
                    Ok(v) => ok(v),
                    Err(e) => err(1, &e),
                }
            }
            ("GET", "/api/anchorinfo/StopRecord") => {
                let id = query.get("id").and_then(|s| s.parse().ok()).unwrap_or(0);
                match self.scheduler.stop_record(id) {
                    Ok(v) => ok(v),
                    Err(e) => err(1, &e),
                }
            }
            ("GET", "/api/anchorinfo/RemoveAnchor") => {
                let id = query.get("id").and_then(|s| s.parse().ok()).unwrap_or(0);
                match self.store.delete_anchor(id) {
                    Ok(()) => ok(json!(true)),
                    Err(e) => err(1, &e.to_string()),
                }
            }
            ("GET", "/api/anchorvideo/list") => {
                let aid = query.get("anchor_id").and_then(|s| s.parse().ok());
                match self.store.list_videos(aid) {
                    Ok(v) => ok(json!(v)),
                    Err(e) => err(1, &e.to_string()),
                }
            }
            ("POST", "/api/anchorvideo/CreateAnalysis") => {
                let path = body.get("storage_path").and_then(|v| v.as_str()).unwrap_or("");
                if path.is_empty() {
                    return err(1, "storage_path required");
                }
                let title = body.get("title").and_then(|v| v.as_str()).unwrap_or("");
                let job = AnalysisPipeline::default().run(path, title);
                ok(json!(job))
            }
            _ => json!({"code": -9, "msg": "资源不存在", "data": null}),
        }
    }
}

fn ok(data: Value) -> Value {
    json!({"code": 0, "msg": "ok", "data": data})
}

fn err(code: i64, msg: &str) -> Value {
    json!({"code": code, "msg": msg, "data": null})
}

pub fn serve(host: &str, port: u16, app: Arc<App>) -> std::io::Result<()> {
    let listener = TcpListener::bind((host, port))?;
    eprintln!("http://{host}:{port}");
    for stream in listener.incoming() {
        let app = app.clone();
        if let Ok(stream) = stream {
            thread::spawn(move || {
                let _ = handle_conn(stream, &app);
            });
        }
    }
    Ok(())
}

fn handle_conn(mut stream: TcpStream, app: &App) -> std::io::Result<()> {
    let mut buf = [0u8; 8192];
    let n = stream.read(&mut buf)?;
    let req = String::from_utf8_lossy(&buf[..n]);
    let mut lines = req.split("\r\n");
    let first = lines.next().unwrap_or("");
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let mut headers = HashMap::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_lowercase(), v.trim().to_string());
        }
    }
    let body_start = req.find("\r\n\r\n").map(|i| i + 4).unwrap_or(req.len());
    let body_raw = &req[body_start.min(req.len())..];
    let body: Value = serde_json::from_str(body_raw).unwrap_or(json!({}));
    let (path, query) = split_query(&target);
    let result = app.handle(&method, &path, &query, body);
    let payload = serde_json::to_vec(&result).unwrap_or_else(|_| b"{}".to_vec());
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    stream.write_all(resp.as_bytes())?;
    stream.write_all(&payload)?;
    Ok(())
}

fn split_query(target: &str) -> (String, HashMap<String, String>) {
    match target.split_once('?') {
        Some((path, qs)) => {
            let mut map = HashMap::new();
            for pair in qs.split('&') {
                if let Some((k, v)) = pair.split_once('=') {
                    map.insert(k.to_string(), v.to_string());
                }
            }
            (path.to_string(), map)
        }
        None => (target.to_string(), HashMap::new()),
    }
}
