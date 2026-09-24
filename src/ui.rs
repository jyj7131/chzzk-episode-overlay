// 확장 팝업과 같은 구성의 창 (어두운 테마, 제목 설정 카드 + 스크롤해야 보이는 OBS 연결 카드)
// 창을 닫으면 프로그램 종료.

use crate::{title, wide, with, App, Conn, APP_NAME, ICON, VERSION, WIN, WINDOW_CLASS, WM_APP_STATUS};
use std::cell::{Cell, RefCell};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Dwm::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

// ── 색 (확장 popup.css와 동일, BGR) ──
const BG: u32 = 0x171514;
const PANEL: u32 = 0x221f1e;
const LINE: u32 = 0x35302e;
const TEXT: u32 = 0xece9e8;
const MUTED: u32 = 0x988f8b;
const ACCENT: u32 = 0xa3ff00;
const INK: u32 = 0x131a0b;
const BAD: u32 = 0x6b6bff;

// ── 컨트롤 ID ──
const ID_H1: i32 = 300;
const ID_VER: i32 = 301;
const ID_L_RAW: i32 = 310;
const ID_RAW: i32 = 311;
const ID_L_PREVIEW: i32 = 312;
const ID_PREVIEW: i32 = 313;
const ID_L_VARS: i32 = 314;
const ID_VAR: i32 = 320; // 320..323
const ID_L_TPL: i32 = 324;
const ID_PRESET: i32 = 330; // 330..333
const ID_L_CUSTOM: i32 = 334;
const ID_TEMPLATE: i32 = 335;
const ID_OBSHEAD: i32 = 340;
const ID_BADGE: i32 = 341;
const ID_L_HOST: i32 = 342;
const ID_L_PORT: i32 = 343;
const ID_HOST: i32 = 344;
const ID_PORT: i32 = 345;
const ID_L_PW: i32 = 346;
const ID_SHOWPW: i32 = 347;
const ID_PASSWORD: i32 = 348;
const ID_L_SOURCE: i32 = 349;
const ID_SOURCE: i32 = 350;
const ID_CONNECT: i32 = 351;
const ID_CONNERR: i32 = 352;
const ID_L_HELP: i32 = 353;
const ID_HELP: i32 = 354;
const ID_NOTICE: i32 = 355;

const NOTICE_TEXT: &str = "비공식 프로그램 · 네이버/치지직과 무관\r\n\
창 제목 중 '치지직 같이보기'만 읽으며, 입력한 OBS로만 전송합니다.\r\n\
정보 수집·기록 없음";
const NOTICE_H: i32 = 48;

const VARS: [(&str, &str); 3] = [("{title}", "제목 전체"), ("{name}", "작품명"), ("{ep}", "화수 숫자")];
const PRESETS: [(&str, &str, i32, i32); 3] = [
    ("제목 그대로", "{title}", 24, 84),
    ("N화", "{ep}화", 112, 44),
    ("현재 N화", "현재 {ep}화", 160, 72),
];
const EXAMPLE: &str = "신세기 에반게리온 1화";
const HELP_TEXT: &str = "1. OBS → 도구 → WebSocket 서버 설정 → WebSocket 서버 사용 체크\r\n\
2. [서버 정보 표시] 버튼을 눌러 나오는 서버 IP / 포트 / 비밀번호를 입력\r\n   (원컴 환경에선 서버 IP 127.0.0.1 그대로 사용 가능)\r\n\
3. OBS 쪽에 텍스트(GDI+) 소스를 추가하고, 소스 이름을 위 \"텍스트 소스 이름\"과 똑같이 세팅\r\n\
4. [연결하기] 버튼 클릭\r\n\r\n\
투컴에서 연결이 안 되면\r\n\
• 송출컴 Windows 방화벽에서 OBS 허용\r\n\
• OBS가 보여주는 IP는 추정값이라 틀릴 수 있음 → 송출컴에서 ipconfig로 IPv4 주소 확인";
const HELP_H: i32 = 196;

const WIDTH: i32 = 364;
// 처음 보이는 높이: 제목 설정 카드까지 + OBS 연결 제목 한 줄만 살짝
const VISIBLE_H: i32 = 376 + 10 + 34;

// ── 화면에 보여줄 상태 스냅샷 ──
#[derive(Clone, Default)]
pub(crate) struct View {
    raw: Option<String>,
    preview: String,
    template: String,
    conn: Conn,
    host: String,
    port: u16,
    password: String,
    source: String,
}

impl View {
    pub fn from(a: &App) -> View {
        let preview = if a.raw.is_some() {
            let o = a.output();
            if o.is_empty() { "(빈 텍스트)".into() } else { o }
        } else {
            "—".into()
        };
        View {
            raw: a.raw.clone(),
            preview,
            template: a.cfg.template.clone(),
            conn: a.conn.clone(),
            host: a.cfg.host.clone(),
            port: a.cfg.port,
            password: a.cfg.password.clone(),
            source: a.cfg.source.clone(),
        }
    }
}

thread_local! {
    static VIEW: RefCell<View> = RefCell::new(View::default());
    static DPI: Cell<i32> = const { Cell::new(96) };
    static SCROLL: Cell<i32> = const { Cell::new(0) };
    static FILLING: Cell<bool> = const { Cell::new(false) };
    static PW_SHOWN: Cell<bool> = const { Cell::new(false) };
    static FOCUS: Cell<i32> = const { Cell::new(0) };
    static CARDS: RefCell<Vec<RECT>> = const { RefCell::new(Vec::new()) };
    static BOXES: RefCell<Vec<(i32, RECT)>> = const { RefCell::new(Vec::new()) };
    static FONTS: RefCell<Vec<HFONT>> = const { RefCell::new(Vec::new()) };
    static BR_BG: Cell<HBRUSH> = const { Cell::new(null_mut()) };
    static BR_PANEL: Cell<HBRUSH> = const { Cell::new(null_mut()) };
}

// 글꼴 번호
const F_BODY: usize = 0; // 13px
const F_LABEL: usize = 1; // 11px
const F_H1: usize = 2; // 15px 굵게
const F_H2: usize = 3; // 12px 굵게
const F_BIG: usize = 4; // 18px 굵게
const F_CODE: usize = 5; // 12px 고정폭
const F_CHIP: usize = 6; // 12px
const F_MINI: usize = 7; // 10px
const F_BOLD: usize = 8; // 13px 굵게

fn font(i: usize) -> HFONT {
    FONTS.with(|f| f.borrow()[i])
}

fn px(v: i32) -> i32 {
    v * DPI.get() / 96
}

fn rect(l: i32, t: i32, r: i32, b: i32) -> RECT {
    RECT { left: l, top: t, right: r, bottom: b }
}

fn item(id: i32) -> HWND {
    unsafe { GetDlgItem(WIN.get(), id) }
}

fn ctrl_text(id: i32) -> String {
    unsafe {
        let h = item(id);
        let mut buf = vec![0u16; (GetWindowTextLengthW(h) + 1) as usize];
        let n = GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32);
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }
}

fn set_text(id: i32, s: &str) {
    unsafe { SetWindowTextW(item(id), wide(s).as_ptr()) };
}

fn read_obs_form() -> (String, u16, String, String) {
    let host = ctrl_text(ID_HOST).trim().to_string();
    (
        if host.is_empty() { "127.0.0.1".into() } else { host },
        ctrl_text(ID_PORT).trim().parse().unwrap_or(4455),
        ctrl_text(ID_PASSWORD),
        ctrl_text(ID_SOURCE).trim().to_string(),
    )
}

// 입력칸이 저장된 값과 다르면 아직 적용 안 된 상태
fn obs_dirty(v: &View) -> bool {
    let (h, p, pw, s) = read_obs_form();
    h != v.host || p != v.port || pw != v.password || s != v.source
}

// ── 초기화 ──
pub(crate) fn register(hinst: HINSTANCE) {
    unsafe {
        let hdc = GetDC(null_mut());
        DPI.set(GetDeviceCaps(hdc, 90)); // LOGPIXELSY
        ReleaseDC(null_mut(), hdc);

        let (malgun, mono) = (wide("Malgun Gothic"), wide("Consolas"));
        let mk = |size: i32, weight: i32, face: &[u16]| CreateFontW(-px(size), 0, 0, 0, weight, 0, 0, 0, 1, 0, 0, 5, 0, face.as_ptr());
        let fonts = vec![
            mk(13, 400, &malgun),
            mk(11, 400, &malgun),
            mk(15, 700, &malgun),
            mk(12, 700, &malgun),
            mk(18, 700, &malgun),
            mk(12, 400, &mono),
            mk(12, 400, &malgun),
            mk(10, 400, &malgun),
            mk(13, 700, &malgun),
        ];
        FONTS.with(|f| *f.borrow_mut() = fonts);
        BR_BG.set(CreateSolidBrush(BG));
        BR_PANEL.set(CreateSolidBrush(PANEL));

        let cls = wide(WINDOW_CLASS);
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: ICON.get(),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: cls.as_ptr(),
        };
        RegisterClassW(&wc);
    }
}

pub(crate) fn create(hinst: HINSTANCE) -> HWND {
    unsafe {
        let style = 0x00C0_0000 | 0x0008_0000 | 0x0002_0000 | 0x0020_0000 | 0x0200_0000; // CAPTION SYSMENU MINIMIZEBOX VSCROLL CLIPCHILDREN
        let mut rc = rect(0, 0, px(WIDTH), px(VISIBLE_H));
        AdjustWindowRect(&mut rc, style, 0);
        let w = rc.right - rc.left + GetSystemMetrics(2); // + SM_CXVSCROLL
        let win = CreateWindowExW(
            0, wide(WINDOW_CLASS).as_ptr(), wide(APP_NAME).as_ptr(), style,
            CW_USEDEFAULT, CW_USEDEFAULT, w, rc.bottom - rc.top, null_mut(), null_mut(), hinst, null(),
        );
        WIN.set(win);

        // 어두운 제목 표시줄 / 스크롤바
        let dark: i32 = 1;
        DwmSetWindowAttribute(win, 20, &dark as *const i32 as _, 4);
        SetWindowTheme(win, wide("DarkMode_Explorer").as_ptr(), null());

        create_controls(win, hinst);
        win
    }
}

// 저장된 설정으로 입력칸 채우기 (이때 생기는 변경 알림은 무시)
pub(crate) fn fill_form() {
    FILLING.set(true);
    if let Some((tpl, host, port, pw, src)) =
        with(|a| (a.cfg.template.clone(), a.cfg.host.clone(), a.cfg.port, a.cfg.password.clone(), a.cfg.source.clone()))
    {
        set_text(ID_TEMPLATE, &tpl);
        set_text(ID_HOST, &host);
        set_text(ID_PORT, &port.to_string());
        set_text(ID_PASSWORD, &pw);
        set_text(ID_SOURCE, &src);
    }
    FILLING.set(false);
}

pub(crate) fn show(win: HWND) {
    unsafe {
        ShowWindow(win, 5); // SW_SHOW
        SetForegroundWindow(win);
    }
    crate::refresh_ui();
}

unsafe fn create_controls(win: HWND, hinst: HINSTANCE) {
    const CHILD: u32 = 0x4000_0000 | 0x1000_0000; // WS_CHILD | WS_VISIBLE
    const TAB: u32 = 0x0001_0000;
    const STATIC_LINE: u32 = CHILD | 0x80 | 0x4000; // SS_NOPREFIX | SS_ENDELLIPSIS
    const STATIC_WRAP: u32 = CHILD | 0x80;
    const OWNERDRAW: u32 = CHILD | TAB | 0xB;
    const EDIT: u32 = CHILD | TAB | 0x80; // ES_AUTOHSCROLL

    let make = |class: &str, text: &str, style: u32, id: i32, f: usize| {
        let h = CreateWindowExW(0, wide(class).as_ptr(), wide(text).as_ptr(), style, 0, 0, 0, 0, win, id as isize as _, hinst, null());
        SendMessageW(h, 0x30, font(f) as usize, 0); // WM_SETFONT
    };
    make("STATIC", APP_NAME, STATIC_LINE, ID_H1, F_H1);
    make("STATIC", &format!("v{VERSION}"), STATIC_LINE, ID_VER, F_LABEL);

    make("STATIC", "감지된 제목", STATIC_LINE, ID_L_RAW, F_LABEL);
    make("STATIC", "", STATIC_LINE, ID_RAW, F_BODY);
    make("STATIC", "OBS 출력", STATIC_LINE, ID_L_PREVIEW, F_LABEL);
    make("STATIC", "", STATIC_LINE, ID_PREVIEW, F_BIG);
    make("STATIC", "변수    누르면 아래 입력칸에 추가", STATIC_LINE, ID_L_VARS, F_LABEL);
    for i in 0..VARS.len() as i32 {
        make("BUTTON", "", OWNERDRAW, ID_VAR + i, F_CHIP);
    }
    make("STATIC", "템플릿", STATIC_LINE, ID_L_TPL, F_LABEL);
    for i in 0..PRESETS.len() as i32 {
        make("BUTTON", "", OWNERDRAW, ID_PRESET + i, F_CHIP);
    }
    make("STATIC", "직접 작성", STATIC_LINE, ID_L_CUSTOM, F_LABEL);
    make("EDIT", "", EDIT | 0x4 | 0x40 | 0x1000, ID_TEMPLATE, F_BODY); // MULTILINE | AUTOVSCROLL | WANTRETURN

    make("STATIC", "OBS 연결", STATIC_LINE, ID_OBSHEAD, F_H2);
    make("STATIC", "", STATIC_LINE, ID_BADGE, F_LABEL);
    make("STATIC", "서버 IP", STATIC_LINE, ID_L_HOST, F_LABEL);
    make("STATIC", "서버 포트", STATIC_LINE, ID_L_PORT, F_LABEL);
    make("EDIT", "", EDIT, ID_HOST, F_BODY);
    make("EDIT", "", EDIT | 0x2000, ID_PORT, F_BODY); // ES_NUMBER
    make("STATIC", "서버 비밀번호", STATIC_LINE, ID_L_PW, F_LABEL);
    make("BUTTON", "", OWNERDRAW, ID_SHOWPW, F_MINI);
    make("EDIT", "", EDIT | 0x20, ID_PASSWORD, F_BODY); // ES_PASSWORD
    make("STATIC", "텍스트 소스 이름", STATIC_LINE, ID_L_SOURCE, F_LABEL);
    make("EDIT", "", EDIT, ID_SOURCE, F_BODY);
    make("BUTTON", "", OWNERDRAW, ID_CONNECT, F_BOLD);
    make("STATIC", "", STATIC_WRAP, ID_CONNERR, F_LABEL);
    make("STATIC", "OBS 설정 방법", STATIC_LINE, ID_L_HELP, F_LABEL);
    make("STATIC", HELP_TEXT, STATIC_WRAP, ID_HELP, F_LABEL);
    make("STATIC", NOTICE_TEXT, STATIC_WRAP | 0x1, ID_NOTICE, F_MINI); // SS_CENTER
    SendMessageW(item(ID_PASSWORD), 0xCC, 0x25CF, 0); // EM_SETPASSWORDCHAR ●
}

// ── 배치 (96dpi 기준 좌표, 스크롤 반영) ──
fn layout() {
    let win = WIN.get();
    if win.is_null() {
        return;
    }
    let v = VIEW.with(|v| v.borrow().clone());
    let err = matches!(v.conn, Conn::Err(_)) && !obs_dirty(&v);

    // 전체 높이 먼저 계산 → 스크롤 범위 보정
    let help_top = 628 + if err { 36 } else { 0 } + 4;
    let card2_bottom = help_top + 20 + HELP_H + 8;
    let notice_top = card2_bottom + 10;
    let content_h = notice_top + NOTICE_H + 16;
    let max_scroll = (px(content_h) - px(VISIBLE_H)).max(0);
    SCROLL.set(SCROLL.get().clamp(0, max_scroll));
    let sc = SCROLL.get();

    let mv = |id: i32, x: i32, y: i32, w: i32, h: i32| unsafe {
        MoveWindow(item(id), px(x), px(y) - sc, px(w), px(h), 0);
    };
    let mut boxes = Vec::new();
    let mut bx = |id: i32, x: i32, y: i32, w: i32, h: i32| {
        boxes.push((id, rect(px(x), px(y) - sc, px(x + w), px(y + h) - sc)));
        mv(id, x + 8, y + 6, w - 16, h - 12);
    };

    // 헤더
    mv(ID_H1, 12, 13, 168, 22);
    mv(ID_VER, 180, 18, 80, 16);

    // 제목 설정 카드
    mv(ID_L_RAW, 24, 56, 316, 16);
    mv(ID_RAW, 24, 72, 316, 20);
    mv(ID_L_PREVIEW, 24, 98, 316, 16);
    mv(ID_PREVIEW, 24, 114, 316, 28);
    mv(ID_L_VARS, 24, 146, 316, 16);
    for i in 0..VARS.len() as i32 {
        mv(ID_VAR + i, 24, 162 + i * 26, 316, 24);
    }
    mv(ID_L_TPL, 24, 246, 316, 16);
    for (i, p) in PRESETS.iter().enumerate() {
        mv(ID_PRESET + i as i32, p.2, 262, p.3, 24);
    }
    mv(ID_L_CUSTOM, 24, 296, 316, 16);
    bx(ID_TEMPLATE, 24, 312, 316, 52);
    let card1 = rect(px(12), px(44) - sc, px(352), px(376) - sc);

    // OBS 연결 카드
    mv(ID_OBSHEAD, 24, 398, 64, 18);
    mv(ID_BADGE, 90, 400, 150, 16);
    mv(ID_L_HOST, 24, 424, 232, 16);
    mv(ID_L_PORT, 264, 424, 76, 16);
    bx(ID_HOST, 24, 440, 232, 30);
    bx(ID_PORT, 264, 440, 76, 30);
    mv(ID_L_PW, 24, 478, 76, 16);
    mv(ID_SHOWPW, 100, 478, 34, 16);
    bx(ID_PASSWORD, 24, 496, 316, 30);
    mv(ID_L_SOURCE, 24, 534, 316, 16);
    bx(ID_SOURCE, 24, 550, 316, 30);
    mv(ID_CONNECT, 24, 590, 316, 32);
    unsafe { ShowWindow(item(ID_CONNERR), if err { 8 } else { 0 }) }; // SW_SHOWNA / SW_HIDE
    if err {
        mv(ID_CONNERR, 24, 628, 316, 32);
    }
    mv(ID_L_HELP, 24, help_top, 316, 16);
    mv(ID_HELP, 24, help_top + 20, 316, HELP_H);
    let card2 = rect(px(12), px(386) - sc, px(352), px(card2_bottom) - sc);
    mv(ID_NOTICE, 12, notice_top, 340, NOTICE_H);

    CARDS.with(|c| *c.borrow_mut() = vec![card1, card2]);
    BOXES.with(|b| *b.borrow_mut() = boxes);

    let si = SCROLLINFO {
        cbSize: std::mem::size_of::<SCROLLINFO>() as u32,
        fMask: 0x1 | 0x2 | 0x4, // RANGE | PAGE | POS
        nMin: 0,
        nMax: px(content_h) - 1,
        nPage: px(VISIBLE_H) as u32,
        nPos: sc,
        nTrackPos: 0,
    };
    unsafe {
        SetScrollInfo(win, 1, &si, 1); // SB_VERT
        RedrawWindow(win, null(), null_mut(), 0x1 | 0x4 | 0x80); // INVALIDATE | ERASE | ALLCHILDREN
    }
}

fn scroll_to(y: i32) {
    SCROLL.set(y);
    layout();
}

// ── 상태 반영 ──
pub(crate) fn refresh(view: View) {
    VIEW.with(|v| *v.borrow_mut() = view.clone());
    if WIN.get().is_null() {
        return;
    }
    set_text(ID_RAW, view.raw.as_deref().unwrap_or("같이보기 창을 찾지 못함"));
    set_text(ID_PREVIEW, &view.preview);
    set_text(ID_BADGE, match view.conn {
        Conn::Ok => "연결됨",
        Conn::Err(_) => "연결 안 됨",
        _ => "",
    });
    set_text(ID_CONNERR, if let Conn::Err(e) = &view.conn { e } else { "" });
    layout();
}

// ── 그리기 ──
unsafe fn round(hdc: HDC, r: &RECT, radius: i32, fill: u32, border: u32) {
    let brush = CreateSolidBrush(fill);
    let pen = CreatePen(0, 1, border);
    let ob = SelectObject(hdc, brush as _);
    let op = SelectObject(hdc, pen as _);
    RoundRect(hdc, r.left, r.top, r.right, r.bottom, radius, radius);
    SelectObject(hdc, ob);
    SelectObject(hdc, op);
    DeleteObject(brush as _);
    DeleteObject(pen as _);
}

unsafe fn text(hdc: HDC, s: &str, mut r: RECT, f: usize, color: u32, flags: u32) {
    let of = SelectObject(hdc, font(f) as _);
    SetTextColor(hdc, color);
    SetBkMode(hdc, 1); // TRANSPARENT
    let mut w: Vec<u16> = s.encode_utf16().collect();
    DrawTextW(hdc, w.as_mut_ptr(), w.len() as i32, &mut r, flags | 0x800); // DT_NOPREFIX
    SelectObject(hdc, of);
}

const CENTER: u32 = 0x1 | 0x4 | 0x20; // DT_CENTER | DT_VCENTER | DT_SINGLELINE
const LEFT: u32 = 0x4 | 0x20 | 0x8000; // DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS

unsafe fn paint(win: HWND) {
    let mut ps: PAINTSTRUCT = std::mem::zeroed();
    let hdc = BeginPaint(win, &mut ps);
    let mut rc = rect(0, 0, 0, 0);
    GetClientRect(win, &mut rc);
    // 깜빡임 방지용 버퍼
    let mem = CreateCompatibleDC(hdc);
    let bmp = CreateCompatibleBitmap(hdc, rc.right, rc.bottom);
    let ob = SelectObject(mem, bmp as _);
    FillRect(mem, &rc, BR_BG.get());
    CARDS.with(|c| {
        for r in c.borrow().iter() {
            round(mem, r, px(20), PANEL, LINE);
        }
    });
    BOXES.with(|b| {
        for (id, r) in b.borrow().iter() {
            round(mem, r, px(12), BG, if FOCUS.get() == *id { ACCENT } else { LINE });
        }
    });
    BitBlt(hdc, 0, 0, rc.right, rc.bottom, mem, 0, 0, 0x00CC_0020); // SRCCOPY
    SelectObject(mem, ob);
    DeleteObject(bmp as _);
    DeleteDC(mem);
    EndPaint(win, &ps);
}

unsafe fn draw_item(d: &DRAWITEMSTRUCT) {
    let id = d.CtlID as i32;
    let (hdc, r) = (d.hDC, d.rcItem);
    let v = VIEW.with(|v| v.borrow().clone());
    FillRect(hdc, &r, BR_PANEL.get());
    let h = r.bottom - r.top;

    match id {
        i if (ID_VAR..ID_VAR + VARS.len() as i32).contains(&i) => {
            let (code, desc) = VARS[(i - ID_VAR) as usize];
            let (name, ep) = title::parse_episode(v.raw.as_deref().unwrap_or(EXAMPLE));
            let pre = if v.raw.is_some() { "" } else { "예: " };
            let value = match code {
                "{title}" => v.raw.clone().unwrap_or_else(|| EXAMPLE.into()),
                "{name}" => name,
                _ => if ep.is_empty() { "(없음)".into() } else { ep },
            };
            let chip = rect(r.left, r.top + px(3), r.left + px(52), r.bottom - px(3));
            round(hdc, &chip, h, LINE, LINE);
            text(hdc, code, chip, F_CODE, TEXT, CENTER);
            text(hdc, desc, rect(r.left + px(60), r.top, r.left + px(122), r.bottom), F_CHIP, TEXT, LEFT);
            text(hdc, &format!("{pre}{value}"), rect(r.left + px(124), r.top, r.right, r.bottom), F_CHIP, MUTED, LEFT);
        }
        i if (ID_PRESET..ID_PRESET + PRESETS.len() as i32).contains(&i) => {
            let (label, tpl, _, _) = PRESETS[(i - ID_PRESET) as usize];
            let on = v.template == tpl;
            let (fill, fg) = if on { (ACCENT, INK) } else { (LINE, TEXT) };
            round(hdc, &r, h, fill, fill);
            text(hdc, label, r, F_CHIP, fg, CENTER);
        }
        ID_SHOWPW => {
            round(hdc, &r, px(8), LINE, LINE);
            text(hdc, if PW_SHOWN.get() { "숨김" } else { "표시" }, r, F_MINI, TEXT, CENTER);
        }
        ID_CONNECT => {
            // 버튼 = 현재 상태 표시
            let (label, fill, border, fg) = if obs_dirty(&v) {
                ("변경사항 적용 (연결하기)", ACCENT, ACCENT, INK)
            } else {
                match v.conn {
                    Conn::Busy => ("연결 중…", LINE, LINE, MUTED),
                    Conn::Ok => ("✓ 연결됨", PANEL, ACCENT, ACCENT),
                    Conn::Err(_) => ("다시 연결하기", LINE, BAD, TEXT),
                    Conn::Unknown => ("연결하기", LINE, LINE, TEXT),
                }
            };
            round(hdc, &r, px(12), fill, border);
            text(hdc, label, r, F_BOLD, fg, CENTER);
        }
        _ => {}
    }
}

// ── 메시지 처리 ──
unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        0xF => {
            paint(hwnd); // WM_PAINT
            0
        }
        0x14 => 1, // WM_ERASEBKGND: paint에서 처리
        0x2B => {
            draw_item(&*(lparam as *const DRAWITEMSTRUCT)); // WM_DRAWITEM
            1
        }
        0x138 => {
            // WM_CTLCOLORSTATIC
            let hdc = wparam as HDC;
            let id = GetDlgCtrlID(lparam as HWND);
            let v = VIEW.with(|v| v.borrow().clone());
            let header = matches!(id, ID_H1 | ID_VER | ID_NOTICE); // 카드 밖 (배경색)
            let fg = match id {
                ID_H1 | ID_RAW => TEXT,
                ID_PREVIEW => ACCENT,
                ID_BADGE => if let Conn::Err(_) = v.conn { BAD } else { ACCENT },
                ID_CONNERR => BAD,
                _ => MUTED,
            };
            SetTextColor(hdc, fg);
            SetBkColor(hdc, if header { BG } else { PANEL });
            (if header { BR_BG.get() } else { BR_PANEL.get() }) as LRESULT
        }
        0x133 => {
            // WM_CTLCOLOREDIT
            let hdc = wparam as HDC;
            SetTextColor(hdc, TEXT);
            SetBkColor(hdc, BG);
            BR_BG.get() as LRESULT
        }
        0x111 => {
            // WM_COMMAND
            let (id, code) = ((wparam & 0xffff) as i32, (wparam >> 16) as u32);
            let clicked = code == 0 || code == 5; // BN_CLICKED / BN_DOUBLECLICKED
            match id {
                // 여러 줄 입력칸은 프로그램이 글자를 바꾸면 EN_CHANGE를 안 보내므로 버튼에서 직접 반영
                i if clicked && (ID_VAR..ID_VAR + VARS.len() as i32).contains(&i) => {
                    // 커서 위치에 변수 넣기
                    let s = wide(VARS[(i - ID_VAR) as usize].0);
                    FILLING.set(true);
                    SendMessageW(item(ID_TEMPLATE), 0xC2, 1, s.as_ptr() as LPARAM); // EM_REPLACESEL
                    FILLING.set(false);
                    crate::set_template(ctrl_text(ID_TEMPLATE));
                }
                i if clicked && (ID_PRESET..ID_PRESET + PRESETS.len() as i32).contains(&i) => {
                    let tpl = PRESETS[(i - ID_PRESET) as usize].1;
                    FILLING.set(true);
                    set_text(ID_TEMPLATE, tpl);
                    FILLING.set(false);
                    crate::set_template(tpl.to_string());
                }
                ID_SHOWPW if clicked => {
                    // 창을 열 때마다 기본은 숨김
                    PW_SHOWN.set(!PW_SHOWN.get());
                    let pw = item(ID_PASSWORD);
                    SendMessageW(pw, 0xCC, if PW_SHOWN.get() { 0 } else { 0x25CF }, 0); // EM_SETPASSWORDCHAR
                    InvalidateRect(pw, null(), 1);
                    InvalidateRect(item(ID_SHOWPW), null(), 0);
                }
                ID_CONNECT if clicked => {
                    let (h, p, pw, s) = read_obs_form();
                    crate::connect(h, p, pw, s);
                }
                ID_TEMPLATE if code == 0x300 && !FILLING.get() => crate::set_template(ctrl_text(ID_TEMPLATE)),
                ID_TEMPLATE | ID_HOST | ID_PORT | ID_PASSWORD | ID_SOURCE if code == 0x100 || code == 0x200 => {
                    // EN_SETFOCUS / EN_KILLFOCUS: 입력칸 테두리 색
                    FOCUS.set(if code == 0x100 { id } else { 0 });
                    InvalidateRect(hwnd, null(), 0);
                }
                ID_HOST | ID_PORT | ID_PASSWORD | ID_SOURCE if code == 0x300 && !FILLING.get() => {
                    layout(); // 연결 버튼을 "변경사항 적용"으로
                }
                _ => {}
            }
            0
        }
        0x115 => {
            // WM_VSCROLL
            let mut si: SCROLLINFO = std::mem::zeroed();
            si.cbSize = std::mem::size_of::<SCROLLINFO>() as u32;
            si.fMask = 0x17; // ALL
            GetScrollInfo(hwnd, 1, &mut si);
            let step = px(40);
            let y = match (wparam & 0xffff) as u32 {
                0 => si.nPos - step,
                1 => si.nPos + step,
                2 => si.nPos - si.nPage as i32,
                3 => si.nPos + si.nPage as i32,
                4 | 5 => si.nTrackPos,
                6 => 0,
                7 => i32::MAX,
                _ => si.nPos,
            };
            scroll_to(y);
            0
        }
        0x20A => {
            // WM_MOUSEWHEEL
            let delta = ((wparam >> 16) & 0xffff) as u16 as i16 as i32;
            scroll_to(SCROLL.get() - delta * px(40) / 120);
            0
        }
        WM_APP_STATUS => {
            crate::on_status();
            0
        }
        0x113 => {
            crate::on_timer(wparam); // WM_TIMER
            0
        }
        0x10 => {
            // WM_CLOSE: OBS 텍스트 비우고 종료
            crate::on_exit();
            DestroyWindow(hwnd);
            0
        }
        0x2 => {
            PostQuitMessage(0); // WM_DESTROY
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
