use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Clone)]
pub struct Config {
    pub template: String,
    pub host: String,
    pub port: u16,
    pub password: String,
    pub source: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            template: "{title}".into(),
            host: "127.0.0.1".into(),
            port: 4455,
            password: String::new(),
            source: "같이보기 화수".into(),
        }
    }
}

const FILE_NAME: &str = "chzzk-episode-overlay.json";

// 설정은 exe 옆에만 저장 (폴더째 옮기거나 지우면 끝)
fn path() -> Option<PathBuf> {
    // 테스트용 경로 지정
    if let Some(p) = std::env::var_os("CHZZK_EPISODE_OVERLAY_CONFIG") {
        return Some(p.into());
    }
    Some(std::env::current_exe().ok()?.parent()?.join(FILE_NAME))
}

// exe 폴더에 쓸 수 있는지 (Program Files 등이면 불가)
pub fn writable() -> bool {
    let Some(p) = path() else { return false };
    let probe = p.with_extension("tmp");
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

impl Config {
    // (설정, 설정 파일이 이미 있었는지)
    pub fn load() -> (Config, bool) {
        let d = Config::default();
        let Some(text) = path().and_then(|p| std::fs::read_to_string(p).ok()) else {
            return (d, false);
        };
        // 메모장 등이 붙이는 BOM 무시
        let v: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).unwrap_or_default();
        let s = |k: &str, def: &str| v[k].as_str().unwrap_or(def).to_string();
        let c = Config {
            template: s("template", &d.template),
            host: s("host", &d.host),
            port: v["port"].as_u64().map_or(d.port, |p| p as u16),
            password: s("password", &d.password),
            source: s("source", &d.source),
        };
        (c, true)
    }

    pub fn save(&self) {
        let v = json!({
            "template": self.template,
            "host": self.host,
            "port": self.port,
            "password": self.password,
            "source": self.source,
        });
        if let Some(p) = path() {
            let _ = std::fs::write(p, serde_json::to_string_pretty(&v).unwrap_or_default());
        }
    }
}
