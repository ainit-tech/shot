//! The program's own browser windows (decision 8): Google's pages in WebView2, the Edge engine
//! built into Windows. The windows keep a profile of their own, so the user signs in to Google in
//! them once, and the program drives their pages: a query's script runs on the page it opens.
//! Every query has a window of its own, as many as the user likes, titled with its question: it
//! comes out next to the selection with AI Mode's conversation alone, and goes for good when
//! closed. A spare page, a fresh AI Mode loaded hidden in the background, waits for the next
//! question, which takes it and goes at once; another spare page is made a moment later. Links
//! that would open a new window open in the user's own browser. A question can be asked aloud:
//! the page listens while the button is held and never in the background. Whether someone is
//! signed in to Google in the windows, the program learns from Google's cookies.

use std::cell::RefCell;
use std::io;
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_COLOR, COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC, COREWEBVIEW2_PERMISSION_KIND,
    COREWEBVIEW2_PERMISSION_KIND_MICROPHONE, COREWEBVIEW2_PERMISSION_STATE_ALLOW, COREWEBVIEW2_WEB_ERROR_STATUS,
    COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED, CreateCoreWebView2EnvironmentWithOptions, ICoreWebView2, ICoreWebView2_2,
    ICoreWebView2Controller, ICoreWebView2Controller2, ICoreWebView2CookieList, ICoreWebView2Environment,
    ICoreWebView2EnvironmentOptions, ICoreWebView2NavigationCompletedEventArgs, ICoreWebView2NavigationCompletedEventArgs2,
    ICoreWebView2NavigationStartingEventArgs,
};
use webview2_com::{
    AddScriptToExecuteOnDocumentCreatedCompletedHandler, CoreWebView2EnvironmentOptions,
    CreateCoreWebView2ControllerCompletedHandler, CreateCoreWebView2EnvironmentCompletedHandler, DOMContentLoadedEventHandler,
    DocumentTitleChangedEventHandler, ExecuteScriptCompletedHandler, GetCookiesCompletedHandler,
    NavigationCompletedEventHandler,
    NavigationStartingEventHandler, NewWindowRequestedEventHandler, PermissionRequestedEventHandler,
    WebMessageReceivedEventHandler, take_pwstr,
};
use windows::Win32::Foundation::{E_POINTER, HWND as ComWindow, RECT as ComRect};
use windows_core::{BOOL as ComBool, Interface, PCWSTR as ComText, PWSTR as ComString};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows_sys::Win32::Graphics::Gdi::{
    CreateSolidBrush, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, MonitorFromRect,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyWindow, GetClientRect, GetCursorPos, GetWindowRect, HICON,
    IDC_ARROW, IsIconic, IsWindowVisible, IsZoomed, KillTimer, LoadCursorW, PostMessageW, RegisterClassExW, SW_RESTORE,
    SW_SHOW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOZORDER, SetForegroundWindow, SetTimer, SetWindowPos,
    SetWindowTextW, ShowWindow, WA_INACTIVE, WM_ACTIVATE, WM_APP, WM_CLOSE, WM_DPICHANGED, WM_MOVE, WM_SIZE,
    WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_OVERLAPPEDWINDOW,
};
use windows_sys::core::{BOOL, PCWSTR};
use windows_sys::w;

use super::{QUIET, app, card, clipboard, last_error, out_of_sight, shell, tray, wide};
use crate::card as layout;
use crate::geometry::Rect;
use crate::image::Image;
use crate::query::{self, Told};
use crate::settings::Mode;
use crate::text::tr;

/// Google's account page: the sign-in when nobody is signed in in the windows, the account
/// itself when someone is.
pub const ACCOUNT: &str = "https://accounts.google.com/";

const CLASS: PCWSTR = w!("ainitShot.browser");
/// Opens the address that waits for the window, after the event in which it was decided.
const WM_NAVIGATE: u32 = WM_APP + 1;
/// Lets the window go for good if it is hidden, after the event in which that was decided: its
/// page has nothing more for the user, as behind an answer card that has closed.
const WM_RELEASE: u32 = WM_APP + 2;
/// The colour of a window until its page is drawn: the toolbar's.
const BACKGROUND: u32 = 0x0020_2020;
/// How long the spare page stays fresh enough to take a question.
const FRESH: Duration = Duration::from_secs(30 * 60);
/// How long after a question has taken the spare page the next one is made, in milliseconds: the
/// question's own page goes first.
const SPARE_DELAY: u32 = 3000;
/// How far a window goes aside from another one that it would cover exactly, at the scale of
/// 100%.
const STEP: i32 = 32;
/// The most characters of a question in a window's title.
const TITLE_LENGTH: usize = 80;
/// What the page has heard of a question asked aloud, for the window that listens: a [`Heard`] in
/// a box in `lparam`.
pub const WM_HEARD: u32 = WM_APP + 3;

/// What another window of the program does with WebView2's environment once it has started,
/// or with nothing if WebView2 couldn't start.
pub type Wanting = Box<dyn FnOnce(Option<&ICoreWebView2Environment>)>;

/// What hears whether someone is signed in to Google in the windows.
pub type Answer = Box<dyn FnOnce(bool)>;

/// What the page has heard of a question asked aloud.
#[derive(Debug, PartialEq)]
pub enum Heard {
    /// The microphone is on.
    Listening,
    /// The words heard so far.
    Words(String),
    /// Nothing was heard, and nothing has gone.
    Silent,
    /// The question has gone, and the window shows the conversation.
    Sent,
    /// The question couldn't be asked, in words for the user.
    Failed(String),
}

/// The script that gives Google's pages a speech recognition that works in the window. WebView2
/// has no speech service of its own: the browser's recognition (`webkitSpeechRecognition`) fails
/// at once with "network", and AI Mode's microphone knows no other. Google's own recognition,
/// which its search box uses, works: as Google's script starts, the script catches Google's own
/// recogniser where Google registers it, and gives the page a recognition under the browser's
/// name that hands the speech to it and its results back as the browser's would; one started
/// before Google has registered its recogniser waits for it. The microphone never listens in
/// the background: a recognition doesn't start in a hidden page and ends when the page is hidden
/// or left, after eight seconds without a word heard, and after a minute at most.
const SPEECH_SCRIPT: &str = r##"(() => {
  if (location.hostname !== "www.google.com") return;
  const set = Map.prototype.set;
  let recogniser = null;
  // Recognitions started before Google registered its recogniser.
  const waiting = new Set();
  const watch = function (key, value) {
    if (key === 0 && typeof value === "function" && /getUserMedia/.test(String(value))) {
      recogniser = value;
      if (Map.prototype.set === watch) Map.prototype.set = set;
      for (const begin of waiting) setTimeout(begin);
    }
    return set.call(this, key, value);
  };
  Map.prototype.set = watch;
  // Every recognition going on in the page, by the function that ends it.
  const listening = new Set();
  const hush = () => {
    for (const halt of [...listening]) halt();
  };
  document.addEventListener("visibilitychange", () => document.visibilityState === "hidden" && hush());
  window.addEventListener("pagehide", hush);
  Object.defineProperty(window, "__screenshotListening", { get: () => listening.size });
  const fire = (target, type, fields = {}) => {
    const event = Object.assign(new Event(type), fields);
    target.dispatchEvent(event);
    target["on" + type]?.call(target, event);
  };
  const list = (items) => Object.assign(items, { item: (index) => items[index] });
  const errorOf = (code) => {
    const text = String(code);
    if (/PERMISSION|DENIED|not-allowed/i.test(text)) return "not-allowed";
    if (/NOT_FOUND|audio-capture/i.test(text)) return "audio-capture";
    if (/NO_SPEECH|no-speech/i.test(text)) return "no-speech";
    return "network";
  };
  class Recognition extends EventTarget {
    lang = "";
    continuous = false;
    interimResults = false;
    maxAlternatives = 1;
    #stop = null;
    #abort = null;
    start() {
      if (this.#stop) {
        throw new DOMException("Failed to execute 'start' on 'SpeechRecognition': recognition has already started.", "InvalidStateError");
      }
      // AI Mode adds "-US" to a page language without a region; the recogniser takes the user's own.
      const primary = this.lang.split("-")[0];
      const language = primary && primary !== "en" && this.lang.endsWith("-US")
        ? navigator.languages.find((tag) => tag.split("-")[0] === primary) || primary
        : this.lang || navigator.language;
      // The text so far and its final part, already given as final results.
      const finals = [];
      let committed = "";
      let running = null;
      let ended = false;
      let stopping = false;
      let heard = false;
      let audible = false;
      let quiet = null;
      const timers = [];
      const later = (action, ms) => timers.push(setTimeout(action, ms));
      const end = () => {
        if (ended) return;
        ended = true;
        timers.forEach(clearTimeout);
        clearTimeout(quiet);
        waiting.delete(begin);
        listening.delete(halt);
        this.#stop = this.#abort = null;
        if (running) fire(this, "audioend");
        fire(this, "end");
      };
      const fail = (error) => {
        if (ended) return;
        fire(this, "error", { error, message: "" });
        halt();
      };
      // Google's recogniser stops without a word, so the end comes from here.
      const halt = () => {
        running?.stop();
        end();
      };
      // After a stop the words already said still come; the recognition ends once they have.
      const settle = () => {
        clearTimeout(quiet);
        quiet = setTimeout(halt, 1000);
      };
      const begin = () => {
        waiting.delete(begin);
        if (ended) return;
        // What Google's recogniser says after the end comes too late.
        running = recogniser({
          language,
          additionalLanguages: [],
          // Google's recogniser says that it listens twice: as the microphone opens, and again as
          // the sound begins to go to its server, which gets what was said in between too. The
          // browser's recognition says so once, as the microphone opens.
          listeningStartHandler: () => {
            if (ended || audible) return;
            audible = true;
            fire(this, "audiostart");
          },
          speechStartHandler: () => ended || fire(this, "speechstart"),
          recognitionResultHandler: (said, stable, final) => {
            if (ended) return;
            heard = true;
            const text = String(said).replace(/\s+/g, " ").trim();
            const fresh = text.startsWith(committed) ? text.slice(committed.length).trim() : text;
            const result = list([{ transcript: fresh, confidence: final ? 0.9 : 0.5 }]);
            result.isFinal = !!final;
            const index = finals.length;
            const results = list([...finals, result]);
            if (final) {
              finals.push(result);
              committed = text;
            }
            fire(this, "result", { results, resultIndex: index });
            if (stopping) final ? halt() : settle();
          },
          recognitionEndHandler: end,
          errorHandler: (code) => fail(errorOf(code)),
          noMatchHandler: () => ended || fire(this, "nomatch"),
        });
        if (!running) return fail("service-not-allowed");
        running.initialize?.();
        running.start(this.continuous);
        fire(this, "start");
        later(() => heard || fail("no-speech"), 8000);
        later(halt, 60000);
      };
      this.#stop = () => {
        // Stopped before the microphone was on: there is nothing to wait for.
        if (!running) return setTimeout(end);
        if (stopping) return;
        stopping = true;
        settle();
        later(halt, 4000);
      };
      this.#abort = () => setTimeout(halt);
      listening.add(halt);
      if (document.visibilityState === "hidden") {
        setTimeout(() => fail("not-allowed"));
      } else if (recogniser) {
        setTimeout(begin);
      } else {
        waiting.add(begin);
        later(() => running || fail("service-not-allowed"), 10000);
      }
    }
    stop() {
      this.#stop?.();
    }
    abort() {
      this.#abort?.();
    }
  }
  window.webkitSpeechRecognition = window.SpeechRecognition = Recognition;
})();
"##;

thread_local! {
    static BROWSER: RefCell<Browser> =
        const { RefCell::new(Browser { environment: None, starting: false, pages: Vec::new(), spare_timer: 0 }) };
    /// What other windows of the program wait to do with WebView2's environment.
    static WANTING: RefCell<Vec<Wanting>> = const { RefCell::new(Vec::new()) };
    /// What waits to hear whether someone is signed in to Google, until a page is there to ask.
    static ASKING: RefCell<Vec<Answer>> = const { RefCell::new(Vec::new()) };
}

/// The Google windows and what they share.
struct Browser {
    /// WebView2's environment, once it has started: every window of the program shows its page
    /// with it.
    environment: Option<ICoreWebView2Environment>,
    /// Whether WebView2 is starting.
    starting: bool,
    /// The windows, the spare page's among them, the one given its work last at the end.
    pages: Vec<Page>,
    /// The timer that makes the next spare page, while it runs.
    spare_timer: usize,
}

/// A Google window and its page.
struct Page {
    window: HWND,
    /// The program's icon on the window's title bar and on the taskbar, destroyed with the window.
    icons: Vec<HICON>,
    /// Comes a moment after the window, once WebView2 has made the page.
    controller: Option<ICoreWebView2Controller>,
    /// The address to open as soon as the window can: once WebView2 has made the page, or once
    /// the navigation that it replaces has been cancelled.
    waiting: Option<String>,
    /// A query waiting for its page to load.
    job: Option<Job>,
    /// The address of the query's page, until it has been asked for once more: a page that
    /// doesn't come, or breaks off before it is whole, is, as the user would ask again. Nothing
    /// can have gone from such a page, nor from the page of WebView2's error in its place.
    again: Option<String>,
    /// A query whose script runs on the page, until the script tells how it went.
    running: Option<Job>,
    /// The selection of the query, on the desktop, and where its button sent the screenshot: the
    /// window comes out next to the selection, at the size of what it shows.
    beside: Option<(Rect, Mode)>,
    /// Whether this is the spare page, hidden, for the next question.
    spare: bool,
    /// Whether the spare page's AI Mode is on its way: its document hasn't come yet.
    loading: bool,
    /// Since when the page is a fresh AI Mode ready for a question, if it is.
    ready: Option<Instant>,
    /// The navigation started last, the window's own or its page's: what earlier ones come to,
    /// as it replaces them, is old news.
    navigation: u64,
    /// Whether the window comes out as the page of the query has its document.
    reveal: bool,
    /// What the program asked the script of a question asked aloud before the script could hear
    /// it: to stop listening or to give up.
    told_page: Option<&'static str>,
    /// Whether the page is the one behind the answer card: it goes with the card, unless the user
    /// has seen it.
    behind_card: bool,
    /// Whether the window was opened at an address of its own, Google's account page for one,
    /// rather than for a query.
    opened: bool,
    /// Whether the page has been on Google's account pages, where one signs in and out: the
    /// spare page, made before, may not know who is signed in any more.
    account: bool,
    /// Whether the window has the title of its question rather than its page's.
    titled: bool,
}

impl Page {
    fn new(window: HWND, icons: Vec<HICON>, spare: bool) -> Self {
        Page {
            window,
            icons,
            controller: None,
            waiting: None,
            job: None,
            again: None,
            running: None,
            beside: None,
            spare,
            loading: false,
            ready: None,
            navigation: 0,
            reveal: false,
            told_page: None,
            behind_card: false,
            opened: false,
            account: false,
            titled: false,
        }
    }
}

/// A query to finish on a page of a window of its own.
pub struct Job {
    /// The name of the query button, for what the program tells the user.
    pub name: String,
    /// The question, which titles the window, as Google titles all its pages alike: the button's
    /// name for a query without one. A question asked aloud takes the words heard once they come.
    pub question: String,
    /// What runs on the page once it has loaded.
    pub script: String,
    /// The screenshot, kept for the clipboard in case the script fails.
    pub picture: Image,
    /// Where the query button sends the screenshot: for an answer the window stays hidden and
    /// the answer goes to the card next to the selection, and the image search's results move
    /// to the tab of exact matches.
    pub mode: Mode,
    /// For a question asked aloud, the window that hears what the page hears ([`WM_HEARD`]).
    /// The window stays hidden until the question has gone.
    pub listener: Option<HWND>,
}

/// Runs the query's script on the page of a window of its own. A question for AI Mode takes the
/// spare page: it goes at once if the page is fresh, and as soon as the page is there if it is on
/// its way. Any other query opens its address on the spare page, or in a new window if there is
/// none, and runs its script once the page has loaded. The window comes out next to the
/// selection, given on the desktop, only with the query's page in it, so that no page from
/// before shows: at once with a fresh spare page, as the page's document is there otherwise. For
/// an answer the window stays hidden behind the card, and for a question asked aloud until the
/// question has gone. The next spare page is made a moment later.
pub fn ask(url: &str, job: Job, selection: Option<Rect>) -> Result<(), String> {
    let (mode, aloud) = (job.mode, job.listener.is_some());
    let ai_mode = url == spare_address();
    let spare = BROWSER.with(|browser| {
        let browser = browser.borrow();
        let page = browser.pages.iter().find(|page| page.spare)?;
        let fresh = page.controller.is_some() && page.ready.is_some_and(|since| since.elapsed() < FRESH);
        Some((page.window, fresh, page.loading))
    });
    let (window, fresh, coming) = match spare {
        Some((window, fresh, loading)) => (window, ai_mode && fresh, ai_mode && !fresh && loading),
        None => (new_page(false)?, false, false),
    };
    if !fresh && !coming {
        load_in(window, url)?;
    }
    // The answer card shows the answer of one page at a time.
    if mode == Mode::Answer {
        card_closed();
    }
    set_title(window, &job.question);
    given(window, |page| {
        page.spare = false;
        page.loading = false;
        page.job = Some(job);
        page.again = (!fresh).then(|| url.to_string());
        page.beside = selection.map(|selection| (selection, mode));
        page.ready = None;
        page.told_page = None;
        page.reveal = !fresh && mode != Mode::Answer && !aloud;
        page.behind_card = mode == Mode::Answer;
    });
    if fresh {
        if mode != Mode::Answer && !aloud {
            show(window);
        }
        if let Some(webview) = webview_of(window) {
            page_ready(window, &webview);
        }
    }
    spare_later();
    Ok(())
}

/// Gives the window its work: its page changes as `change` says and goes to the end of the list,
/// as the one given its work last.
fn given(window: HWND, change: impl FnOnce(&mut Page)) {
    BROWSER.with(|browser| {
        let mut browser = browser.borrow_mut();
        let Some(index) = browser.pages.iter().position(|page| page.window == window) else { return };
        let mut page = browser.pages.remove(index);
        change(&mut page);
        browser.pages.push(page);
    });
}

/// Keeps the spare page ready for the next question, a fresh AI Mode loaded hidden in the
/// background, so that the question goes at once and no page from before shows: made as the
/// program starts and a moment after a question has taken the one before, and loaded afresh as
/// the hotkey freezes the screen if it has grown old.
pub fn prepare() {
    let spare = BROWSER.with(|browser| {
        let browser = browser.try_borrow().ok()?;
        Some(browser.pages.iter().find(|page| page.spare).map(|page| (page.window, page.loading, page.ready)))
    });
    match spare {
        // WebView2 has called in, in the middle of something.
        None => {}
        Some(None) => {
            if let Ok(window) = new_page(true) {
                refresh(window);
            }
        }
        Some(Some((window, false, ready))) if ready.is_none_or(|since| since.elapsed() >= FRESH) => refresh(window),
        // On its way, or fresh.
        Some(Some(_)) => {}
    }
}

/// Loads AI Mode afresh on the spare page.
fn refresh(window: HWND) {
    with_page(window, |page| {
        page.loading = true;
        page.ready = None;
    });
    if load_in(window, &spare_address()).is_err() {
        with_page(window, |page| page.loading = false);
    }
}

/// Loads the spare page afresh, or makes one: someone may have signed in to Google, or out.
fn refresh_spare() {
    let spare = BROWSER.with(|browser| browser.borrow().pages.iter().find(|page| page.spare).map(|page| page.window));
    match spare {
        Some(window) => refresh(window),
        None => prepare(),
    }
}

/// Makes the next spare page a moment from now, once the question that took the last one is
/// under way.
fn spare_later() {
    if BROWSER.with(|browser| browser.borrow().spare_timer) != 0 {
        return;
    }
    let timer = unsafe { SetTimer(null_mut(), 0, SPARE_DELAY, Some(spare_due)) };
    BROWSER.with(|browser| browser.borrow_mut().spare_timer = timer);
}

unsafe extern "system" fn spare_due(_: HWND, _: u32, timer: usize, _: u32) {
    unsafe { KillTimer(null_mut(), timer) };
    BROWSER.with(|browser| {
        if let Ok(mut browser) = browser.try_borrow_mut() {
            browser.spare_timer = 0;
        }
    });
    prepare();
}

/// Hands WebView2's environment to another window of the program, at once if it has started,
/// once it has otherwise; the spare page starts it if it hasn't yet.
pub fn with_environment(then: Wanting) {
    let started = BROWSER.with(|browser| browser.try_borrow().ok()?.environment.clone());
    if let Some(environment) = started {
        return then(Some(&environment));
    }
    // WebView2 may call back before its start has returned: the wish waits first.
    WANTING.with(|wanting| wanting.borrow_mut().push(then));
    prepare();
}

/// Finds out whether someone is signed in to Google in the windows: Google keeps the cookie SID
/// only for an account signed in. `then` hears it once WebView2 has made a page, and hears
/// nothing if WebView2 can't start or can't tell.
pub fn signed_in(then: Answer) {
    let controller = BROWSER.with(|browser| browser.try_borrow().ok()?.pages.iter().find_map(|page| page.controller.clone()));
    match controller {
        Some(controller) => ask_cookies(&controller, then),
        None => {
            ASKING.with(|asking| asking.borrow_mut().push(then));
            prepare();
        }
    }
}

fn ask_cookies(controller: &ICoreWebView2Controller, then: Answer) {
    let answered = GetCookiesCompletedHandler::create(Box::new(move |result, cookies| {
        match (result, cookies) {
            (Ok(()), Some(cookies)) => then(has_account(&cookies)),
            (Err(error), _) => eprintln!("Whether anyone is signed in to Google is unknown: {}", error.message()),
            (Ok(()), None) => {}
        }
        Ok(())
    }));
    let google = wide("https://www.google.com/");
    let asked = unsafe { controller.CoreWebView2() }
        .and_then(|webview| webview.cast::<ICoreWebView2_2>())
        .and_then(|webview| unsafe { webview.CookieManager() })
        .and_then(|cookies| unsafe { cookies.GetCookies(ComText(google.as_ptr()), &answered) });
    if let Err(error) = asked {
        eprintln!("Whether anyone is signed in to Google is unknown: {}", error.message());
    }
}

/// Whether Google's cookies are those of an account signed in.
fn has_account(cookies: &ICoreWebView2CookieList) -> bool {
    let mut count = 0;
    if unsafe { cookies.Count(&mut count) }.is_err() {
        return false;
    }
    (0..count).any(|i| {
        let Ok(cookie) = (unsafe { cookies.GetValueAtIndex(i) }) else { return false };
        let mut name = ComString::null();
        unsafe { cookie.Name(&mut name) }.is_ok() && matches!(take_pwstr(name).as_str(), "SID" | "__Secure-1PSID" | "__Secure-3PSID")
    })
}

/// WebView2 couldn't start: what waited for it hears so, and the windows waiting for their pages
/// go, their queries given up.
fn environment_failed(error: &windows_core::Error) {
    BROWSER.with(|browser| browser.borrow_mut().starting = false);
    wanting_failed();
    let waiting: Vec<HWND> = BROWSER.with(|browser| {
        browser.borrow().pages.iter().filter(|page| page.controller.is_none()).map(|page| page.window).collect()
    });
    if waiting.is_empty() {
        failed(error);
    }
    for window in waiting {
        page_failed(window, &error.message());
    }
}

/// What waited for WebView2's environment hears that there is none.
fn wanting_failed() {
    for then in WANTING.with(|wanting| wanting.take()) {
        then(None);
    }
}

/// The window of the question asked aloud under way, if there is one.
fn listening_page() -> Option<HWND> {
    BROWSER.with(|browser| {
        let browser = browser.try_borrow().ok()?;
        let aloud = |page: &&Page| page.job.as_ref().or(page.running.as_ref()).is_some_and(|job| job.listener.is_some());
        browser.pages.iter().find(aloud).map(|page| page.window)
    })
}

/// The button of a question asked aloud has been let go: the page stops listening once the words
/// already said have come, and sends the question.
pub fn stop_listening() {
    if let Some(window) = listening_page() {
        tell_page(window, "stop");
    }
}

/// A question asked aloud is given up, as the frozen screen closes: the page stops listening at
/// once and sends nothing, and its hidden window goes.
pub fn abort_listening() {
    let Some(window) = listening_page() else { return };
    with_page(window, |page| {
        page.job.take_if(|job| job.listener.is_some());
        page.running.take_if(|job| job.listener.is_some());
    });
    tell_page(window, "abort");
    release(window);
}

/// Tells the script of a question asked aloud to stop listening or to give up: at once, and
/// again as soon as the script says it can hear, in case it couldn't yet.
fn tell_page(window: HWND, what: &'static str) {
    with_page(window, |page| page.told_page = Some(what));
    post_page(window, what);
}

fn post_page(window: HWND, what: &str) {
    let what = wide(what);
    if let Some(webview) = webview_of(window) {
        let _ = unsafe { webview.PostWebMessageAsString(ComText(what.as_ptr())) };
    }
}

/// Whether a question asked aloud is under way on the window's page.
fn asking_aloud(window: HWND) -> bool {
    page_of(window, |page| page.job.as_ref().or(page.running.as_ref()).is_some_and(|job| job.listener.is_some()))
        .unwrap_or(false)
}

/// Tells the window that listens what the page has heard of a question asked aloud.
fn tell(listener: HWND, heard: Heard) {
    let heard = Box::into_raw(Box::new(heard));
    if unsafe { PostMessageW(listener, WM_HEARD, 0, heard as isize) } == 0 {
        drop(unsafe { Box::from_raw(heard) });
    }
}

/// Opens the address in a window of its own in the middle of the monitor under the mouse and
/// brings the window to the front; a window opened so before and still in sight takes it instead.
pub fn open(url: &str) -> Result<(), String> {
    let open = BROWSER.with(|browser| {
        let browser = browser.borrow();
        let page = browser.pages.iter().rev().find(|page| page.opened && unsafe { IsWindowVisible(page.window) } != 0)?;
        Some(page.window)
    });
    let window = match open {
        Some(window) => window,
        None => new_page(false)?,
    };
    if let Err(problem) = load_in(window, url) {
        if open.is_none() {
            destroy(window);
        }
        return Err(problem);
    }
    given(window, |page| {
        page.opened = true;
        page.beside = None;
    });
    move_to(window, place());
    bring_up(window);
    Ok(())
}

/// Shows the window with the page it has, next to the selection of its query if it has one.
fn show(window: HWND) {
    if let Some((selection, mode)) = page_of(window, |page| page.beside).flatten() {
        move_to(window, beside(selection, mode, &taken_places(window)));
    }
    bring_up(window);
}

/// Shows the window with the conversation behind the answer card, next to its selection.
pub fn show_conversation() {
    let behind = BROWSER.with(|browser| {
        let browser = browser.try_borrow().ok()?;
        browser.pages.iter().find(|page| page.behind_card).map(|page| page.window)
    });
    if let Some(window) = behind {
        show(window);
    }
}

/// Where the program's other windows in sight stand: the top left corners of their frames.
fn taken_places(window: HWND) -> Vec<(i32, i32)> {
    BROWSER.with(|browser| {
        let browser = browser.borrow();
        let others = browser.pages.iter().filter(|page| page.window != window && unsafe { IsWindowVisible(page.window) } != 0);
        others
            .map(|page| {
                let mut frame = RECT::default();
                unsafe { GetWindowRect(page.window, &mut frame) };
                (frame.left, frame.top)
            })
            .collect()
    })
}

/// Opens the address in the window without showing it, once WebView2 has made its page if it
/// hasn't yet.
fn load_in(window: HWND, url: &str) -> Result<(), String> {
    match controller_of(window) {
        Some(controller) => navigate(&controller, url).map_err(|e| e.message()),
        None => {
            with_page(window, |page| page.waiting = Some(url.to_string()));
            Ok(())
        }
    }
}

fn bring_up(window: HWND) {
    unsafe {
        if QUIET {
            ShowWindow(window, SW_SHOWNOACTIVATE);
            return;
        }
        ShowWindow(window, if IsIconic(window) != 0 { SW_RESTORE } else { SW_SHOW });
        SetForegroundWindow(window);
    }
    if let Some(controller) = controller_of(window) {
        let _ = unsafe { controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC) };
    }
}

/// Makes a new window, hidden, and has WebView2 make its page, starting WebView2 with the
/// program's own profile first if it hasn't started yet: the page comes a moment later.
fn new_page(spare: bool) -> Result<HWND, String> {
    let window = create_window().map_err(|e| format!("{} {e}.", tr("The window wasn't made:")))?;
    let icons = tray::give_icon(window);
    BROWSER.with(|browser| browser.borrow_mut().pages.push(Page::new(window, icons, spare)));
    let (environment, starting) = BROWSER.with(|browser| {
        let browser = browser.borrow();
        (browser.environment.clone(), browser.starting)
    });
    let started = match environment {
        Some(environment) => create_controller(&environment, window).map_err(|error| error.message()),
        None if starting => Ok(()),
        None => start_environment(),
    };
    // Without WebView2 the window is of no use.
    if let Err(problem) = started {
        destroy(window);
        return Err(problem);
    }
    Ok(window)
}

/// Starts WebView2 with the program's own profile: the windows waiting for it get their pages
/// once it has started.
fn start_environment() -> Result<(), String> {
    BROWSER.with(|browser| browser.borrow_mut().starting = true);
    let profile = shell::browser_profile();
    let _ = std::fs::create_dir_all(&profile);
    let profile = wide(&profile.to_string_lossy());
    let created = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(|result, environment| {
        match result.and_then(|()| environment.ok_or_else(|| E_POINTER.into())) {
            Ok(environment) => environment_ready(environment),
            Err(error) => environment_failed(&error),
        }
        Ok(())
    }));
    // Chromium slows down pages it takes for unseen, and the page behind an answer card is
    // never seen: it has to keep its pace all the same.
    let options = CoreWebView2EnvironmentOptions::default();
    #[allow(unused_mut)]
    let mut arguments = String::from(
        "--disable-background-timer-throttling --disable-renderer-backgrounding \
         --disable-backgrounding-occluded-windows --disable-features=CalculateNativeWinOcclusion",
    );
    // Tests may give the browser more: a recording in place of the microphone, for one.
    #[cfg(test)]
    if let Ok(more) = std::env::var("SCREENSHOT_BROWSER_ARGUMENTS") {
        arguments = format!("{arguments} {more}");
    }
    unsafe { options.set_additional_browser_arguments(arguments) };
    let options: ICoreWebView2EnvironmentOptions = options.into();
    let started = unsafe {
        CreateCoreWebView2EnvironmentWithOptions(ComText::null(), ComText(profile.as_ptr()), &options, &created)
    };
    if let Err(error) = started {
        BROWSER.with(|browser| browser.borrow_mut().starting = false);
        wanting_failed();
        let failed = tr("WebView2 didn't start:");
        return Err(format!("{failed} {}", error.message()));
    }
    Ok(())
}

/// WebView2 has started: the other windows of the program that waited for it have it, and the
/// Google windows that waited for their pages get them.
fn environment_ready(environment: ICoreWebView2Environment) {
    let waiting: Vec<HWND> = BROWSER.with(|browser| {
        let mut browser = browser.borrow_mut();
        browser.environment = Some(environment.clone());
        browser.starting = false;
        browser.pages.iter().filter(|page| page.controller.is_none()).map(|page| page.window).collect()
    });
    for then in WANTING.with(|wanting| wanting.take()) {
        then(Some(&environment));
    }
    for window in waiting {
        if let Err(error) = create_controller(&environment, window) {
            page_failed(window, &error.message());
        }
    }
}

/// Has WebView2 make the window's page.
fn create_controller(environment: &ICoreWebView2Environment, window: HWND) -> windows_core::Result<()> {
    let created = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(move |result, controller| {
        match result.and_then(|()| controller.ok_or_else(|| E_POINTER.into())) {
            Ok(controller) => controller_ready(window, controller),
            Err(error) => page_failed(window, &error.message()),
        }
        Ok(())
    }));
    unsafe { environment.CreateCoreWebView2Controller(ComWindow(window), &created) }
}

/// WebView2 has made the window's page: the page fills the window, and the address that waited
/// opens. A window gone meanwhile lets its page go.
fn controller_ready(window: HWND, controller: ICoreWebView2Controller) {
    let gone = BROWSER.with(|browser| browser.try_borrow().is_ok_and(|b| !b.pages.iter().any(|page| page.window == window)));
    if gone {
        let _ = unsafe { controller.Close() };
        return;
    }
    if let Err(error) = attach(&controller, window) {
        let _ = unsafe { controller.Close() };
        return page_failed(window, &error.message());
    }
    let waiting = with_page(window, |page| {
        page.controller = Some(controller.clone());
        page.waiting.take()
    })
    .flatten();
    if let Some(url) = waiting
        && let Err(error) = navigate(&controller, &url)
    {
        load_failed(window, &error.message());
    }
    for then in ASKING.with(|asking| asking.take()) {
        ask_cookies(&controller, then);
    }
}

/// Puts the page into the window and, if the window is shown, gives it the keyboard. The page is
/// dark until it is drawn, as the window is. Every page gets the program's tidying and speech
/// scripts before its own, the window's title follows the page's unless it has its question's,
/// a query waiting for the page runs its script as soon as the page's document is there, and
/// the script tells the program how it went. A link that would open a new window, or that leaves
/// Google, opens in the user's own browser. The microphone is the page's only for a question
/// asked aloud; otherwise WebView2 asks the user.
fn attach(controller: &ICoreWebView2Controller, window: HWND) -> windows_core::Result<()> {
    unsafe {
        controller.SetBounds(client_rect(window))?;
        controller.SetIsVisible(true)?;
        let dark = COREWEBVIEW2_COLOR { A: 255, R: 0x20, G: 0x20, B: 0x20 };
        controller.cast::<ICoreWebView2Controller2>()?.SetDefaultBackgroundColor(dark)?;
        let webview = controller.CoreWebView2()?;
        for script in [query::TIDY_SCRIPT.to_string(), query::sign_in_script(), SPEECH_SCRIPT.to_string()] {
            let script = wide(&script);
            let added = AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(|_, _| Ok(())));
            webview.AddScriptToExecuteOnDocumentCreated(ComText(script.as_ptr()), &added)?;
        }
        let mut token = 0;
        let new_window = NewWindowRequestedEventHandler::create(Box::new(|_, args| {
            if let Some(args) = args {
                let mut uri = ComString::null();
                args.Uri(&mut uri)?;
                args.SetHandled(true)?;
                open_outside(&take_pwstr(uri));
            }
            Ok(())
        }));
        webview.add_NewWindowRequested(&new_window, &mut token)?;
        let title_changed = DocumentTitleChangedEventHandler::create(Box::new(move |webview, _| {
            if let Some(webview) = webview
                && page_of(window, |page| page.titled) == Some(false)
            {
                let mut title = ComString::null();
                webview.DocumentTitle(&mut title)?;
                SetWindowTextW(window, wide(&take_pwstr(title)).as_ptr());
            }
            Ok(())
        }));
        webview.add_DocumentTitleChanged(&title_changed, &mut token)?;
        let document_ready = DOMContentLoadedEventHandler::create(Box::new(move |webview, args| {
            let mut id = 0;
            let navigation = args.and_then(|args| args.NavigationId(&mut id).ok()).map(|()| id);
            #[cfg(test)]
            tests::NAVIGATIONS.with(|all| all.borrow_mut().push(format!("document {id}")));
            if let Some(webview) = webview
                && is_current(window, navigation)
            {
                back_from_account(window, &webview);
                page_ready(window, &webview);
            }
            Ok(())
        }));
        webview.cast::<ICoreWebView2_2>()?.add_DOMContentLoaded(&document_ready, &mut token)?;
        let navigated = NavigationCompletedEventHandler::create(Box::new(move |_, args| {
            if let Some(args) = args {
                navigation_completed(window, &args);
            }
            Ok(())
        }));
        webview.add_NavigationCompleted(&navigated, &mut token)?;
        let starting = NavigationStartingEventHandler::create(Box::new(move |_, args| {
            if let Some(args) = args {
                navigation_starting(window, &args);
            }
            Ok(())
        }));
        webview.add_NavigationStarting(&starting, &mut token)?;
        let told = WebMessageReceivedEventHandler::create(Box::new(move |_, args| {
            if let Some(args) = args {
                let mut message = ComString::null();
                if args.TryGetWebMessageAsString(&mut message).is_ok() {
                    script_told(window, &take_pwstr(message));
                }
            }
            Ok(())
        }));
        webview.add_WebMessageReceived(&told, &mut token)?;
        let asked = PermissionRequestedEventHandler::create(Box::new(move |_, args| {
            if let Some(args) = args {
                let mut kind = COREWEBVIEW2_PERMISSION_KIND::default();
                args.PermissionKind(&mut kind)?;
                if kind == COREWEBVIEW2_PERMISSION_KIND_MICROPHONE && asking_aloud(window) {
                    args.SetState(COREWEBVIEW2_PERMISSION_STATE_ALLOW)?;
                }
            }
            Ok(())
        }));
        webview.add_PermissionRequested(&asked, &mut token)?;
        // A hidden window, behind an answer card, mustn't take the keyboard from the card.
        if IsWindowVisible(window) != 0 {
            controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC)?;
        }
    }
    Ok(())
}

/// A page's document is there: the query waiting for the page runs its script on it at once,
/// and the script itself waits for what it needs, and the window comes out if it waited for the
/// page. So does a query whose page was replaced before its script told how it went, as when
/// Google shows a captcha first and the page comes after it; but a question asked aloud doesn't
/// listen again by itself. A fresh AI Mode without a query is ready for one.
fn page_ready(window: HWND, webview: &ICoreWebView2) {
    let job = with_page(window, |page| {
        page.loading = false;
        page.job.take().or_else(|| page.running.take().filter(|job| job.listener.is_none()))
    })
    .flatten();
    let Some(job) = job else {
        let mut source = ComString::null();
        if unsafe { webview.Source(&mut source) }.is_ok() && is_fresh(&take_pwstr(source)) {
            with_page(window, |page| page.ready = Some(Instant::now()));
        }
        return;
    };
    let script = wide(&job.script);
    // The script answers with a message of its own; what it returns at once means nothing.
    let started = ExecuteScriptCompletedHandler::create(Box::new(|_, _| Ok(())));
    match unsafe { webview.ExecuteScript(ComText(script.as_ptr()), &started) } {
        Ok(()) => {
            let reveal = with_page(window, |page| {
                page.running = Some(job);
                std::mem::take(&mut page.reveal)
            });
            if reveal == Some(true) {
                show(window);
            }
        }
        Err(error) => job_failed(window, job, &error.message()),
    }
}

/// A page that has been on Google's account pages has come back from them: someone may have
/// signed in there, or out, and the spare page, made before, is loaded afresh.
fn back_from_account(window: HWND, webview: &ICoreWebView2) {
    let mut source = ComString::null();
    if unsafe { webview.Source(&mut source) }.is_err() || take_pwstr(source).starts_with(ACCOUNT) {
        return;
    }
    if with_page(window, |page| std::mem::take(&mut page.account) && !page.spare) == Some(true) {
        refresh_spare();
        app::check_account();
    }
}

/// A navigation has ended: if it failed, the query that waited for its page can't go on. One
/// that another navigation replaced, as the image search's results that move to the tab of exact
/// matches, hasn't failed, and an earlier one that a later navigation has made old news means
/// nothing now. A page that came, whatever the server answered with it, has the query's script
/// running on it, which tells how the query went: WebView2 takes any answer but success for a
/// failure, and Google serves its check that one is not a robot as "too many requests", a check
/// that often lets one through by itself and goes on to AI Mode.
fn navigation_completed(window: HWND, args: &ICoreWebView2NavigationCompletedEventArgs) {
    #[cfg(test)]
    {
        let (mut id, mut success, mut status) = (0u64, ComBool::default(), COREWEBVIEW2_WEB_ERROR_STATUS::default());
        let _ = unsafe { args.NavigationId(&mut id).and(args.IsSuccess(&mut success)).and(args.WebErrorStatus(&mut status)) };
        tests::NAVIGATIONS.with(|all| all.borrow_mut().push(format!("end {id} {} {}", success.as_bool(), status.0)));
    }
    let mut id = 0;
    let current = is_current(window, unsafe { args.NavigationId(&mut id) }.ok().map(|()| id));
    // The spare page's AI Mode has come, or won't now.
    if current {
        with_page(window, |page| page.loading = false);
    }
    let mut success = ComBool::default();
    if unsafe { args.IsSuccess(&mut success) }.is_ok() && success.as_bool() {
        return;
    }
    let mut status = COREWEBVIEW2_WEB_ERROR_STATUS::default();
    if unsafe { args.WebErrorStatus(&mut status) }.is_ok() && status == COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED {
        return;
    }
    if !current {
        return;
    }
    // The server's answer: nothing for a page that never came, as without a connection.
    let mut code = 0;
    let answered = args
        .cast::<ICoreWebView2NavigationCompletedEventArgs2>()
        .and_then(|args| unsafe { args.HttpStatusCode(&mut code) })
        .is_ok_and(|()| code != 0);
    // A page that didn't come, or broke off before it was whole, as one may over a connection
    // that a pause has worn out, is asked for once more: the query waits for it again, and its
    // script on WebView2's page of the error, if it began there, goes with that page.
    let outcome = with_page(window, |page| {
        if answered && page.running.is_some() {
            return None;
        }
        if (page.job.is_some() || page.running.is_some())
            && let Some(url) = page.again.take()
        {
            page.job = page.job.take().or_else(|| page.running.take());
            return Some(Err(url));
        }
        page.job.take().or_else(|| page.running.take()).map(Ok)
    })
    .flatten();
    match outcome {
        Some(Ok(job)) => job_failed(window, job, tr("Google's page didn't open.")),
        Some(Err(url)) => {
            if let Err(problem) = load_in(window, &url) {
                load_failed(window, &problem);
            }
        }
        None => {}
    }
}

/// A navigation is starting. The image search's results move to the tab of exact matches before
/// they load; a link that the user follows out of Google, to another site or through Google's
/// own way out to one, opens in the user's own browser instead of the window.
fn navigation_starting(window: HWND, args: &ICoreWebView2NavigationStartingEventArgs) {
    #[cfg(test)]
    {
        let (mut id, mut uri) = (0u64, ComString::null());
        let _ = unsafe { args.NavigationId(&mut id).and(args.Uri(&mut uri)) };
        tests::NAVIGATIONS.with(|all| all.borrow_mut().push(format!("start {id} {}", take_pwstr(uri))));
    }
    with_page(window, |page| page.ready = None);
    let mut uri = ComString::null();
    if unsafe { args.Uri(&mut uri) }.is_err() {
        return;
    }
    let uri = take_pwstr(uri);
    if uri.starts_with(ACCOUNT) {
        with_page(window, |page| page.account = true);
    }
    let searching = page_of(window, |page| page.running.as_ref().map(|job| job.mode == Mode::Images)).flatten();
    if searching == Some(true)
        && let Some(exact) = query::exact_matches(&uri)
    {
        let _ = unsafe { args.SetCancel(true) };
        with_page(window, |page| page.waiting = Some(exact));
        // WebView2 is in the middle of its event: the new navigation starts after it.
        unsafe { PostMessageW(window, WM_NAVIGATE, 0, 0) };
        return;
    }
    let (mut user, mut redirected) = (ComBool::default(), ComBool::default());
    let followed = unsafe { args.IsUserInitiated(&mut user).and(args.IsRedirected(&mut redirected)) }.is_ok()
        && user.as_bool()
        && !redirected.as_bool();
    // A server of the tests' own, on this computer, stands for Google.
    let local = cfg!(test) && uri.starts_with("http://127.0.0.1:");
    if followed && !local && query::leaves_google(&uri) {
        let _ = unsafe { args.SetCancel(true) };
        open_outside(&uri);
        return;
    }
    // The page goes there: what earlier navigations come to is old news from now on.
    let mut id = 0;
    if unsafe { args.NavigationId(&mut id) }.is_ok() {
        with_page(window, |page| page.navigation = page.navigation.max(id));
    }
}

/// Whether a navigation of the window's page is the one started last, or its number isn't
/// known: the document or the end of one that a later navigation has replaced is old news.
fn is_current(window: HWND, navigation: Option<u64>) -> bool {
    let last = page_of(window, |page| page.navigation);
    navigation.zip(last).is_none_or(|(navigation, last)| navigation >= last)
}

/// Opens the address in the user's own browser; tests only note it down.
fn open_outside(url: &str) {
    #[cfg(not(test))]
    let _ = shell::open(url);
    #[cfg(test)]
    tests::OPENED_OUTSIDE.with(|opened| opened.borrow_mut().push(url.to_string()));
}

/// The query's script has told how it went, or what it has heard of a question asked aloud.
fn script_told(window: HWND, message: &str) {
    let told = query::told(message);
    // What the program asked of a question asked aloud before its script could hear it goes now.
    if told == Told::Ready {
        if let Some(asked) = with_page(window, |page| page.told_page.take()).flatten() {
            post_page(window, asked);
        }
        return;
    }
    let running = page_of(window, |page| page.running.as_ref().map(|job| (job.mode, job.name.clone(), job.listener))).flatten();
    let Some((mode, name, listener)) = running else { return };
    match told {
        Told::Ready => {}
        Told::Listening => {
            if let Some(listener) = listener {
                tell(listener, Heard::Listening);
            }
        }
        // The words heard title the window.
        Told::Heard(words) => {
            if let Some(listener) = listener {
                set_title(window, &words);
                tell(listener, Heard::Words(words));
            }
        }
        // Nothing has gone, and the hidden window has nothing to show.
        Told::Silent => {
            take_running(window);
            if let Some(listener) = listener {
                tell(listener, Heard::Silent);
                release(window);
            }
        }
        Told::Sent => {
            take_running(window);
            if let Some(listener) = listener {
                tell(listener, Heard::Sent);
                show(window);
            }
        }
        // The card shows the answer of the page behind it alone.
        Told::Answer { text, complete } => {
            if mode == Mode::Answer && page_of(window, |page| page.behind_card) == Some(true) {
                card::answer(&text);
            }
            if complete {
                take_running(window);
            }
        }
        // The script runs again on the page that comes after Google's own; a question asked aloud
        // doesn't, so as not to listen by itself.
        Told::Show(why) => {
            if let Some(listener) = listener {
                take_running(window);
                tell_page(window, "abort");
                tell(listener, Heard::Failed(why.clone()));
            }
            show(window);
            leave_card(window);
            let open = tr("The window with it is open.");
            app::notify(&name, &format!("{why} {open}"));
        }
        Told::Failed(why) => {
            if let Some(job) = take_running(window) {
                job_failed(window, job, &why);
            }
        }
    }
}

fn take_running(window: HWND) -> Option<Job> {
    with_page(window, |page| page.running.take()).flatten()
}

/// The query couldn't be finished in its window: the window comes out, instead of the card for an
/// answer, so that the user sees what happened. A question asked aloud stops listening instead,
/// and its hidden window goes.
fn job_failed(window: HWND, job: Job, why: &str) {
    if job.listener.is_some() {
        tell_page(window, "abort");
        release(window);
    } else {
        show(window);
        leave_card(window);
    }
    give_up(job, why, window);
}

/// Tells that the query couldn't be finished: the frozen screen says so of a question asked
/// aloud, and any other query's screenshot goes to the clipboard, so that Ctrl+V finishes it by
/// hand.
fn give_up(job: Job, why: &str, owner: HWND) {
    if let Some(listener) = job.listener {
        return tell(listener, Heard::Failed(why.into()));
    }
    #[cfg(test)]
    tests::FAILED.with(|failed| failed.borrow_mut().push(why.to_string()));
    // The tests leave the user's clipboard alone.
    let copied = !QUIET && clipboard::put(owner, &job.picture).is_ok();
    let how = if copied { format!(" {}", query::paste_hint(job.mode)) } else { String::new() };
    app::notify(&job.name, &format!("{why}{how}"));
}

/// The address couldn't be opened in the window: its query gives up, or the user is told.
fn load_failed(window: HWND, problem: &str) {
    match with_page(window, |page| page.job.take()).flatten() {
        Some(job) => job_failed(window, job, problem),
        None => app::notify(window_not_open(), problem),
    }
}

/// WebView2 couldn't make the window's page: the window goes, and a query waiting for it gives
/// up.
fn page_failed(window: HWND, problem: &str) {
    let job = with_page(window, |page| page.job.take()).flatten();
    leave_card(window);
    match job {
        Some(job) => give_up(job, problem, window),
        None => app::notify(window_not_open(), problem),
    }
    destroy(window);
}

/// The window comes out in place of the answer card, if it is behind it: the card closes, and the
/// window stays.
fn leave_card(window: HWND) {
    if with_page(window, |page| std::mem::take(&mut page.behind_card)) == Some(true) {
        card::close();
    }
}

/// The answer card has closed: the hidden window behind it goes for good, with its answer for no
/// card any more; a window that the user has seen stays.
pub fn card_closed() {
    let behind = BROWSER.with(|browser| {
        let mut browser = browser.try_borrow_mut().ok()?;
        let page = browser.pages.iter_mut().find(|page| page.behind_card)?;
        page.behind_card = false;
        Some(page.window)
    });
    if let Some(window) = behind
        && unsafe { IsWindowVisible(window) } == 0
    {
        with_page(window, |page| {
            page.job = None;
            page.running = None;
        });
        release(window);
    }
}

/// Lets the window go for good once WebView2's event has ended, if it is hidden by then.
fn release(window: HWND) {
    unsafe { PostMessageW(window, WM_RELEASE, 0, 0) };
}

/// Lets the window go for good, with its page.
fn destroy(window: HWND) {
    let page = BROWSER.with(|browser| {
        let mut browser = browser.try_borrow_mut().ok()?;
        let index = browser.pages.iter().position(|page| page.window == window)?;
        Some(browser.pages.remove(index))
    });
    if let Some(page) = page {
        let_go(page);
    }
}

/// Lets a window and its page go. WebView2 has to be let go while the thread that made it still
/// runs: let go later, as the thread's own data is dropped, it brings the program down.
fn let_go(page: Page) {
    if let Some(controller) = &page.controller {
        let _ = unsafe { controller.Close() };
    }
    unsafe { DestroyWindow(page.window) };
    for icon in page.icons {
        unsafe { DestroyIcon(icon) };
    }
}

/// Closes the windows for good as the program exits.
pub fn close() {
    let (pages, timer) = BROWSER.with(|browser| {
        let mut browser = browser.borrow_mut();
        browser.environment = None;
        browser.starting = false;
        (std::mem::take(&mut browser.pages), std::mem::take(&mut browser.spare_timer))
    });
    if timer != 0 {
        unsafe { KillTimer(null_mut(), timer) };
    }
    for page in pages {
        let_go(page);
    }
}

fn navigate(controller: &ICoreWebView2Controller, url: &str) -> windows_core::Result<()> {
    let url = wide(url);
    unsafe { controller.CoreWebView2()?.Navigate(ComText(url.as_ptr())) }
}

fn failed(error: &windows_core::Error) {
    app::notify(window_not_open(), &error.message());
}

/// The title of a notification that the Google window didn't open, for the other windows too.
pub fn window_not_open() -> &'static str {
    tr("The Google window didn't open")
}

/// Changes the window's page as `change` says and gives back what it returns; nothing if the
/// window has no page, or while WebView2 has called in, in the middle of a change.
fn with_page<T>(window: HWND, change: impl FnOnce(&mut Page) -> T) -> Option<T> {
    BROWSER.with(|browser| {
        let mut browser = browser.try_borrow_mut().ok()?;
        browser.pages.iter_mut().find(|page| page.window == window).map(change)
    })
}

/// What `look` reads of the window's page.
fn page_of<T>(window: HWND, look: impl FnOnce(&Page) -> T) -> Option<T> {
    BROWSER.with(|browser| {
        let browser = browser.try_borrow().ok()?;
        browser.pages.iter().find(|page| page.window == window).map(look)
    })
}

/// The controller of the window's page, taken out so that no borrow is held while WebView2 is
/// called: it may call back into the program.
fn controller_of(window: HWND) -> Option<ICoreWebView2Controller> {
    page_of(window, |page| page.controller.clone()).flatten()
}

fn webview_of(window: HWND) -> Option<ICoreWebView2> {
    controller_of(window).and_then(|controller| unsafe { controller.CoreWebView2() }.ok())
}

/// Titles the window with its question, on one line and not too long, as Google titles all its
/// pages alike: the windows of several questions can be told apart on the taskbar.
fn set_title(window: HWND, question: &str) {
    let line = question.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.is_empty() {
        return;
    }
    let short = match line.char_indices().nth(TITLE_LENGTH) {
        Some((end, _)) => format!("{}…", line[..end].trim_end()),
        None => line,
    };
    with_page(window, |page| page.titled = true);
    unsafe { SetWindowTextW(window, wide(&format!("{short} — ainitShot")).as_ptr()) };
}

/// Where the spare page goes: AI Mode, or in tests the page of the test's own server that stands
/// for it.
fn spare_address() -> String {
    #[cfg(test)]
    if let Some(address) = tests::SPARE.with(|spare| spare.borrow().clone()) {
        return address;
    }
    query::AI_MODE.to_string()
}

/// Whether a page is a fresh AI Mode, ready for a question.
fn is_fresh(source: &str) -> bool {
    #[cfg(test)]
    if tests::SPARE.with(|spare| spare.borrow().as_deref() == Some(source)) {
        return true;
    }
    query::is_fresh_ai_mode(source)
}

fn create_window() -> io::Result<HWND> {
    unsafe {
        let instance = GetModuleHandleW(null());
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: CreateSolidBrush(BACKGROUND),
            lpszClassName: CLASS,
            ..Default::default()
        };
        // The class stays registered after the first window.
        RegisterClassExW(&class);
        let place = seen_at(place());
        let quiet = if QUIET { WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE } else { 0 };
        let window = CreateWindowExW(
            quiet,
            CLASS,
            w!("ainitShot"),
            WS_OVERLAPPEDWINDOW,
            place.left,
            place.top,
            place.width(),
            place.height(),
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        if window.is_null() {
            return Err(last_error());
        }
        // A dark title bar, like the rest of the program.
        let dark: BOOL = 1;
        DwmSetWindowAttribute(window, DWMWA_USE_IMMERSIVE_DARK_MODE as u32, (&dark as *const BOOL).cast(), size_of::<BOOL>() as u32);
        Ok(window)
    }
}

/// Where the window goes next to the selection, given on the desktop: where the answer card
/// would, at the size of a conversation or of the image search's results, inside the work area
/// of the selection's monitor. Another window of the program standing exactly there, at one of
/// the `taken` places, stays in sight: the window goes a step lower and to the right of it, as
/// far as the work area lets it.
fn beside(selection: Rect, mode: Mode, taken: &[(i32, i32)]) -> Rect {
    let area = RECT { left: selection.left, top: selection.top, right: selection.right, bottom: selection.bottom };
    let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    let (mut dpi, mut unused) = (96, 96);
    let mut frame = RECT::default();
    unsafe {
        let monitor = MonitorFromRect(&area, MONITOR_DEFAULTTONEAREST);
        GetMonitorInfoW(monitor, &mut info);
        GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut unused);
        AdjustWindowRectExForDpi(&mut frame, WS_OVERLAPPEDWINDOW, 0, 0, dpi);
    }
    let work = info.rcWork;
    let screen = Rect::new(work.left, work.top, work.right, work.bottom);
    let scale = dpi as f32 / 96.0;
    let size = if mode == Mode::Images { layout::RESULTS } else { layout::CONVERSATION };
    let mut seen = layout::place(selection, layout::window_size(size, scale, screen), scale, screen);
    // Windows draws the window's borders a pixel thin and keeps the rest of them, by which the
    // window is resized, unseen: the window reaches that much past what is seen of it.
    let (side, bottom) = (-frame.left - 1, frame.bottom - 1);
    let whole = |seen: Rect| Rect::new(seen.left - side, seen.top, seen.right + side, seen.bottom + bottom);
    let corner = |seen: Rect| {
        let place = seen_at(whole(seen));
        (place.left, place.top)
    };
    let step = (STEP as f32 * scale).round() as i32;
    while step > 0 && taken.contains(&corner(seen)) && seen.right + step <= work.right && seen.bottom + step <= work.bottom {
        seen = Rect::new(seen.left + step, seen.top + step, seen.right + step, seen.bottom + step);
    }
    whole(seen)
}

fn move_to(window: HWND, place: Rect) {
    let place = seen_at(place);
    unsafe {
        if IsZoomed(window) != 0 {
            ShowWindow(window, SW_RESTORE);
        }
        // Moved to a monitor of another scale, the window first takes the size that Windows
        // suggests for it there; the second move gives it its own.
        for _ in 0..2 {
            SetWindowPos(window, null_mut(), place.left, place.top, place.width(), place.height(), SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }
}

/// Where the window goes: the place itself, or as far right of all the screens while the tests
/// look at it, at the same height and size (see `QUIET`).
fn seen_at(place: Rect) -> Rect {
    if !QUIET {
        return place;
    }
    let away = out_of_sight(RECT { left: place.left, top: place.top, right: place.right, bottom: place.bottom });
    Rect::new(away.left, away.top, away.right, away.bottom)
}

/// Where the window appears when no query brings it: in the middle of the monitor under the
/// mouse, as tall as most of it and about half as wide.
fn place() -> Rect {
    let mut cursor = POINT::default();
    let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe {
        GetCursorPos(&mut cursor);
        GetMonitorInfoW(MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST), &mut info);
    }
    let work = info.rcWork;
    let (width, height) = (work.right - work.left, work.bottom - work.top);
    let (w, h) = ((width * 45 / 100).max(width.min(960)), height * 88 / 100);
    let (left, top) = (work.left + (width - w) / 2, work.top + (height - h) / 2);
    Rect::new(left, top, left + w, top + h)
}

fn client_rect(window: HWND) -> ComRect {
    let mut rect = RECT::default();
    unsafe { GetClientRect(window, &mut rect) };
    ComRect { left: rect.left, top: rect.top, right: rect.right, bottom: rect.bottom }
}

unsafe extern "system" fn window_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_SIZE => {
            if let Some(controller) = controller_of(window) {
                let _ = unsafe { controller.SetBounds(client_rect(window)) };
            }
        }
        WM_MOVE => {
            // Lists and hints of the page open in the right place.
            if let Some(controller) = controller_of(window) {
                let _ = unsafe { controller.NotifyParentWindowPositionChanged() };
            }
        }
        WM_ACTIVATE => {
            let handled = unsafe { DefWindowProcW(window, message, wparam, lparam) };
            if (wparam & 0xFFFF) as u32 != WA_INACTIVE
                && let Some(controller) = controller_of(window)
            {
                let _ = unsafe { controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC) };
            }
            return handled;
        }
        WM_DPICHANGED => {
            let suggested = unsafe { &*(lparam as *const RECT) };
            let (w, h) = (suggested.right - suggested.left, suggested.bottom - suggested.top);
            unsafe { SetWindowPos(window, null_mut(), suggested.left, suggested.top, w, h, SWP_NOZORDER | SWP_NOACTIVATE) };
        }
        WM_NAVIGATE => {
            let waiting = with_page(window, |page| page.waiting.take()).flatten();
            if let (Some(url), Some(controller)) = (waiting, controller_of(window))
                && let Err(error) = navigate(&controller, &url)
            {
                load_failed(window, &error.message());
            }
        }
        WM_RELEASE => {
            // Shown meanwhile, the window stays.
            if unsafe { IsWindowVisible(window) } == 0 && page_of(window, |page| page.spare) == Some(false) {
                destroy(window);
            }
        }
        WM_CLOSE => {
            // Closed, the window goes for good with its page, which stops listening, if it did.
            // Closed on Google's account pages, where the user may have signed in or out, it has
            // the spare page, made before, loaded afresh.
            let account = page_of(window, |page| page.account).unwrap_or(false);
            destroy(window);
            app::check_account();
            if account {
                refresh_spare();
            } else {
                prepare();
            }
        }
        _ => return unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
    0
}

#[cfg(test)]
pub(super) mod tests {
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
    use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetWindowTextW, IsWindow, MSG, PM_REMOVE, PeekMessageW,
    };

    use super::super::gdi;
    use super::*;
    use crate::settings::{Mode, QueryButton};

    thread_local! {
        /// The addresses that the windows gave to the user's own browser.
        pub static OPENED_OUTSIDE: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        /// What the page told the window that listens to a question asked aloud.
        static HEARD: RefCell<Vec<Heard>> = const { RefCell::new(Vec::new()) };
        /// Why queries failed, as the user would be told.
        pub static FAILED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        /// The windows' navigations as they start, have their document and end, in order.
        pub static NAVIGATIONS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        /// The address of the test's own server that stands for AI Mode on the spare page.
        pub static SPARE: RefCell<Option<String>> = const { RefCell::new(None) };
    }

    /// AI Mode as the program's script sees it: the question field with its send button, which
    /// takes a pasted picture, and the answer in a block of its own, its turn marked complete a
    /// moment after the question goes.
    const AI_MODE_PAGE: &str = r#"<!doctype html><title>AI Mode</title>
<textarea></textarea><button data-xid="input-plate-send-button" disabled>Send</button>
<script>
  const field = document.querySelector("textarea"), send = document.querySelector("button");
  field.addEventListener("paste", (event) => {
    for (const file of event.clipboardData.files) {
      const chip = document.createElement("span");
      chip.setAttribute("aria-label", file.name);
      document.body.append(chip);
      send.disabled = false;
    }
  });
  send.addEventListener("click", () => {
    const turn = document.createElement("div");
    turn.setAttribute("data-xid", "aim-mars-turn-root");
    const block = document.createElement("div");
    block.setAttribute("data-container-id", "main-col");
    block.textContent = "Bonjour";
    turn.append(block);
    document.body.append(turn);
    setTimeout(() => turn.setAttribute("data-asrc", "true"), 300);
  });
</script>"#;

    /// How many questions have gone on the page of [`AI_MODE_PAGE`].
    const ASKED: &str = r#"document.querySelectorAll('[data-xid="aim-mars-turn-root"]').length"#;

    /// Google's check that one is not a robot, which it serves as "too many requests", and which
    /// may let one through by itself.
    const CHECK_PAGE: &str = "<!doctype html><title>Sorry</title><p>Are you a robot?</p>";

    /// A web server of the test's own on this computer, on the port it returns:
    /// - "/slow" sends the start of its page at once and the rest a moment later, as a page
    ///   coming from far away does;
    /// - "/ai" is AI Mode ([`AI_MODE_PAGE`]);
    /// - "/sorry/pass" is Google's check, which goes on to AI Mode by itself after a second, and
    ///   "/sorry/wait" the check that waits for the user ([`CHECK_PAGE`]);
    /// - "/ai-flaky" is AI Mode streamed whose connection breaks before the stream's end the
    ///   first time, and that comes whole after;
    /// - any other path gets a whole page at once.
    fn serve() -> u16 {
        use std::io::{Read, Write};
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let broken = Arc::new(AtomicBool::new(false));
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let broken = broken.clone();
                std::thread::spawn(move || {
                    let mut request = [0u8; 4096];
                    let read = stream.read(&mut request).unwrap_or(0);
                    let request = String::from_utf8_lossy(&request[..read]).into_owned();
                    let path = request.split(' ').nth(1).unwrap_or("/").to_string();
                    let status = if path.starts_with("/sorry") { "429 Too Many Requests" } else { "200 OK" };
                    if path == "/ai-flaky" && !broken.swap(true, Ordering::SeqCst) {
                        let head = "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\n\
                                    Transfer-Encoding: chunked\r\n\r\n";
                        let chunk = format!("{:x}\r\n{AI_MODE_PAGE}\r\n", AI_MODE_PAGE.len());
                        let _ = stream.write_all(head.as_bytes()).and_then(|()| stream.write_all(chunk.as_bytes()));
                        let _ = stream.flush();
                        std::thread::sleep(Duration::from_millis(500));
                        return;
                    }
                    let head = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\n\
                         Connection: close\r\n\r\n"
                    );
                    let _ = stream.write_all(head.as_bytes());
                    let page = match path.as_str() {
                        "/slow" => {
                            // Enough for the browser to take it for the window's new document.
                            let start = format!("<!doctype html><title>slow</title><!--{}-->", " ".repeat(8192));
                            let _ = stream.write_all(start.as_bytes()).and_then(|()| stream.flush());
                            std::thread::sleep(Duration::from_millis(1500));
                            "<p>slow</p>".to_string()
                        }
                        "/ai" | "/ai-flaky" => AI_MODE_PAGE.to_string(),
                        "/sorry/pass" => format!("{CHECK_PAGE}<script>setTimeout(() => location.replace('/ai'), 1000)</script>"),
                        "/sorry/wait" => CHECK_PAGE.to_string(),
                        _ => "<!doctype html><title>page</title><p>page</p>".to_string(),
                    };
                    let _ = stream.write_all(page.as_bytes());
                });
            }
        });
        port
    }

    /// Makes the spare page ready on the server's AI Mode, out of sight, and gives the server's
    /// port.
    fn start_on_server() -> u16 {
        unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };
        let port = serve();
        SPARE.with(|spare| *spare.borrow_mut() = Some(format!("http://127.0.0.1:{port}/ai")));
        prepare();
        let started = pump_until(30, is_ready);
        let first = NAVIGATIONS.with(|all| all.borrow_mut().drain(..).collect::<Vec<_>>().join(", "));
        assert!(started, "the spare page didn't load: {first}");
        port
    }

    /// A query of a button for AI Mode as the program makes it, for AI Mode on the test's server.
    fn ai_mode_query(name: &str, mode: Mode, prompt: &str) -> Job {
        let picture = Image { width: 1, height: 1, pixels: vec![255; 4] };
        let button = button(name, mode, prompt);
        let query::Way::Window { script, .. } = query::send(&button, &picture.to_png().unwrap()) else { panic!("no window") };
        let script = script.replace("\"www.google.com\"", "\"127.0.0.1\"");
        assert!(script.contains("\"127.0.0.1\""), "the script names no host of AI Mode");
        Job { name: button.name, question: button.prompt, script, picture, mode, listener: None }
    }

    /// Translate's query, whose answer goes to the card.
    fn translation() -> Job {
        ai_mode_query("Перевести", Mode::Answer, "Translate the text in the picture into French.")
    }

    /// "Ask Google"'s query, whose window comes out with the conversation.
    fn conversation() -> Job {
        ai_mode_query("Спросить Google", Mode::Chat, "What is in the picture?")
    }

    /// Google lets Translate through its check by itself, as it may for someone it has seen pass
    /// the check before: the check's page, served as "too many requests", goes on to AI Mode, and
    /// the query goes on there to its answer, the window staying hidden.
    #[test]
    #[ignore]
    fn a_check_that_lets_one_through_by_itself_leaves_the_query_going() {
        let port = start_on_server();
        ask(&format!("http://127.0.0.1:{port}/sorry/pass"), translation(), None).unwrap();
        let done = pump_until(30, idle);
        let shown = unsafe { IsWindowVisible(window().unwrap()) } != 0;
        let failed = FAILED.with(|failed| failed.borrow().clone());
        let navigations = NAVIGATIONS.with(|all| all.borrow().join(", "));
        let page = evaluate("document.title + ' ' + document.querySelectorAll('[data-asrc]').length");
        close();
        assert!(done, "the query didn't finish: {navigations}");
        assert!(failed.is_empty() && !shown, "failed: {failed:?}, the window shown: {shown}; {navigations}");
        assert_eq!(page, "\"AI Mode 1\"", "the question didn't go: {navigations}");
    }

    /// AI Mode's connection breaks before the end of its stream, as one may after a pause, and
    /// the page never says it is whole: the page is asked for once more, as the user would, and
    /// the query goes on there to its answer, the window staying hidden.
    #[test]
    #[ignore]
    fn a_page_that_breaks_off_is_asked_for_once_more() {
        let port = start_on_server();
        let address = format!("http://127.0.0.1:{port}/ai-flaky");
        ask(&address, translation(), None).unwrap();
        let done = pump_until(30, idle);
        let shown = unsafe { IsWindowVisible(window().unwrap()) } != 0;
        let failed = FAILED.with(|failed| failed.borrow().clone());
        let navigations = NAVIGATIONS.with(|all| all.borrow().join(", "));
        let page = evaluate("document.title + ' ' + document.querySelectorAll('[data-asrc]').length");
        close();
        assert!(navigations.contains(" false "), "the stream didn't break: {navigations}");
        assert_eq!(navigations.matches(&address).count(), 2, "not asked for once more: {navigations}");
        assert!(done, "the query didn't finish: {navigations}");
        assert!(failed.is_empty() && !shown, "failed: {failed:?}, the window shown: {shown}; {navigations}");
        assert_eq!(page, "\"AI Mode 1\"", "the question didn't go: {navigations}");
    }

    /// A page that doesn't come at all, where nobody answers, is asked for once more, and then
    /// the query gives up as before: the window comes out, and the user is told.
    #[test]
    #[ignore]
    fn a_page_that_doesnt_come_fails_after_one_more_try() {
        start_on_server();
        let nobody = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let address = format!("http://127.0.0.1:{nobody}/ai");
        ask(&address, translation(), None).unwrap();
        let done = pump_until(30, idle);
        let shown = unsafe { IsWindowVisible(window().unwrap()) } != 0;
        let failed = FAILED.with(|failed| failed.borrow().clone());
        let navigations = NAVIGATIONS.with(|all| all.borrow().join(", "));
        close();
        assert_eq!(navigations.matches(&address).count(), 2, "not asked for once more: {navigations}");
        assert!(done && failed.len() == 1 && shown, "failed: {failed:?}, the window shown: {shown}; {navigations}");
    }

    /// Google's check waits for the user: after a few seconds the window comes out with it, and
    /// once the user is through, the query goes on in AI Mode to its answer.
    #[test]
    #[ignore]
    fn a_check_that_waits_shows_the_window_and_the_query_goes_on_after_it() {
        let port = start_on_server();
        ask(&format!("http://127.0.0.1:{port}/sorry/wait"), translation(), None).unwrap();
        let shown = pump_until(20, || unsafe { IsWindowVisible(window().unwrap()) } != 0);
        let waiting = !idle();
        let failed_early = FAILED.with(|failed| failed.borrow().clone());
        // The user passes the check, and Google goes on to AI Mode.
        evaluate("location.replace('/ai')");
        let done = pump_until(30, idle);
        let failed = FAILED.with(|failed| failed.borrow().clone());
        let navigations = NAVIGATIONS.with(|all| all.borrow().join(", "));
        let page = evaluate("document.title + ' ' + document.querySelectorAll('[data-asrc]').length");
        close();
        assert!(shown, "the window didn't come out with the check: {navigations}");
        assert!(waiting && failed_early.is_empty(), "the query gave up at the check: {failed_early:?}; {navigations}");
        assert!(done && failed.is_empty(), "the query failed after the check: {failed:?}; {navigations}");
        assert_eq!(page, "\"AI Mode 1\"", "the question didn't go: {navigations}");
    }

    /// A query whose page comes while an earlier page is still loading on the spare page, as when
    /// the hotkey has had it loaded afresh after half an hour and Translate follows at once: the
    /// earlier page's end isn't the query's, and the query goes on to its answer, the window
    /// staying hidden. On a server of the test's own, out of sight: `LOCALAPPDATA=<folder> cargo
    /// test -p ainitshot -- win::browser::tests::a_query_outlives_the_page_it_replaced --exact
    /// --ignored`.
    #[test]
    #[ignore]
    fn a_query_outlives_the_page_it_replaced() {
        let port = start_on_server();
        let address = |path: &str| format!("http://127.0.0.1:{port}{path}");
        // The spare page loads afresh, and it takes its time...
        load_in(spare_window().unwrap(), &address("/slow")).unwrap();
        let since = Instant::now();
        pump_until(2, || since.elapsed() > Duration::from_millis(300));
        // ...while Translate sends its screenshot on a page of its own; its script takes a moment.
        let script = "setTimeout(() => window.chrome.webview.postMessage('sent'), 1500)".to_string();
        let picture = Image { width: 1, height: 1, pixels: vec![255; 4] };
        let job = Job { name: "Перевести".into(), question: "Перевести".into(), script, picture, mode: Mode::Answer, listener: None };
        ask(&address("/page"), job, None).unwrap();
        let done = pump_until(10, idle);
        // The slow page's end may come later still.
        let since = Instant::now();
        pump_until(4, || since.elapsed() > Duration::from_secs(3));
        let shown = unsafe { IsWindowVisible(window().unwrap()) } != 0;
        let failed = FAILED.with(|failed| failed.borrow().clone());
        let navigations = NAVIGATIONS.with(|all| all.borrow().join(", "));
        close();
        assert!(done, "the query didn't finish: {navigations}");
        assert!(failed.is_empty() && !shown, "the query failed: {failed:?}, the window shown: {shown}; {navigations}");
    }

    /// A question for AI Mode takes the spare page, a fresh AI Mode loaded hidden in the
    /// background, and goes on it at once, loading nothing: its window comes out titled with the
    /// question, and another spare page is made a moment later. On a server of the test's own,
    /// out of sight: `LOCALAPPDATA=<folder> cargo test -p ainitshot --
    /// win::browser::tests::a_question_takes_the_spare_page --exact --ignored`.
    #[test]
    #[ignore]
    fn a_question_takes_the_spare_page() {
        let port = start_on_server();
        let ai_mode = format!("http://127.0.0.1:{port}/ai");
        let spare = spare_window().unwrap();
        ask(&ai_mode, conversation(), None).unwrap();
        let taken = window() == Some(spare) && spare_window().is_none();
        let done = pump_until(10, idle);
        let loaded = NAVIGATIONS.with(|all| all.borrow().iter().filter(|n| n.starts_with("start ")).count());
        let shown = unsafe { IsWindowVisible(spare) } != 0;
        let page = evaluate(&format!("document.title + ' ' + {ASKED}"));
        let title = title_of(spare);
        let another = pump_until(15, || is_ready() && spare_window().is_some_and(|window| window != spare));
        let navigations = NAVIGATIONS.with(|all| all.borrow().join(", "));
        close();
        assert!(taken, "the question didn't take the spare page");
        assert!(done && shown, "the question didn't go: {navigations}");
        assert_eq!(loaded, 0, "the question loaded its page: {navigations}");
        assert_eq!(page, "\"AI Mode 1\"", "the question didn't go: {navigations}");
        assert_eq!(title, "What is in the picture? — ainitShot");
        assert!(another, "no other spare page was made: {navigations}");
    }

    /// Two questions one after the other have a window each, both out with their conversations,
    /// the second a step aside from the first next to the same selection: the second, asked
    /// before another spare page has been made, loads AI Mode in a window of its own. On a server
    /// of the test's own, out of sight: `LOCALAPPDATA=<folder> cargo test -p ainitshot --
    /// win::browser::tests::two_questions_have_two_windows --exact --ignored`.
    #[test]
    #[ignore]
    fn two_questions_have_two_windows() {
        let port = start_on_server();
        let ai_mode = format!("http://127.0.0.1:{port}/ai");
        let selection = Some(Rect::new(300, 200, 780, 290));
        ask(&ai_mode, conversation(), selection).unwrap();
        let first = window().unwrap();
        ask(&ai_mode, conversation(), selection).unwrap();
        let second = window().unwrap();
        let done = pump_until(15, idle);
        let asked = |window: HWND| evaluate_on(&webview_of(window).unwrap(), ASKED);
        let (one, two) = (asked(first), asked(second));
        let shown = [first, second].map(|window| unsafe { IsWindowVisible(window) } != 0);
        let [top_one, top_two] = [first, second].map(|window| {
            let mut frame = RECT::default();
            unsafe { GetWindowRect(window, &mut frame) };
            frame.top
        });
        let navigations = NAVIGATIONS.with(|all| all.borrow().join(", "));
        close();
        assert_ne!(first, second, "one window for both questions");
        assert!(done, "the questions didn't finish: {navigations}");
        assert_eq!((one.as_str(), two.as_str()), ("1", "1"), "a question didn't go: {navigations}");
        assert_eq!(shown, [true, true], "a window didn't come out");
        assert!(top_two > top_one, "the second window covers the first: {top_one}, {top_two}");
    }

    /// A window closed goes for good with its page, and the spare page stays. On a server of the
    /// test's own, out of sight: `LOCALAPPDATA=<folder> cargo test -p ainitshot --
    /// win::browser::tests::a_closed_window_goes_for_good --exact --ignored`.
    #[test]
    #[ignore]
    fn a_closed_window_goes_for_good() {
        let port = start_on_server();
        ask(&format!("http://127.0.0.1:{port}/ai"), conversation(), None).unwrap();
        let window = window().unwrap();
        let done = pump_until(10, idle);
        unsafe { PostMessageW(window, WM_CLOSE, 0, 0) };
        let gone = pump_until(5, || unsafe { IsWindow(window) } == 0);
        let kept = BROWSER.with(|browser| browser.borrow().pages.iter().any(|page| page.window == window));
        let spare = pump_until(15, is_ready);
        close();
        assert!(done, "the question didn't go");
        assert!(gone && !kept, "the window stays");
        assert!(spare, "no spare page after the window closed");
    }

    /// The hidden window behind the answer card goes for good as the card closes, and a window
    /// that the user has seen the conversation in stays. On a server of the test's own, out of
    /// sight: `LOCALAPPDATA=<folder> cargo test -p ainitshot --
    /// win::browser::tests::the_window_behind_the_card_goes_with_it --exact --ignored`.
    #[test]
    #[ignore]
    fn the_window_behind_the_card_goes_with_it() {
        let port = start_on_server();
        let ai_mode = format!("http://127.0.0.1:{port}/ai");
        let selection = Some(Rect::new(300, 200, 780, 290));
        ask(&ai_mode, translation(), selection).unwrap();
        let unseen = window().unwrap();
        let answered = pump_until(10, idle);
        let hidden = unsafe { IsWindowVisible(unseen) } == 0;
        card_closed();
        let unseen_gone = pump_until(5, || unsafe { IsWindow(unseen) } == 0);
        ask(&ai_mode, translation(), selection).unwrap();
        let seen = window().unwrap();
        let answered_again = pump_until(15, idle);
        show_conversation();
        card_closed();
        pump_until(1, || false);
        let seen_stays = unsafe { IsWindow(seen) != 0 && IsWindowVisible(seen) != 0 };
        let navigations = NAVIGATIONS.with(|all| all.borrow().join(", "));
        close();
        assert!(answered && answered_again, "a translation didn't come: {navigations}");
        assert!(hidden, "the window behind the card came out");
        assert!(unseen_gone, "the window that nobody saw stays");
        assert!(seen_stays, "the window that the user saw went");
    }

    unsafe extern "system" fn listener_proc(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if message == WM_HEARD {
            let heard = unsafe { Box::from_raw(lparam as *mut Heard) };
            HEARD.with(|all| all.borrow_mut().push(*heard));
            return 0;
        }
        unsafe { DefWindowProcW(window, message, wparam, lparam) }
    }

    /// A window that only hears what the page tells of a question asked aloud, as the frozen
    /// screen does.
    fn listener() -> HWND {
        use windows_sys::Win32::UI::WindowsAndMessaging::HWND_MESSAGE;
        let class = w!("ainitShot.test.listener");
        unsafe {
            let instance = GetModuleHandleW(null());
            let description = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(listener_proc),
                hInstance: instance,
                lpszClassName: class,
                ..Default::default()
            };
            RegisterClassExW(&description);
            CreateWindowExW(0, class, w!(""), 0, 0, 0, 0, 0, HWND_MESSAGE, null_mut(), instance, null())
        }
    }

    /// Whether the page has told something of the question asked aloud.
    fn heard(what: impl Fn(&Heard) -> bool) -> bool {
        HEARD.with(|all| all.borrow().iter().any(what))
    }

    fn all_heard() -> String {
        HEARD.with(|all| format!("{:?}", all.borrow()))
    }

    /// Handles the windows' messages until `done` says so, at most for `seconds`; says whether
    /// it was done.
    pub(crate) fn pump_until(seconds: u64, mut done: impl FnMut() -> bool) -> bool {
        let start = Instant::now();
        while !done() {
            if start.elapsed() > Duration::from_secs(seconds) {
                return false;
            }
            let mut message = MSG::default();
            while unsafe { PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) } != 0 {
                unsafe { DispatchMessageW(&message) };
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        true
    }

    /// No query waits for its page or runs on it.
    fn idle() -> bool {
        BROWSER.with(|browser| browser.borrow().pages.iter().all(|page| page.job.is_none() && page.running.is_none()))
    }

    /// The window given its work last: a query's or one opened at an address.
    fn window() -> Option<HWND> {
        BROWSER.with(|browser| browser.borrow().pages.iter().rev().find(|page| !page.spare).map(|page| page.window))
    }

    /// The spare page's window.
    fn spare_window() -> Option<HWND> {
        BROWSER.with(|browser| browser.borrow().pages.iter().find(|page| page.spare).map(|page| page.window))
    }

    /// The controller of the page of the window given its work last.
    fn controller() -> Option<ICoreWebView2Controller> {
        controller_of(window()?)
    }

    /// Whether the spare page is a fresh AI Mode, ready for a question.
    fn is_ready() -> bool {
        BROWSER.with(|browser| {
            browser.borrow().pages.iter().any(|page| {
                page.spare && page.controller.is_some() && page.ready.is_some_and(|since| since.elapsed() < FRESH)
            })
        })
    }

    /// The window's title.
    fn title_of(window: HWND) -> String {
        let mut buffer = [0u16; 256];
        let len = unsafe { GetWindowTextW(window, buffer.as_mut_ptr(), buffer.len() as i32) };
        String::from_utf16_lossy(&buffer[..len.max(0) as usize])
    }

    /// What a script returns on the page of the window given its work last, as JSON.
    fn evaluate(script: &str) -> String {
        evaluate_on(&page(), script)
    }

    /// What a script returns on a page of the program, as JSON.
    pub(crate) fn evaluate_on(webview: &ICoreWebView2, script: &str) -> String {
        let result = Rc::new(RefCell::new(None));
        let sink = result.clone();
        let done = ExecuteScriptCompletedHandler::create(Box::new(move |_, json| {
            *sink.borrow_mut() = Some(json);
            Ok(())
        }));
        let script = wide(script);
        unsafe { webview.ExecuteScript(ComText(script.as_ptr()), &done) }.unwrap();
        pump_until(10, || result.borrow().is_some());
        result.take().unwrap_or_default()
    }

    /// The page of the window given its work last.
    fn page() -> ICoreWebView2 {
        unsafe { controller().unwrap().CoreWebView2() }.unwrap()
    }

    /// Calls the browser's developer protocol on the window's page and returns its answer.
    fn devtools(method: &str, parameters: &str) -> String {
        devtools_on(&page(), method, parameters)
    }

    fn devtools_on(webview: &ICoreWebView2, method: &str, parameters: &str) -> String {
        let result = Rc::new(RefCell::new(None));
        let sink = result.clone();
        let done = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |_, json| {
            *sink.borrow_mut() = Some(json);
            Ok(())
        }));
        let (method, parameters) = (wide(method), wide(parameters));
        unsafe { webview.CallDevToolsProtocolMethod(ComText(method.as_ptr()), ComText(parameters.as_ptr()), &done) }.unwrap();
        pump_until(10, || result.borrow().is_some());
        result.take().unwrap_or_default()
    }

    /// Saves what the window's page shows as a PNG file, through the browser's developer
    /// protocol: the page alone, nothing else of the screen.
    fn save_page(path: &str) {
        save_page_of(&page(), path);
    }

    /// Saves what a page of the program shows as a PNG file.
    pub(crate) fn save_page_of(webview: &ICoreWebView2, path: &str) {
        let json = devtools_on(webview, "Page.captureScreenshot", "{}");
        let data = json.split("\"data\":\"").nth(1).and_then(|rest| rest.split('"').next()).expect("no picture");
        std::fs::write(path, from_base64(data)).unwrap();
    }

    /// Clicks the page at a point, as the user would, through the browser's developer protocol:
    /// the page gets the mouse's events, and the user's own mouse stays where it is.
    fn click(x: f64, y: f64) {
        for kind in ["mousePressed", "mouseReleased"] {
            devtools("Input.dispatchMouseEvent", &format!(r#"{{"type":"{kind}","x":{x},"y":{y},"button":"left","clickCount":1}}"#));
        }
    }

    pub(crate) fn from_base64(text: &str) -> Vec<u8> {
        let value = |c: u8| {
            (match c {
                b'A'..=b'Z' => c - b'A',
                b'a'..=b'z' => c - b'a' + 26,
                b'0'..=b'9' => c - b'0' + 52,
                b'+' => 62,
                _ => 63,
            }) as u32
        };
        let digits: Vec<u8> = text.bytes().filter(|&c| c != b'=').collect();
        let mut bytes = Vec::with_capacity(digits.len() * 3 / 4);
        for chunk in digits.chunks(4) {
            let n = chunk.iter().enumerate().fold(0, |n, (i, &c)| n | value(c) << (18 - 6 * i));
            bytes.extend_from_slice(&n.to_be_bytes()[1..chunk.len()]);
        }
        bytes
    }

    /// A screenshot of a line of English, as PNG.
    fn english() -> Vec<u8> {
        let (width, height) = (480, 90);
        let mut buffer = gdi::Buffer::new(width, height).unwrap();
        buffer.frame().pixels.fill(255);
        let font = gdi::Font::new(32, false).unwrap();
        let text: Vec<u16> = "Good morning, my friend".encode_utf16().collect();
        gdi::draw_text(buffer.dc(), &font, &text, 16, 24, [0, 0, 0, 255]);
        let pixels = buffer.frame().pixels.to_vec();
        Image { width, height, pixels }.to_png().unwrap()
    }

    fn button(name: &str, mode: Mode, prompt: &str) -> QueryButton {
        QueryButton { name: name.into(), mode, prompt: prompt.into(), key: None, icon: None }
    }

    #[test]
    fn base64_is_read_with_its_padding() {
        assert_eq!(from_base64("TWFu"), b"Man");
        assert_eq!(from_base64("TWE="), b"Ma");
        assert_eq!(from_base64("TQ=="), b"M");
        assert_eq!(from_base64("iVBORw0KGgo="), b"\x89PNG\r\n\x1a\n");
    }

    /// Opens Google in the window and waits for its title. It shows a window and goes online,
    /// so it runs only when asked, with the profile in a folder of one's choice:
    /// `LOCALAPPDATA=<folder> cargo test -p ainitshot window -- --ignored`.
    #[test]
    #[ignore]
    fn the_window_opens_google() {
        unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };
        open("https://www.google.com/").unwrap();
        let window = window().unwrap();
        let title = || {
            let mut buffer = [0u16; 256];
            let len = unsafe { GetWindowTextW(window, buffer.as_mut_ptr(), buffer.len() as i32) };
            String::from_utf16_lossy(&buffer[..len.max(0) as usize])
        };
        pump_until(30, || controller().is_some() && title().contains("Google"));
        let (started, title) = (controller().is_some(), title());
        close();
        assert!(started, "WebView2 didn't start");
        assert!(title.contains("Google"), "the page didn't open: «{title}»");
    }

    /// Opens AI Mode in the window with a profile where nobody is signed in to Google and checks
    /// that the page invites to sign in. It shows the window and goes online, so it runs only when
    /// asked: `LOCALAPPDATA=<folder> cargo test -p ainitshot invites -- --ignored`. With
    /// `SCREENSHOT_PREVIEW=<file.png>` it also saves what the page shows.
    #[test]
    #[ignore]
    fn the_window_invites_to_sign_in_while_nobody_is() {
        unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };
        open(query::AI_MODE).unwrap();
        let found = pump_until(40, || controller().is_some() && evaluate("!!document.getElementById('ainitshot-sign-in')") == "true");
        if let (true, Some(file)) = (found, std::env::var_os("SCREENSHOT_PREVIEW")) {
            pump_until(1, || false);
            save_page(&file.to_string_lossy());
        }
        close();
        assert!(found, "the page doesn't invite to sign in");
    }

    /// Asks AI Mode in the hidden window to translate a picture with English on it and waits for
    /// the answer in the card. It shows the card, goes online and asks Google, so it runs only
    /// when asked: `LOCALAPPDATA=<folder> cargo test -p ainitshot card -- --ignored`.
    #[test]
    #[ignore]
    fn the_card_shows_the_translation_from_the_hidden_window() {
        unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };
        let prompt = "Переведи текст на картинке на русский язык. Ответь только переводом.";
        let button = button("Перевести", Mode::Answer, prompt);
        let query::Way::Window { url, script } = query::send(&button, &english()) else { panic!("not the window") };
        let selection = Rect::new(300, 200, 780, 290);
        card::open(selection);
        let picture = Image { width: 1, height: 1, pixels: vec![255; 4] };
        let job = Job { name: button.name, question: button.prompt.clone(), script, picture, mode: button.mode, listener: None };
        ask(&url, job, Some(selection)).unwrap();
        let start = Instant::now();
        pump_until(90, || idle() && card::text().is_some_and(|t| t != "Google думает…"));
        let shown = card::text();
        let seconds = start.elapsed().as_secs_f32();
        card::close();
        close();
        let shown = shown.expect("the card was closed");
        assert!(shown.to_lowercase().contains("утро"), "the card shows «{shown}» after {seconds:.1} s");
        println!("the card showed «{shown}» after {seconds:.1} s");
    }

    /// Asks AI Mode about a picture with English on it, as "Ask Google" does, and checks that
    /// the window comes out next to the selection with the conversation alone: no header, logo
    /// or history rail of Google's, the field for the next question, and the page no wider than
    /// the window. It shows the window, goes online and asks Google, so it runs only when asked:
    /// `LOCALAPPDATA=<folder> cargo test -p ainitshot conversation -- --ignored`. With
    /// `SCREENSHOT_PREVIEW=<file.png>` it also saves what the page shows.
    #[test]
    #[ignore]
    fn the_conversation_comes_out_next_to_the_selection() {
        unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };
        let button = button("Спросить Google", Mode::Chat, "Что на картинке? Объясни.");
        let query::Way::Window { url, script } = query::send(&button, &english()) else { panic!("not the window") };
        let selection = Rect::new(300, 200, 780, 290);
        let picture = Image { width: 1, height: 1, pixels: vec![255; 4] };
        let job = Job { name: button.name, question: button.prompt.clone(), script, picture, mode: button.mode, listener: None };
        ask(&url, job, Some(selection)).unwrap();
        let sent = pump_until(90, idle);
        let complete = r#"!!document.querySelector('[data-xid="aim-mars-turn-root"][data-asrc] [data-container-id="main-col"]')"#;
        let answered = sent && pump_until(90, || evaluate(complete) == "true");
        let problems = evaluate(
            r##"(() => {
              const shown = (e) => !!e && e.getClientRects().length > 0;
              const problems = [];
              if (shown(document.querySelector("#gb"))) problems.push("Google's header shows");
              if (shown(document.querySelector("body > header"))) problems.push("Google's logo shows");
              if (shown(document.querySelector('[data-xid="m3fCN"]'))) problems.push("the history rail shows");
              if (!shown(document.querySelector('[data-xid="aim-mars-input-plate"]'))) problems.push("no question field");
              if (document.documentElement.scrollWidth > innerWidth) problems.push("the page is wider than the window");
              return problems;
            })()"##,
        );
        if let Ok(path) = std::env::var("SCREENSHOT_PREVIEW") {
            save_page(&path);
        }
        let mut frame = RECT::default();
        unsafe { GetWindowRect(window().unwrap(), &mut frame) };
        close();
        assert!(sent, "the question didn't go");
        assert!(answered, "AI Mode didn't answer");
        assert_eq!(problems, "[]");
        let (left, top) = (frame.left, frame.top);
        assert!(left > selection.right && top == selection.top, "the window isn't to the right of the selection: {left}, {top}");
    }

    /// What AI Mode's page holds for a question, as JSON: its size, its send buttons and question
    /// fields, each with the plate it stands in, whether it shows and how it is, the picture
    /// pasted and the turns of the conversation.
    const QUESTION_PLACES: &str = r#"(() => {
      const plate = (e) => e.closest('[data-xid$="input-plate"]')?.getAttribute("data-xid") ?? "";
      return JSON.stringify({
        size: [innerWidth, innerHeight],
        hidden: document.hidden,
        address: location.href.slice(0, 70),
        buttons: [...document.querySelectorAll('button[data-xid="input-plate-send-button"]')].map((b) => [plate(b), !!b.offsetParent, b.disabled]),
        fields: [...document.querySelectorAll("textarea")].map((t) => [plate(t), !!t.offsetParent, t.value]),
        plates: [...document.querySelectorAll('[data-xid$="input-plate"]')].map((p) => [p.getAttribute("data-xid"), !!p.offsetParent]),
        pictures: document.querySelectorAll('[aria-label="screenshot.png"]').length,
        focused: document.activeElement?.tagName,
        says: [...document.querySelectorAll('[role="alert"], [aria-live]')].map((e) => e.innerText.trim()).filter((t) => t && t.length < 120),
        turns: document.querySelectorAll('[data-xid="aim-mars-turn-root"]').length,
      });
    })()"#;

    /// Asks AI Mode as G does in the program: the question takes the spare page, a fresh AI Mode
    /// loaded hidden in the size of a window that no query has brought, and the window comes out
    /// narrow next to the selection. It goes online and asks Google, out of sight, so it runs only
    /// when asked: `LOCALAPPDATA=<folder> cargo test -p ainitshot -- --ignored --exact --nocapture
    /// win::browser::tests::a_question_goes_on_ai_modes_spare_page`. With
    /// `SCREENSHOT_PREVIEW=<file.png>` it also saves what the page shows, and with
    /// `SPARE_AGE=<seconds>` the spare page waits as long for the question.
    #[test]
    #[ignore]
    fn a_question_goes_on_ai_modes_spare_page() {
        unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };
        prepare();
        assert!(pump_until(60, is_ready), "the spare page didn't come");
        let spare = spare_window().unwrap();
        let places = || evaluate_on(&webview_of(spare).unwrap(), QUESTION_PLACES);
        // As long as the spare page has waited for the question, in seconds.
        let waited = std::env::var("SPARE_AGE").ok().and_then(|age| age.parse().ok()).unwrap_or(3);
        pump_until(waited, || false);
        eprintln!("spare page: {}", places());
        let button = button("Спросить Google", Mode::Chat, "Что на картинке? Объясни.");
        let query::Way::Window { url, script } = query::send(&button, &english()) else { panic!("not the window") };
        let selection = Rect::new(300, 200, 780, 290);
        let picture = Image { width: 1, height: 1, pixels: vec![255; 4] };
        let job = Job { name: button.name, question: button.prompt.clone(), script, picture, mode: button.mode, listener: None };
        ask(&url, job, Some(selection)).unwrap();
        // What the page holds as the script goes, whenever that changes.
        let mut last = String::new();
        let mut sent = false;
        for _ in 0..60 {
            let now = places();
            if now != last {
                eprintln!("{now}");
                last = now;
            }
            sent = idle();
            if sent {
                break;
            }
            pump_until(1, || false);
        }
        eprintln!("the question {}: {}", if sent { "went" } else { "stayed" }, places());
        eprintln!("the program was told: {:?}", FAILED.with(|failed| failed.borrow().clone()));
        let turned = pump_until(10, || evaluate("document.querySelectorAll('[data-xid=\"aim-mars-turn-root\"]').length") != "0");
        eprintln!("after it: {}", places());
        if let Ok(path) = std::env::var("SCREENSHOT_PREVIEW") {
            save_page(&path);
        }
        close();
        assert!(sent, "the question didn't go");
        assert!(turned, "the page took no question");
    }

    /// Searches Google by Google's own logo, as "Search by image in Google" does, and checks
    /// that the window comes out next to the selection on the tab of exact matches with pages
    /// in it, without the large copy of the picture and no wider than the window, and that a
    /// click on a page found goes to the user's own browser. It shows the window, goes online and asks Google, so it runs only when asked:
    /// `LOCALAPPDATA=<folder> cargo test -p ainitshot exact -- --ignored`. With
    /// `SCREENSHOT_PREVIEW=<file.png>` it also saves what the page shows.
    #[test]
    #[ignore]
    fn the_exact_matches_come_out_next_to_the_selection() {
        unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };
        // A picture that is on many pages: Google's logo, taken from Google itself.
        open("https://www.google.com/").unwrap();
        pump_until(30, || controller().is_some() && evaluate("document.readyState") == "\"complete\"");
        let logo = evaluate(
            r#"(() => {
              const request = new XMLHttpRequest();
              request.open("GET", "/images/branding/googlelogo/2x/googlelogo_color_272x92dp.png", false);
              request.overrideMimeType("text/plain; charset=x-user-defined");
              request.send();
              return btoa([...request.responseText].map((c) => String.fromCharCode(c.charCodeAt(0) & 255)).join(""));
            })()"#,
        );
        let png = from_base64(logo.trim_matches('"'));
        assert!(png.starts_with(b"\x89PNG"), "no logo: {logo:.80}");
        let button = button("Поиск по картинке в Google", Mode::Images, "");
        let query::Way::Window { url, script } = query::send(&button, &png) else { panic!("not the window") };
        let selection = Rect::new(300, 200, 780, 290);
        let picture = Image { width: 1, height: 1, pixels: vec![255; 4] };
        let start = Instant::now();
        let job = Job { question: button.name.clone(), name: button.name, script, picture, mode: button.mode, listener: None };
        ask(&url, job, Some(selection)).unwrap();
        let found = pump_until(60, idle);
        let seconds = start.elapsed().as_secs_f32();
        let address = evaluate("location.href");
        let pages = evaluate(r#"document.querySelectorAll('a[href*="/goto?url="]').length"#);
        let problems = evaluate(
            r##"(() => {
              const shown = (e) => !!e && e.getClientRects().length > 0;
              const problems = [];
              if (shown(document.querySelector('#searchform [jsname="ZZZihe"]'))) problems.push("the large copy of the picture shows");
              if (document.documentElement.scrollWidth > innerWidth) problems.push("the page is wider than the window");
              if (document.querySelector("#sfcnt").getBoundingClientRect().height > 40) problems.push("the room for the copy stays");
              return problems;
            })()"##,
        );
        if let Ok(path) = std::env::var("SCREENSHOT_PREVIEW") {
            save_page(&path);
        }
        // A click on the first page found opens it in the user's own browser, not in the window.
        let point = evaluate(
            r#"(() => {
              const link = [...document.querySelectorAll('a[href*="/goto?url="]')].find((a) => a.getClientRects().length);
              const box = link.getBoundingClientRect();
              return box.left + box.width / 2 + "," + (box.top + box.height / 2);
            })()"#,
        );
        let (x, y) = point.trim_matches('"').split_once(',').unwrap();
        click(x.parse().unwrap(), y.parse().unwrap());
        pump_until(10, || OPENED_OUTSIDE.with(|opened| !opened.borrow().is_empty()));
        let opened = OPENED_OUTSIDE.with(|opened| opened.borrow().clone());
        let staying = evaluate("location.href");
        let mut frame = RECT::default();
        unsafe { GetWindowRect(window().unwrap(), &mut frame) };
        close();
        assert!(found, "the results didn't come");
        assert!(opened.len() == 1 && opened[0].contains("/goto?url="), "opened outside: {opened:?}");
        assert!(staying.contains("udm=48"), "the window left the results: {staying}");
        assert!(address.contains("udm=48"), "not the exact matches: {address}");
        assert!(pages.parse::<u32>().unwrap_or(0) > 0, "no pages with the picture");
        assert_eq!(problems, "[]");
        let (left, top) = (frame.left, frame.top);
        assert!(left > selection.right && top == selection.top, "the window isn't to the right of the selection: {left}, {top}");
        println!("{pages} links to pages with the picture after {seconds:.1} s");
    }

    /// Asks AI Mode a question aloud through its microphone in the window and checks that the
    /// speech recognition that the program gives the page hears it and that AI Mode sends it. The
    /// browser hears a recording instead of the microphone, made for one with the speech synthesis
    /// of Windows, and `SCREENSHOT_SPEECH` holds its words, so it runs only when asked:
    /// `SCREENSHOT_BROWSER_ARGUMENTS="--use-fake-ui-for-media-stream
    /// --use-fake-device-for-media-stream --use-file-for-fake-audio-capture=<speech.wav>"
    /// SCREENSHOT_SPEECH="what is in this picture" LOCALAPPDATA=<folder>
    /// cargo test -p ainitshot microphone -- --ignored`. `SCREENSHOT_SPEECH_LANGUAGE=ru` asks
    /// AI Mode in Russian; with `SCREENSHOT_SPEECH_SEND=<milliseconds>` the question goes by the
    /// send button that long after the microphone, perhaps before a word has been heard, and
    /// AI Mode still sends what the recording says.
    #[test]
    #[ignore]
    fn ai_modes_microphone_hears_the_question() {
        use webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_PERMISSION_STATE_ALLOW;
        use webview2_com::PermissionRequestedEventHandler;
        let words = std::env::var("SCREENSHOT_SPEECH").expect("the words of the recording");
        let language = std::env::var("SCREENSHOT_SPEECH_LANGUAGE").map(|l| format!("&hl={l}")).unwrap_or_default();
        unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };
        open(&format!("https://www.google.com/search?udm=50&aep=11{language}")).unwrap();
        pump_until(30, || controller().is_some());
        // The program's own profile keeps the user's answer; a throwaway one asks every time.
        let allowed = PermissionRequestedEventHandler::create(Box::new(|_, args| {
            if let Some(args) = args {
                unsafe { args.SetState(COREWEBVIEW2_PERMISSION_STATE_ALLOW)? };
            }
            Ok(())
        }));
        let webview = unsafe { controller().unwrap().CoreWebView2() }.unwrap();
        let mut token = 0;
        unsafe { webview.add_PermissionRequested(&allowed, &mut token) }.unwrap();
        let microphone = r#"document.querySelector('button[data-xid="h4gG8"]')"#;
        let ready = pump_until(30, || {
            evaluate("document.readyState") == "\"complete\""
                && evaluate(&format!("!!{microphone} && {microphone}.getClientRects().length > 0")) == "true"
        });
        // Google's script binds the button a moment after the page has loaded.
        pump_until(3, || false);
        let point = evaluate(&format!(
            "(() => {{ const box = {microphone}.getBoundingClientRect(); return box.left + box.width / 2 + ',' + (box.top + box.height / 2); }})()"
        ));
        if let Some((x, y)) = point.trim_matches('"').split_once(',') {
            click(x.parse().unwrap_or(0.0), y.parse().unwrap_or(0.0));
        }
        let send_after = std::env::var("SCREENSHOT_SPEECH_SEND").ok().and_then(|ms| ms.parse::<u64>().ok());
        if let Some(ms) = send_after {
            let start = Instant::now();
            pump_until(10, || start.elapsed() >= Duration::from_millis(ms));
            let send = r#"document.querySelector('button[data-xid="input-plate-voice-send-button"]')"#;
            let point = evaluate(&format!(
                "(() => {{ const box = {send}.getBoundingClientRect(); return box.left + box.width / 2 + ',' + (box.top + box.height / 2); }})()"
            ));
            if let Some((x, y)) = point.trim_matches('"').split_once(',') {
                click(x.parse().unwrap_or(0.0), y.parse().unwrap_or(0.0));
            }
        }
        let address = || evaluate("decodeURIComponent(location.href.replace(/\\+/g, ' '))");
        let sent = pump_until(30, || address().contains("&q="));
        let address = address();
        if let Ok(path) = std::env::var("SCREENSHOT_PREVIEW") {
            save_page(&path);
        }
        close();
        assert!(ready, "no microphone in AI Mode");
        assert!(sent, "AI Mode didn't send the question");
        assert!(address.to_lowercase().contains(&words.to_lowercase()), "AI Mode heard another question: {address}");
    }

    /// Asks AI Mode a question aloud as a held "Ask Google" does, on the spare page: the
    /// microphone goes on soon in the hidden window, which says so once, the words come as they
    /// are heard, and once the button is let go the question goes with the screenshot and the
    /// window comes out next to the selection. The browser hears a recording instead of the microphone,
    /// as in `ai_modes_microphone_hears_the_question`:
    /// `SCREENSHOT_BROWSER_ARGUMENTS=... SCREENSHOT_SPEECH="what is in this picture"
    /// LOCALAPPDATA=<folder> cargo test -p ainitshot aloud -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn a_question_asked_aloud_goes_with_the_screenshot() {
        let words = std::env::var("SCREENSHOT_SPEECH").expect("the words of the recording").to_lowercase();
        unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };
        let listener = listener();
        prepare();
        let ready = pump_until(30, is_ready);
        let query::Way::Window { url, script } = query::listen(&english()) else { panic!("not the window") };
        let selection = Rect::new(300, 200, 780, 290);
        let picture = Image { width: 1, height: 1, pixels: vec![255; 4] };
        let (name, question) = ("Спросить Google".to_string(), "Спросить Google".to_string());
        let job = Job { name, question, script, picture, mode: Mode::Chat, listener: Some(listener) };
        let start = Instant::now();
        ask(&url, job, Some(selection)).unwrap();
        let on = pump_until(15, || heard(|h| *h == Heard::Listening));
        let on_after = start.elapsed().as_secs_f32();
        let hidden = unsafe { IsWindowVisible(window().unwrap()) } == 0;
        let all = pump_until(20, || heard(|h| matches!(h, Heard::Words(w) if w.to_lowercase().contains(&words))));
        stop_listening();
        let sent = pump_until(30, || heard(|h| *h == Heard::Sent));
        let shown = unsafe { IsWindowVisible(window().unwrap()) } != 0;
        let address = evaluate("decodeURIComponent(location.href.replace(/\\+/g, ' '))").to_lowercase();
        let mut frame = RECT::default();
        unsafe { GetWindowRect(window().unwrap(), &mut frame) };
        let log = all_heard();
        let told_on = HEARD.with(|all| all.borrow().iter().filter(|heard| **heard == Heard::Listening).count());
        close();
        assert!(ready, "no fresh AI Mode was made ready");
        assert!(on, "the microphone didn't go on: {log}");
        assert_eq!(told_on, 1, "the microphone was said to go on {told_on} times: {log}");
        assert!(hidden, "the window showed while it listened");
        assert!(all, "the words weren't heard: {log}");
        assert!(sent && shown, "the question didn't go: {log}");
        assert!(address.contains(&words), "AI Mode was asked another question: {address}");
        let (left, top) = (frame.left, frame.top);
        assert!(left > selection.right && top == selection.top, "the window isn't to the right of the selection: {left}, {top}");
        println!("the microphone went on {on_after:.1} s after the question was asked");
    }

    /// The microphone never stays on in the background: a question asked aloud and given up, as
    /// Esc or the frozen screen closing do, has its hidden window go at once, page and all; a
    /// recognition on a page that is hidden ends; and one that hears nothing ends after eight
    /// seconds by itself. The browser hears silence instead of the microphone:
    /// `SCREENSHOT_BROWSER_ARGUMENTS="--use-fake-ui-for-media-stream
    /// --use-fake-device-for-media-stream --use-file-for-fake-audio-capture=<silence.wav>"
    /// LOCALAPPDATA=<folder> cargo test -p ainitshot background -- --ignored`.
    #[test]
    #[ignore]
    fn the_microphone_never_stays_on_in_the_background() {
        unsafe { CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32) };
        let listener = listener();
        prepare();
        pump_until(30, is_ready);
        // A question asked aloud, given up.
        let query::Way::Window { url, script } = query::listen(&english()) else { panic!("not the window") };
        let picture = Image { width: 1, height: 1, pixels: vec![255; 4] };
        let (name, question) = ("Спросить Google".to_string(), "Спросить Google".to_string());
        let job = Job { name, question, script, picture, mode: Mode::Chat, listener: Some(listener) };
        ask(&url, job, Some(Rect::new(300, 200, 780, 290))).unwrap();
        let asked = window().unwrap();
        let on = pump_until(15, || heard(|h| *h == Heard::Listening)) && evaluate("window.__screenshotListening") == "1";
        abort_listening();
        let given_up = pump_until(5, || unsafe { IsWindow(asked) } == 0);
        // A recognition on a page that is hidden, the next spare page's.
        pump_until(30, is_ready);
        let spare = webview_of(spare_window().unwrap()).unwrap();
        let listening = || evaluate_on(&spare, "window.__screenshotListening");
        let start = r#"(() => { const r = new window.webkitSpeechRecognition(); r.continuous = true; r.start(); window.__test = r; return 1; })()"#;
        evaluate_on(&spare, start);
        let on_again = pump_until(15, || listening() == "1");
        let controller = controller_of(spare_window().unwrap()).unwrap();
        unsafe { controller.SetIsVisible(false) }.unwrap();
        let hidden_off = pump_until(5, || listening() == "0");
        unsafe { controller.SetIsVisible(true) }.unwrap();
        // A recognition that hears nothing.
        evaluate_on(&spare, start);
        let on_once_more = pump_until(15, || listening() == "1");
        let since = Instant::now();
        let silent_off = pump_until(15, || listening() == "0");
        let silent_after = since.elapsed().as_secs_f32();
        close();
        assert!(on, "the question asked aloud didn't start listening: {}", all_heard());
        assert!(given_up, "the window of the question given up stays");
        assert!(on_again && hidden_off, "the hidden page still listens");
        assert!(on_once_more && silent_off, "the silent recognition still listens");
        assert!((7.0..10.0).contains(&silent_after), "the silent recognition ended after {silent_after:.1} s");
    }
}
