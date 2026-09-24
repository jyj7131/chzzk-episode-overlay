#![windows_subsystem = "windows"]

mod config;
mod obs;
mod title;
mod ui;

use config::Config;
use std::cell::{Cell, RefCell};
use std::ptr::{null, null_mut};
use std::sync::mpsc::{self, Receiver, Sender};

use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::LibraryLoader::*;
use windows_sys::Win32::System::Threading::*;
use windows_sys::Win32::UI::Accessibility::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

pub(crate) const APP_NAME: &str = "같이보기 화수 표시";
pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");
pub(crate) const WINDOW_CLASS: &str = "ChzzkEpObsWindow";

pub(crate) const DEFAULT_HOST: &str = "127.0.0.1";
const NEED_SOURCE: &str = "텍스트 소스 이름을 입력하세요";

pub(crate) const WM_APP_STATUS: u32 = 0x8002;
const TIMER_CLEAR: usize = 1;
const TIMER_TEMPLATE: usize = 2;
const CLEAR_DELAY_MS: u32 = 1000; // 새로고침/화 전환 중 잠깐 벗어나는 경우를 대비

const EVENT_DESTROY: u32 = 0x8001;
const EVENT_NAMECHANGE: u32 = 0x800C;
const OUT_OF_CONTEXT_SKIP_OWN: u32 = 0x0002;

pub(crate) fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

// ── 상태 ──
#[derive(Clone, Default)]
pub(crate) enum Conn {
    #[default]
    Unknown,
    Busy,
    Ok,
    Err(String),
}

enum Job {
    Push { cfg: obs::Cfg, text: String, force: bool },
    Test { cfg: obs::Cfg },
}

struct Res {
    test: bool,
    result: Result<(), String>,
}

pub(crate) struct App {
    pub cfg: Config,
    // 같이보기 제목이 보인 창들: (창, 제목, 최근 순서)
    watch: Vec<(isize, String, u64)>,
    seq: u64,
    pub raw: Option<String>,
    pub conn: Conn,
    tx: Sender<Job>,
    rx: Receiver<Res>,
}

impl App {
    fn obs_cfg(&self) -> obs::Cfg {
        obs::Cfg {
            host: if self.cfg.host.is_empty() { DEFAULT_HOST.into() } else { self.cfg.host.clone() },
            port: self.cfg.port,
            password: self.cfg.password.clone(),
            source: self.cfg.source.clone(),
        }
    }

    pub fn output(&self) -> String {
        self.raw.as_deref().map_or(String::new(), |r| title::format(&self.cfg.template, r))
    }

    fn push(&mut self, force: bool) {
        if self.cfg.source.is_empty() {
            return; // 아직 설정 전
        }
        let _ = self.tx.send(Job::Push { cfg: self.obs_cfg(), text: self.output(), force });
    }

    // 연결 확인 (소스 이름이 비어 있으면 시도하지 않고 안내)
    fn test_connection(&mut self) {
        if self.cfg.source.is_empty() {
            self.conn = Conn::Err(NEED_SOURCE.into());
            return;
        }
        self.conn = Conn::Busy;
        let _ = self.tx.send(Job::Test { cfg: self.obs_cfg() });
    }

    // 안전장치: 닫힌 창이나 더 이상 같이보기가 아닌 창은 목록에서 제거 (닫힘 알림을 놓친 경우 대비)
    fn prune(&mut self) {
        self.watch.retain(|w| unsafe { IsWindow(w.0 as HWND) != 0 } && title::extract(&window_text(w.0 as HWND)).is_some());
    }

    // 가장 최근에 바뀐 같이보기 창의 제목을 표시, 없으면 잠시 후 비움
    fn apply(&mut self, force: bool) {
        let win = WIN.get();
        self.prune();
        match self.watch.iter().max_by_key(|w| w.2) {
            None => unsafe {
                SetTimer(win, TIMER_CLEAR, CLEAR_DELAY_MS, None);
            },
            Some(w) => {
                unsafe { KillTimer(win, TIMER_CLEAR) };
                self.raw = Some(w.1.clone());
                self.push(force);
            }
        }
    }
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    pub(crate) static WIN: Cell<HWND> = const { Cell::new(null_mut()) };
    pub(crate) static ICON: Cell<HICON> = const { Cell::new(null_mut()) };
}

// 창 메시지 처리 중 다시 들어오는 경우(재진입)엔 조용히 건너뜀
pub(crate) fn with<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|c| c.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

fn window_text(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    // InternalGetWindowText: 응답 없는 창에 메시지를 보내지 않음
    let n = unsafe { InternalGetWindowText(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

// ── 창 제목 감시 (WinEvent 후크: 제목이 바뀌거나 창이 닫힐 때만 호출됨, 폴링 없음) ──
unsafe extern "system" fn win_event(_: HWINEVENTHOOK, event: u32, hwnd: HWND, id_object: i32, id_child: i32, _: u32, _: u32) {
    if id_object != 0 || id_child != 0 || hwnd.is_null() {
        return; // 창 자체(OBJID_WINDOW, CHILDID_SELF)만
    }
    let changed = match event {
        EVENT_NAMECHANGE => {
            if GetAncestor(hwnd, GA_ROOT) != hwnd {
                return;
            }
            let t = window_text(hwnd);
            with(|a| on_title(a, hwnd as isize, &t)).unwrap_or(false)
        }
        EVENT_DESTROY => with(|a| remove_watch(a, hwnd as isize)).unwrap_or(false),
        _ => false,
    };
    if changed {
        refresh_ui();
    }
}

fn on_title(a: &mut App, hwnd: isize, t: &str) -> bool {
    if let Some(raw) = title::extract(t) {
        a.seq += 1;
        let seq = a.seq;
        match a.watch.iter_mut().find(|w| w.0 == hwnd) {
            Some(w) => {
                w.1 = raw;
                w.2 = seq;
            }
            None => a.watch.push((hwnd, raw, seq)),
        }
        a.apply(false);
        true
    } else {
        // 같이보기 탭을 닫았거나, 다른 탭/페이지로 이동 → 1초 뒤 비움 (그 사이 돌아오면 유지)
        remove_watch(a, hwnd)
    }
}

fn remove_watch(a: &mut App, hwnd: isize) -> bool {
    let before = a.watch.len();
    a.watch.retain(|w| w.0 != hwnd);
    if a.watch.len() == before {
        return false;
    }
    a.apply(false);
    true
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let list = &mut *(lparam as *mut Vec<(isize, String)>);
    if let Some(raw) = title::extract(&window_text(hwnd)) {
        list.push((hwnd as isize, raw));
    }
    1
}

// 열려 있는 창을 한 번 훑어서 목록을 새로 만듦 (시작할 때 / 창을 다시 활성화할 때)
fn rescan(force: bool) {
    let mut found: Vec<(isize, String)> = Vec::new();
    let fg = unsafe {
        EnumWindows(Some(enum_proc), &mut found as *mut _ as LPARAM);
        GetForegroundWindow() as isize
    };
    found.sort_by_key(|(h, _)| *h == fg); // 지금 앞에 있는 창을 가장 최근으로
    with(|a| {
        a.watch = found
            .into_iter()
            .map(|(h, raw)| {
                a.seq += 1;
                (h, raw, a.seq)
            })
            .collect();
        if a.watch.is_empty() {
            a.raw = None;
        }
        a.apply(force);
    });
    refresh_ui();
}

// ── 작업 스레드: OBS 전송은 여기서 (화면이 멈추지 않도록) ──
fn worker(rx: Receiver<Job>, tx: Sender<Res>, win: isize) {
    let mut last_sent: Option<String> = None;
    for job in rx {
        let res = match job {
            Job::Push { cfg, text, force } => {
                if !force && last_sent.as_deref() == Some(text.as_str()) {
                    continue;
                }
                let r = obs::set_text(&cfg, &text);
                last_sent = r.is_ok().then_some(text);
                Res { test: false, result: r }
            }
            Job::Test { cfg } => {
                let r = obs::test(&cfg);
                if r.is_err() {
                    last_sent = None;
                }
                Res { test: true, result: r }
            }
        };
        let _ = tx.send(res);
        unsafe { PostMessageW(win as HWND, WM_APP_STATUS, 0, 0) };
    }
}

// ── 창에서 부르는 동작 (확장 팝업과 같은 규칙) ──

// 템플릿은 바로 저장, OBS 전송은 입력이 멈추면
pub(crate) fn set_template(t: String) {
    with(|a| {
        a.cfg.template = t;
        a.cfg.save();
    });
    refresh_ui();
    unsafe { SetTimer(WIN.get(), TIMER_TEMPLATE, 400, None) };
}

// OBS 연결 칸은 [연결하기]를 눌러야 저장 + 연결 확인, 성공하면 바로 전송
pub(crate) fn connect(host: String, port: u16, password: String, source: String) {
    with(|a| {
        a.cfg.host = host; // 비어 있으면 연결할 때 127.0.0.1 사용
        a.cfg.port = port;
        a.cfg.password = password;
        a.cfg.source = source;
        a.cfg.save();
        a.test_connection();
    });
    refresh_ui();
}

// 작업 스레드의 결과 반영
pub(crate) fn on_status() {
    with(|a| {
        while let Ok(r) = a.rx.try_recv() {
            let test_ok = r.test && r.result.is_ok();
            a.conn = match r.result {
                Ok(()) => Conn::Ok,
                Err(e) => Conn::Err(e),
            };
            if test_ok {
                a.push(true);
            }
        }
    });
    refresh_ui();
}

pub(crate) fn on_timer(id: usize) {
    unsafe { KillTimer(WIN.get(), id) };
    match id {
        TIMER_CLEAR => {
            with(|a| {
                a.prune();
                if a.watch.is_empty() {
                    a.raw = None;
                    a.push(false);
                }
            });
            refresh_ui();
        }
        TIMER_TEMPLATE => {
            with(|a| a.push(true));
        }
        _ => {}
    }
}

// 창을 닫으면 OBS에 화수가 남지 않도록 비우고 종료 (전송이 가능한 상태일 때만)
pub(crate) fn on_exit() {
    if let Some(Some(cfg)) = with(|a| matches!(a.conn, Conn::Ok).then(|| a.obs_cfg())) {
        let _ = obs::set_text(&cfg, "");
    }
}

pub(crate) fn refresh_ui() {
    if let Some(view) = with(|a| ui::View::from(a)) {
        ui::refresh(view);
    }
}

fn main() {
    unsafe {
        // 중복 실행 방지: 이미 떠 있으면 그 창을 앞으로 가져오고 종료
        CreateMutexW(null(), 0, wide("ChzzkEpisodeOBS_single").as_ptr());
        if GetLastError() == 183 {
            let other = FindWindowW(wide(WINDOW_CLASS).as_ptr(), null());
            if !other.is_null() {
                ShowWindow(other, 9); // SW_RESTORE
                SetForegroundWindow(other);
            }
            return;
        }

        SetProcessDPIAware();

        // 설정은 exe 옆에만 저장하므로, 쓸 수 없는 폴더면 옮기라고 안내하고 종료
        if !config::writable() {
            let msg = "이 폴더에는 설정 파일을 저장할 수 없습니다.\n\n\
                       바탕화면이나 문서 폴더처럼 쓰기 가능한 폴더로\n\
                       프로그램을 옮긴 뒤 다시 실행해 주세요.\n\n\
                       (Program Files 등 시스템 폴더에서는 실행할 수 없습니다)";
            MessageBoxW(null_mut(), wide(msg).as_ptr(), wide(APP_NAME).as_ptr(), 0x30); // MB_ICONWARNING
            return;
        }

        let hinst = GetModuleHandleW(null());
        let mut icon = LoadIconW(hinst, 1 as _);
        if icon.is_null() {
            icon = LoadIconW(null_mut(), IDI_APPLICATION);
        }
        ICON.set(icon);

        let (cfg, existed) = Config::load();
        ui::register(hinst);
        let win = ui::create(hinst);
        WIN.set(win);

        let (job_tx, job_rx) = mpsc::channel();
        let (res_tx, res_rx) = mpsc::channel();
        let win_id = win as isize;
        std::thread::spawn(move || worker(job_rx, res_tx, win_id));
        APP.with(|c| {
            *c.borrow_mut() = Some(App { cfg, watch: Vec::new(), seq: 0, raw: None, conn: Conn::Unknown, tx: job_tx, rx: res_rx })
        });
        ui::fill_form();

        // 창 제목 변경 / 창 닫힘만 구독
        let hooks = [
            SetWinEventHook(EVENT_NAMECHANGE, EVENT_NAMECHANGE, null_mut(), Some(win_event), 0, 0, OUT_OF_CONTEXT_SKIP_OWN),
            SetWinEventHook(EVENT_DESTROY, EVENT_DESTROY, null_mut(), Some(win_event), 0, 0, OUT_OF_CONTEXT_SKIP_OWN),
        ];

        rescan(false);
        if existed {
            // 시작할 때 OBS 연결 상태 확인 (성공하면 현재 제목 전송)
            with(|a| a.test_connection());
            refresh_ui();
        }
        ui::show(win);

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            if IsDialogMessageW(win, &msg) != 0 {
                continue; // Tab 키로 입력칸 이동
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        for h in hooks {
            UnhookWinEvent(h);
        }
    }
}
