// OBS WebSocket v5. 요청 하나마다 접속 → 인증 → 요청 → 종료.
// 외부 WebSocket 라이브러리 없이 필요한 부분만 직접 구현 (텍스트 프레임, 비TLS).

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

#[derive(Clone)]
pub struct Cfg {
    pub host: String,
    pub port: u16,
    pub password: String,
    pub source: String,
}

const CANT_CONNECT: &str = "OBS에 연결할 수 없음 (OBS 실행 / WebSocket 서버 사용 확인)";

fn sha_b64(s: &str) -> String {
    B64.encode(Sha256::digest(s.as_bytes()))
}

pub fn make_auth(password: &str, salt: &str, challenge: &str) -> String {
    sha_b64(&(sha_b64(&format!("{password}{salt}")) + challenge))
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    for chunk in out.chunks_mut(8) {
        let mut h = RandomState::new().build_hasher();
        h.write_usize(chunk.as_ptr() as usize);
        let r = h.finish().to_le_bytes();
        chunk.copy_from_slice(&r[..chunk.len()]);
    }
    out
}

fn io_err(e: io::Error) -> String {
    match e.kind() {
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => "OBS 응답 시간 초과".into(),
        _ => CANT_CONNECT.into(),
    }
}

enum Msg {
    Text(String),
    Close(u16),
}

struct Ws(TcpStream);

impl Ws {
    fn connect(host: &str, port: u16) -> Result<Ws, String> {
        let addrs = (host, port).to_socket_addrs().map_err(|_| "서버 IP 형식이 올바르지 않음".to_string())?;
        let stream = addrs
            .into_iter()
            .find_map(|a| TcpStream::connect_timeout(&a, Duration::from_secs(3)).ok())
            .ok_or_else(|| CANT_CONNECT.to_string())?;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
        let _ = stream.set_nodelay(true);
        let mut ws = Ws(stream);

        let key = B64.encode(random_bytes::<16>());
        let req = format!(
            "GET / HTTP/1.1\r\nHost: {host}:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
             Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        );
        ws.0.write_all(req.as_bytes()).map_err(io_err)?;

        // 응답 헤더 끝(\r\n\r\n)까지 읽기
        let mut head = Vec::new();
        let mut b = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            if head.len() > 4096 || ws.0.read(&mut b).map_err(io_err)? == 0 {
                return Err(CANT_CONNECT.into());
            }
            head.push(b[0]);
        }
        if !head.starts_with(b"HTTP/1.1 101") {
            return Err(CANT_CONNECT.into());
        }
        Ok(ws)
    }

    fn send_frame(&mut self, opcode: u8, payload: &[u8]) -> io::Result<()> {
        let mut f = vec![0x80 | opcode];
        let len = payload.len();
        if len < 126 {
            f.push(0x80 | len as u8);
        } else if len < 65536 {
            f.push(0x80 | 126);
            f.extend_from_slice(&(len as u16).to_be_bytes());
        } else {
            f.push(0x80 | 127);
            f.extend_from_slice(&(len as u64).to_be_bytes());
        }
        let mask = random_bytes::<4>();
        f.extend_from_slice(&mask);
        f.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        self.0.write_all(&f)
    }

    fn send_json(&mut self, v: &Value) -> Result<(), String> {
        self.send_frame(0x1, v.to_string().as_bytes()).map_err(io_err)
    }

    fn read_exact(&mut self, n: usize) -> Result<Vec<u8>, String> {
        let mut buf = vec![0u8; n];
        self.0.read_exact(&mut buf).map_err(|e| {
            if e.kind() == io::ErrorKind::UnexpectedEof { CANT_CONNECT.to_string() } else { io_err(e) }
        })?;
        Ok(buf)
    }

    fn recv(&mut self) -> Result<Msg, String> {
        let mut data = Vec::new();
        loop {
            let h = self.read_exact(2)?;
            let (fin, opcode) = (h[0] & 0x80 != 0, h[0] & 0x0f);
            let mut len = (h[1] & 0x7f) as u64;
            if len == 126 {
                len = u16::from_be_bytes(self.read_exact(2)?.try_into().unwrap()) as u64;
            } else if len == 127 {
                len = u64::from_be_bytes(self.read_exact(8)?.try_into().unwrap());
            }
            let mask = if h[1] & 0x80 != 0 { Some(self.read_exact(4)?) } else { None };
            let mut payload = self.read_exact(len as usize)?;
            if let Some(m) = mask {
                payload.iter_mut().enumerate().for_each(|(i, b)| *b ^= m[i % 4]);
            }
            match opcode {
                0x8 => {
                    let code = if payload.len() >= 2 { u16::from_be_bytes([payload[0], payload[1]]) } else { 1005 };
                    return Ok(Msg::Close(code));
                }
                0x9 => {
                    let _ = self.send_frame(0xA, &payload);
                }
                0x0 | 0x1 => {
                    data.extend_from_slice(&payload);
                    if fin {
                        return Ok(Msg::Text(String::from_utf8_lossy(&data).into_owned()));
                    }
                }
                _ => {}
            }
        }
    }

    fn close(mut self) {
        let _ = self.send_frame(0x8, &1000u16.to_be_bytes());
    }
}

pub fn request(cfg: &Cfg, request_type: &str, request_data: Value) -> Result<Value, String> {
    let mut ws = Ws::connect(&cfg.host, cfg.port)?;
    loop {
        match ws.recv()? {
            Msg::Close(4009) => return Err("OBS WebSocket 비밀번호가 틀림".into()),
            Msg::Close(code) => return Err(format!("OBS 연결이 끊김 ({code})")),
            Msg::Text(t) => {
                let v: Value = serde_json::from_str(&t).unwrap_or_default();
                let d = &v["d"];
                match v["op"].as_i64() {
                    Some(0) => {
                        let mut identify = json!({ "rpcVersion": 1, "eventSubscriptions": 0 });
                        if let Some(a) = d.get("authentication") {
                            if cfg.password.is_empty() {
                                return Err("OBS에 비밀번호가 설정되어 있음 — 비밀번호 입력 필요".into());
                            }
                            let salt = a["salt"].as_str().unwrap_or("");
                            let challenge = a["challenge"].as_str().unwrap_or("");
                            identify["authentication"] = make_auth(&cfg.password, salt, challenge).into();
                        }
                        ws.send_json(&json!({ "op": 1, "d": identify }))?;
                    }
                    Some(2) => {
                        ws.send_json(&json!({ "op": 6, "d": {
                            "requestType": request_type, "requestId": "r1", "requestData": request_data,
                        }}))?;
                    }
                    Some(7) => {
                        let st = &d["requestStatus"];
                        let result = if st["result"].as_bool() == Some(true) {
                            Ok(d.get("responseData").cloned().unwrap_or(json!({})))
                        } else if st["code"].as_i64() == Some(600) {
                            Err(format!("OBS에 '{}' 소스가 없음", cfg.source))
                        } else {
                            let comment = st["comment"].as_str().map(|c| format!(": {c}")).unwrap_or_default();
                            Err(format!("OBS 오류 {}{comment}", st["code"]))
                        };
                        ws.close();
                        return result;
                    }
                    _ => {}
                }
            }
        }
    }
}

pub fn set_text(cfg: &Cfg, text: &str) -> Result<(), String> {
    request(cfg, "SetInputSettings", json!({
        "inputName": cfg.source, "inputSettings": { "text": text }, "overlay": true,
    }))
    .map(|_| ())
}

// 연결 + 텍스트 소스인지 확인
pub fn test(cfg: &Cfg) -> Result<(), String> {
    let res = request(cfg, "GetInputSettings", json!({ "inputName": cfg.source }))?;
    let kind = res["inputKind"].as_str().unwrap_or("");
    if kind.starts_with("text_") {
        Ok(())
    } else {
        Err(format!("'{}'는 텍스트 소스가 아님 ({kind})", cfg.source))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn auth_matches_protocol_docs() {
        assert_eq!(
            super::make_auth(
                "supersecretpassword",
                "lM1GncleQOaCu9lT1yeUZhFYnqhsLLP1G5lAGo3ixaI=",
                "+IxH4CnCiqpX1rM9scsNynZzbOe4KhDeYcTNS3PDaeY="
            ),
            "1Ct943GAT+6YQUUX47Ia/ncufilbe6+oD6lY+5kaCu4="
        );
    }
}
