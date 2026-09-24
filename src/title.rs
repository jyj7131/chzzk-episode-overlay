// 창 제목: "치지직 같이보기 - 작품명 1화 - CHZZK - Chrome"
// (브라우저마다 " - CHZZK" 뒤에 붙는 부분이 달라서 앞뒤 표식만 봄)

const PREFIX: &str = "치지직 같이보기 - ";
const MARK: &str = " - CHZZK";

pub fn extract(window_title: &str) -> Option<String> {
    let rest = window_title.strip_prefix(PREFIX)?;
    let end = rest.rfind(MARK)?;
    let raw = rest[..end].trim();
    (!raw.is_empty()).then(|| raw.to_string())
}

// "작품명 1화" -> ("작품명", "1")
pub fn parse_episode(raw: &str) -> (String, String) {
    let chars: Vec<(usize, char)> = raw.char_indices().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].1.is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].1.is_ascii_digit() || chars[i].1 == '.') {
                i += 1;
            }
            let mut j = i;
            while j < chars.len() && chars[j].1.is_whitespace() {
                j += 1;
            }
            if j < chars.len() && chars[j].1 == '화' {
                let (s, e) = (chars[start].0, chars.get(i).map_or(raw.len(), |c| c.0));
                return (raw[..s].trim().to_string(), raw[s..e].to_string());
            }
        } else {
            i += 1;
        }
    }
    (raw.to_string(), String::new())
}

pub fn format(template: &str, raw: &str) -> String {
    let (name, ep) = parse_episode(raw);
    template.replace("{title}", raw).replace("{name}", &name).replace("{ep}", &ep)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_titles() {
        assert_eq!(extract("치지직 같이보기 - 작품명 1화 - CHZZK - Chrome").as_deref(), Some("작품명 1화"));
        assert_eq!(extract("치지직 같이보기 - A - B 2화 - CHZZK - Whale").as_deref(), Some("A - B 2화"));
        assert_eq!(extract("치지직 - CHZZK - Chrome"), None);
        assert_eq!(extract("YouTube - Chrome"), None);
    }

    #[test]
    fn episodes() {
        assert_eq!(parse_episode("작품명 1화"), ("작품명".into(), "1".into()));
        assert_eq!(parse_episode("제목 86 3화"), ("제목 86".into(), "3".into()));
        assert_eq!(parse_episode("극장판"), ("극장판".into(), "".into()));
        assert_eq!(format("현재 {ep}화", "작품명 12화"), "현재 12화");
        assert_eq!(format("{name} | EP.{ep}", "작품명 1화"), "작품명 | EP.1");
    }
}
