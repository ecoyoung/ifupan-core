use std::sync::Arc;

use clap::{Parser, Subcommand};
use ifupan_core::api::{self, App};
use ifupan_core::pipeline::Pipeline;

#[derive(Parser)]
#[command(name = "ifupan-core", about = "爱复盘核心流水线（Rust 复刻）")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 识别 URL 并探测是否在播
    Probe { url: String },
    /// 写入主播
    Add {
        url: String,
        #[arg(long)]
        detect: bool,
    },
    /// 列出主播
    List,
    /// 本地 REST（对齐 HttpListenerServer）
    Serve {
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        #[arg(long, default_value_t = 5001)]
        port: u16,
    },
    /// 常驻弹幕 WSS（打印 JSON 行）
    Barrage { url: String },
}

fn main() {
    let cli = Cli::parse();
    let pipe = Pipeline::default_paths().expect("open db");
    match cli.cmd {
        Cmd::Probe { url } => {
            let v = pipe.probe(&url).expect("probe");
            println!("{}", serde_json::to_string_pretty(&v).unwrap());
        }
        Cmd::Add { url, detect } => {
            let a = pipe.add(&url).expect("add");
            println!("{}", serde_json::to_string_pretty(&a).unwrap());
            if detect {
                pipe.scheduler.start_detection();
                eprintln!("detection started, ctrl+c to stop");
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
            }
        }
        Cmd::List => {
            let rows = pipe.store.list_anchors().expect("list");
            println!("{}", serde_json::to_string_pretty(&rows).unwrap());
        }
        Cmd::Serve { host, port } => {
            let app = Arc::new(App {
                store: pipe.store.clone(),
                scheduler: pipe.scheduler.clone(),
                token: std::sync::Mutex::new(String::new()),
            });
            api::serve(&host, port, app).expect("serve");
        }
        Cmd::Barrage { url } => {
            use ifupan_core::barrage::BarrageHub;
            use ifupan_core::barrage_ws::BarrageStart;
            use ifupan_core::detector::detect;
            let room = detect(&url);
            if room.room_id.is_empty() && room.wss_url.is_empty() {
                eprintln!("no room_id/wss_url: {}", room.error);
                std::process::exit(1);
            }
            let hub = BarrageHub::default();
            hub.start(
                BarrageStart {
                    anchor_id: 0,
                    room_id: room.room_id,
                    wss_url: room.wss_url,
                    video_id: "live".into(),
                    batch: "cli".into(),
                },
                |ev| {
                    println!("{}", serde_json::to_string(&ev).unwrap_or_default());
                },
            );
            eprintln!("barrage connected, ctrl+c to stop");
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
    }
}
