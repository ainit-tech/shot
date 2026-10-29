//! The program's own browser window (decision 8): Google's pages in WebView2, the Edge engine
//! built into Windows. The window keeps a profile of its own, so the user signs in to Google in
//! it once, and the program drives its pages: a query's script runs on the page it opens. For a
//! query the window comes out next to the selection with AI Mode's conversation alone; links
//! that would open a new window open in the user's own browser. The window keeps a fresh AI Mode
//! ready in the background for the next question, and only hides when closed. A question can be
//! asked aloud: the window listens while the button is held and never in the background.
//! Whether someone is signed in to Google in the window, the program learns from Google's cookies.

use std::cell::RefCell;
use std::io;
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_COLOR, COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC, COREWEBVIEW2_PERMISSION_KIND,
    COREWEBVIEW2_PERMISSION_KIND_MICROPHONE, COREWEBVIEW2_PERMISSION_STATE_ALLOW, COREWEBVIEW2_WEB_ERROR_STATUS,
    COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED, CreateCoreWebView2EnvironmentWithOptions, ICoreWebView2, ICoreWebView2_2,
    ICoreWebView2Controller, ICoreWebView2Controller2, ICoreWebView2CookieList, ICoreWebView2Environment,
    ICoreWebView2EnvironmentOptions,
    ICoreWebView2NavigationCompletedEventArgs, ICoreWebView2NavigationStartingEventArgs,
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
    CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyWindow, GetClientRect, GetCursorPos, HICON, IDC_ARROW, IsIconic,
    IsWindowVisible,
    IsZoomed, LoadCursorW, PostMessageW, RegisterClassExW, SW_HIDE, SW_RESTORE, SW_SHOW, SWP_NOACTIVATE, SWP_NOZORDER,
    SetForegroundWindow, SetWindowPos, SetWindowTextW, ShowWindow, WA_INACTIVE, WM_ACTIVATE, WM_APP, WM_CLOSE,
    WM_DPICHANGED, WM_MOVE, WM_SIZE, WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
};
use windows_sys::core::{BOOL, PCWSTR};
use windows_sys::w;

use super::{app, card, clipboard, last_error, shell, tray, wide};
use crate::card as layout;
use crate::geometry::Rect;
use crate::image::Image;
use crate::query::{self, Told};
use crate::settings::Mode;
use crate::text::tr;

/// Google's account page: the sign-in when nobody is signed in in the window, the account itself
/// when someone is.
pub const ACCOUNT: &str = "https://accounts.google.com/";

const CLASS: PCWSTR = w!("ainitShot.browser");
/// Opens the address that waits for the window, after the event in which it was decided.
const WM_NAVIGATE: u32 = WM_APP + 1;
/// The colour of the window until the page is drawn: the toolbar's.
const BACKGROUND: u32 = 0x0020_2020;
/// How long a fresh AI Mode kept ready stays fresh enough to take a question.
const FRESH: Duration = Duration::from_secs(30 * 60);
/// What the page has heard of a question asked aloud, for the window that listens: a [`Heard`] in
/// a box in `lparam`.
pub const WM_HEARD: u32 = WM_APP + 3;

/// What another window of the program does with WebView2's environment once it has started,
/// or with nothing if WebView2 couldn't start.
pub type Wanting = Box<dyn FnOnce(Option<&ICoreWebView2Environment>)>;

/// What hears whether someone is signed in to Google in the window.
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
          listeningStartHandler: () => ended || fire(this, "audiostart"),
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
    static BROWSER: RefCell<Option<Browser>> = const { RefCell::new(None) };
    /// What other windows of the program wait to do with WebView2's environment.
    static WANTING: RefCell<Vec<Wanting>> = const { RefCell::new(Vec::new()) };
    /// What waits to hear whether someone is signed in to Google, until the window's page is
    /// there to ask.
    static ASKING: RefCell<Vec<Answer>> = const { RefCell::new(Vec::new()) };
}

struct Browser {
    window: HWND,
    /// The program's icon on the window's title bar and on the taskbar, destroyed with the window.
    icons: Vec<HICON>,
    /// Comes a moment after the window, once WebView2 has started.
    controller: Option<ICoreWebView2Controller>,
    /// The address to open as soon as the window can: once WebView2 is ready, or once the
    /// navigation that it replaces has been cancelled.
    waiting: Option<String>,
    /// A query waiting for its page to load.
    job: Option<Job>,
    /// A query whose script runs on the page, until the script tells how it went.
    running: Option<Job>,
    /// The selection of the last query, on the desktop, and where its button sent the screenshot:
    /// the window comes out next to the selection, at the size of what it shows.
    beside: Option<(Rect, Mode)>,
    /// Since when the page in the window is a fresh AI Mode ready for the next question, if it is.
    ready: Option<Instant>,
    /// Whether the window comes out as the page of the query has its document.
    reveal: bool,
    /// What the program asked the script of a question asked aloud before the script could hear
    /// it: to stop listening or to give up.
    told_page: Option<&'static str>,
    /// WebView2's environment, once it has started: other windows of the program show their
    /// pages with it too.
    environment: Option<ICoreWebView2Environment>,
}

/// A query to finish on a page of the window.
pub struct Job {
    /// The name of the query button, for what the program tells the user.
    pub name: String,
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

/// Runs the query's script on a page of the window. A question for a conversation goes at once
/// to the fresh AI Mode kept ready, if there is one; any other query opens its address and runs
/// its script once the page has loaded. The window comes out next to the selection, given on the
/// desktop, only with the query's page in it, so that no page from before shows: at once with
/// the page kept ready, as the page's document is there otherwise. For an answer the window
/// stays hidden, and for a question asked aloud until the question has gone.
pub fn ask(url: &str, job: Job, selection: Option<Rect>) -> Result<(), String> {
    let (mode, aloud) = (job.mode, job.listener.is_some());
    let fresh = mode == Mode::Chat && is_ready();
    if !fresh {
        load(url)?;
    }
    // The page loads only after this returns, when the window's messages are handled.
    BROWSER.with(|browser| {
        if let Some(browser) = browser.borrow_mut().as_mut() {
            browser.job = Some(job);
            browser.beside = selection.map(|selection| (selection, mode));
            browser.ready = None;
            browser.told_page = None;
            browser.reveal = !fresh && mode != Mode::Answer && !aloud;
        }
    });
    if mode != Mode::Answer {
        if fresh && !aloud {
            show();
        } else if let Some(window) = window() {
            unsafe { ShowWindow(window, SW_HIDE) };
        }
    }
    if fresh && let Some(webview) = controller().and_then(|controller| unsafe { controller.CoreWebView2() }.ok()) {
        page_ready(&webview);
    }
    Ok(())
}

/// Keeps a fresh AI Mode ready in the hidden window for the next question, so that the question
/// goes at once and no page from before shows: loaded in the background as the program starts,
/// again once the window has been closed, and as the hotkey freezes the screen if the page kept
/// ready has grown old. A window in sight or a query under way is left as it is.
pub fn prepare() {
    let state = BROWSER.with(|browser| {
        let browser = browser.try_borrow().ok()?;
        Some(browser.as_ref().map(|b| (b.window, b.job.is_some() || b.running.is_some(), b.ready)))
    });
    match state {
        // The window is busy with WebView2, which has called in.
        None => {}
        Some(None) => drop(load(query::AI_MODE)),
        Some(Some((window, busy, ready))) => {
            let shown = unsafe { IsWindowVisible(window) } != 0;
            if !busy && !shown && ready.is_none_or(|since| since.elapsed() >= FRESH) {
                drop(load(query::AI_MODE));
            }
        }
    }
}

/// Hands WebView2's environment to another window of the program, at once if it has started,
/// once it has otherwise; the Google window starts it if it hasn't yet.
pub fn with_environment(then: Wanting) {
    let started = BROWSER.with(|browser| browser.try_borrow().ok()?.as_ref()?.environment.clone());
    if let Some(environment) = started {
        return then(Some(&environment));
    }
    // WebView2 may call back before the window's start has returned: the wish waits first.
    WANTING.with(|wanting| wanting.borrow_mut().push(then));
    if window().is_none() {
        prepare();
    }
}

/// Finds out whether someone is signed in to Google in the window: Google keeps the cookie SID
/// only for an account signed in. `then` hears it once the window's WebView2 has started, and
/// hears nothing if WebView2 can't start or can't tell.
pub fn signed_in(then: Answer) {
    match controller() {
        Some(controller) => ask_cookies(&controller, then),
        None => {
            ASKING.with(|asking| asking.borrow_mut().push(then));
            if window().is_none() {
                prepare();
            }
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

/// WebView2 couldn't start: what waited for it hears so.
fn environment_failed() {
    for then in WANTING.with(|wanting| wanting.take()) {
        then(None);
    }
}

/// Whether the page in the window is a fresh AI Mode, ready for a question.
fn is_ready() -> bool {
    BROWSER.with(|browser| {
        let browser = browser.try_borrow().ok();
        browser.as_ref().and_then(|b| b.as_ref()).is_some_and(|b| {
            b.controller.is_some() && b.job.is_none() && b.running.is_none() && b.ready.is_some_and(|since| since.elapsed() < FRESH)
        })
    })
}

/// The button of a question asked aloud has been let go: the page stops listening once the words
/// already said have come, and sends the question.
pub fn stop_listening() {
    tell_page("stop");
}

/// A question asked aloud is given up, as the frozen screen closes: the page stops listening at
/// once and sends nothing.
pub fn abort_listening() {
    BROWSER.with(|browser| {
        if let Ok(mut browser) = browser.try_borrow_mut()
            && let Some(browser) = browser.as_mut()
        {
            if browser.job.as_ref().is_some_and(|job| job.listener.is_some()) {
                browser.job = None;
            }
            if browser.running.as_ref().is_some_and(|job| job.listener.is_some()) {
                browser.running = None;
            }
        }
    });
    tell_page("abort");
}

/// Tells the script of a question asked aloud to stop listening or to give up: at once, and
/// again as soon as the script says it can hear, in case it couldn't yet.
fn tell_page(what: &'static str) {
    BROWSER.with(|browser| {
        if let Ok(mut browser) = browser.try_borrow_mut()
            && let Some(browser) = browser.as_mut()
        {
            browser.told_page = Some(what);
        }
    });
    post_page(what);
}

fn post_page(what: &str) {
    let what = wide(what);
    if let Some(webview) = controller().and_then(|controller| unsafe { controller.CoreWebView2() }.ok()) {
        let _ = unsafe { webview.PostWebMessageAsString(ComText(what.as_ptr())) };
    }
}

/// Whether a question asked aloud is under way.
fn asking_aloud() -> bool {
    BROWSER.with(|browser| {
        let browser = browser.try_borrow().ok();
        browser.as_ref().and_then(|b| b.as_ref()).is_some_and(|b| {
            b.job.as_ref().or(b.running.as_ref()).is_some_and(|job| job.listener.is_some())
        })
    })
}

/// Tells the window that listens what the page has heard of a question asked aloud.
fn tell(listener: HWND, heard: Heard) {
    let heard = Box::into_raw(Box::new(heard));
    if unsafe { PostMessageW(listener, WM_HEARD, 0, heard as isize) } == 0 {
        drop(unsafe { Box::from_raw(heard) });
    }
}

/// Opens the address in the program's window in the middle of the monitor under the mouse and
/// brings the window to the front.
pub fn open(url: &str) -> Result<(), String> {
    let window = load(url)?;
    BROWSER.with(|browser| {
        if let Some(browser) = browser.borrow_mut().as_mut() {
            browser.beside = None;
        }
    });
    move_to(window, place());
    bring_up(window);
    Ok(())
}

/// Shows the window with the page it has, the conversation behind an answer for one, next to
/// the selection of the last query.
pub fn show() {
    let Some(window) = window() else { return };
    if let Some((selection, mode)) = BROWSER.with(|browser| browser.try_borrow().ok()?.as_ref()?.beside) {
        move_to(window, beside(selection, mode));
    }
    bring_up(window);
}

/// Opens the address in the window without showing it. The first time it makes the window and
/// starts WebView2, which takes a moment; the page opens when it is ready.
fn load(url: &str) -> Result<HWND, String> {
    let window = match window() {
        Some(window) => window,
        None => start()?,
    };
    match controller() {
        Some(controller) => {
            unsafe { controller.SetIsVisible(true) }.and_then(|()| navigate(&controller, url)).map_err(|e| e.message())?;
        }
        None => BROWSER.with(|browser| {
            if let Some(browser) = browser.borrow_mut().as_mut() {
                browser.waiting = Some(url.to_string());
            }
        }),
    }
    Ok(window)
}

fn bring_up(window: HWND) {
    unsafe {
        ShowWindow(window, if IsIconic(window) != 0 { SW_RESTORE } else { SW_SHOW });
        SetForegroundWindow(window);
    }
    if let Some(controller) = controller() {
        let _ = unsafe { controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC) };
    }
}

/// Makes the window and starts WebView2 with the program's own profile.
fn start() -> Result<HWND, String> {
    let window = create_window()
        .map_err(|e| format!("{} {e}.", tr("The window wasn't made:")))?;
    let icons = tray::give_icon(window);
    BROWSER.with(|browser| {
        *browser.borrow_mut() =
            Some(Browser {
                window,
                icons,
                controller: None,
                waiting: None,
                job: None,
                running: None,
                beside: None,
                ready: None,
                reveal: false,
                told_page: None,
                environment: None,
            });
    });
    let profile = shell::browser_profile();
    let _ = std::fs::create_dir_all(&profile);
    let profile = wide(&profile.to_string_lossy());
    let created = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(|result, environment| {
        match result.and_then(|()| environment.ok_or_else(|| E_POINTER.into())) {
            Ok(environment) => create_controller(&environment),
            Err(error) => {
                failed(&error);
                environment_failed();
            }
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
        // Without WebView2 the window is of no use.
        let icons = BROWSER.with(|browser| browser.borrow_mut().take()).map(|browser| browser.icons).unwrap_or_default();
        unsafe { DestroyWindow(window) };
        for icon in icons {
            unsafe { DestroyIcon(icon) };
        }
        environment_failed();
        let failed = tr("WebView2 didn't start:");
        return Err(format!("{failed} {}", error.message()));
    }
    Ok(window)
}

fn create_controller(environment: &ICoreWebView2Environment) {
    BROWSER.with(|browser| {
        if let Ok(mut browser) = browser.try_borrow_mut()
            && let Some(browser) = browser.as_mut()
        {
            browser.environment = Some(environment.clone());
        }
    });
    for then in WANTING.with(|wanting| wanting.take()) {
        then(Some(environment));
    }
    let Some(window) = window() else { return };
    let created = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(|result, controller| {
        match result.and_then(|()| controller.ok_or_else(|| E_POINTER.into())) {
            Ok(controller) => ready(controller),
            Err(error) => failed(&error),
        }
        Ok(())
    }));
    if let Err(error) = unsafe { environment.CreateCoreWebView2Controller(ComWindow(window), &created) } {
        failed(&error);
    }
}

/// WebView2 is ready: the page fills the window, and the address that waited opens.
fn ready(controller: ICoreWebView2Controller) {
    let Some(window) = window() else { return };
    if let Err(error) = attach(&controller, window) {
        failed(&error);
        return;
    }
    let waiting = BROWSER.with(|browser| {
        let mut browser = browser.borrow_mut();
        let browser = browser.as_mut()?;
        browser.controller = Some(controller.clone());
        browser.waiting.take()
    });
    if let Some(url) = waiting
        && let Err(error) = navigate(&controller, &url)
    {
        failed(&error);
    }
    for then in ASKING.with(|asking| asking.take()) {
        ask_cookies(&controller, then);
    }
}

/// Puts the page into the window and, if the window is shown, gives it the keyboard. The page is
/// dark until it is drawn, as the window is. Every page gets the program's tidying and speech
/// scripts before its own, the window's title follows the page's, a query waiting for a page
/// runs its script as soon as the page's document is there, and the script tells the program
/// how it went. A link that would open a new window, or that leaves Google, opens in the user's
/// own browser. The microphone is the page's only for a question asked aloud; otherwise
/// WebView2 asks the user.
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
            if let Some(webview) = webview {
                let mut title = ComString::null();
                webview.DocumentTitle(&mut title)?;
                SetWindowTextW(window, wide(&take_pwstr(title)).as_ptr());
            }
            Ok(())
        }));
        webview.add_DocumentTitleChanged(&title_changed, &mut token)?;
        let document_ready = DOMContentLoadedEventHandler::create(Box::new(|webview, _| {
            if let Some(webview) = webview {
                page_ready(&webview);
            }
            Ok(())
        }));
        webview.cast::<ICoreWebView2_2>()?.add_DOMContentLoaded(&document_ready, &mut token)?;
        let navigated = NavigationCompletedEventHandler::create(Box::new(|_, args| {
            if let Some(args) = args {
                navigation_completed(&args);
            }
            Ok(())
        }));
        webview.add_NavigationCompleted(&navigated, &mut token)?;
        let starting = NavigationStartingEventHandler::create(Box::new(move |_, args| {
            if let Some(args) = args {
                navigation_starting(&args, window);
            }
            Ok(())
        }));
        webview.add_NavigationStarting(&starting, &mut token)?;
        let told = WebMessageReceivedEventHandler::create(Box::new(|_, args| {
            if let Some(args) = args {
                let mut message = ComString::null();
                if args.TryGetWebMessageAsString(&mut message).is_ok() {
                    script_told(&take_pwstr(message));
                }
            }
            Ok(())
        }));
        webview.add_WebMessageReceived(&told, &mut token)?;
        let asked = PermissionRequestedEventHandler::create(Box::new(|_, args| {
            if let Some(args) = args {
                let mut kind = COREWEBVIEW2_PERMISSION_KIND::default();
                args.PermissionKind(&mut kind)?;
                if kind == COREWEBVIEW2_PERMISSION_KIND_MICROPHONE && asking_aloud() {
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

/// A page's document is there: the query waiting for a page runs its script on it at once, and
/// the script itself waits for what it needs, and the window comes out if it waited for the
/// page. So does a query whose page was replaced before its script told how it went, as when
/// Google shows a captcha first and the page comes after it; but a question asked aloud doesn't
/// listen again by itself. A fresh AI Mode without a query is the page kept ready.
fn page_ready(webview: &ICoreWebView2) {
    let job = BROWSER.with(|browser| {
        let mut browser = browser.try_borrow_mut().ok()?;
        let browser = browser.as_mut()?;
        browser.job.take().or_else(|| browser.running.take().filter(|job| job.listener.is_none()))
    });
    let Some(job) = job else {
        let mut source = ComString::null();
        if unsafe { webview.Source(&mut source) }.is_ok() && query::is_fresh_ai_mode(&take_pwstr(source)) {
            BROWSER.with(|browser| {
                if let Ok(mut browser) = browser.try_borrow_mut()
                    && let Some(browser) = browser.as_mut()
                {
                    browser.ready = Some(Instant::now());
                }
            });
        }
        return;
    };
    let script = wide(&job.script);
    // The script answers with a message of its own; what it returns at once means nothing.
    let started = ExecuteScriptCompletedHandler::create(Box::new(|_, _| Ok(())));
    match unsafe { webview.ExecuteScript(ComText(script.as_ptr()), &started) } {
        Ok(()) => {
            let reveal = BROWSER.with(|browser| {
                let mut browser = browser.borrow_mut();
                let browser = browser.as_mut()?;
                browser.running = Some(job);
                Some(std::mem::take(&mut browser.reveal))
            });
            if reveal == Some(true) {
                show();
            }
        }
        Err(error) => job_failed(job, &error.message()),
    }
}

/// A navigation has ended: if it failed, the query that waited for its page can't go on. One
/// that another navigation replaced, as the image search's results that move to the tab of exact
/// matches, hasn't failed.
fn navigation_completed(args: &ICoreWebView2NavigationCompletedEventArgs) {
    let mut success = ComBool::default();
    if unsafe { args.IsSuccess(&mut success) }.is_ok() && success.as_bool() {
        return;
    }
    let mut status = COREWEBVIEW2_WEB_ERROR_STATUS::default();
    if unsafe { args.WebErrorStatus(&mut status) }.is_ok() && status == COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED {
        return;
    }
    let job = BROWSER.with(|browser| {
        let mut browser = browser.try_borrow_mut().ok()?;
        let browser = browser.as_mut()?;
        browser.job.take().or_else(|| browser.running.take())
    });
    if let Some(job) = job {
        job_failed(job, tr("Google's page didn't open."));
    }
}

/// A navigation is starting. The image search's results move to the tab of exact matches before
/// they load; a link that the user follows out of Google, to another site or through Google's
/// own way out to one, opens in the user's own browser instead of the window.
fn navigation_starting(args: &ICoreWebView2NavigationStartingEventArgs, window: HWND) {
    BROWSER.with(|browser| {
        if let Ok(mut browser) = browser.try_borrow_mut()
            && let Some(browser) = browser.as_mut()
        {
            browser.ready = None;
        }
    });
    let mut uri = ComString::null();
    if unsafe { args.Uri(&mut uri) }.is_err() {
        return;
    }
    let uri = take_pwstr(uri);
    let searching = BROWSER.with(|browser| {
        browser.try_borrow().ok()?.as_ref()?.running.as_ref().map(|job| job.mode == Mode::Images)
    });
    if searching == Some(true)
        && let Some(exact) = query::exact_matches(&uri)
    {
        let _ = unsafe { args.SetCancel(true) };
        BROWSER.with(|browser| {
            if let Ok(mut browser) = browser.try_borrow_mut()
                && let Some(browser) = browser.as_mut()
            {
                browser.waiting = Some(exact);
            }
        });
        // WebView2 is in the middle of its event: the new navigation starts after it.
        unsafe { PostMessageW(window, WM_NAVIGATE, 0, 0) };
        return;
    }
    let (mut user, mut redirected) = (ComBool::default(), ComBool::default());
    let followed = unsafe { args.IsUserInitiated(&mut user).and(args.IsRedirected(&mut redirected)) }.is_ok()
        && user.as_bool()
        && !redirected.as_bool();
    if followed && query::leaves_google(&uri) {
        let _ = unsafe { args.SetCancel(true) };
        open_outside(&uri);
    }
}

/// Opens the address in the user's own browser; tests only note it down.
fn open_outside(url: &str) {
    #[cfg(not(test))]
    let _ = shell::open(url);
    #[cfg(test)]
    tests::OPENED_OUTSIDE.with(|opened| opened.borrow_mut().push(url.to_string()));
}

/// The query's script has told how it went, or what it has heard of a question asked aloud.
fn script_told(message: &str) {
    let told = query::told(message);
    // What the program asked of a question asked aloud before its script could hear it goes now.
    if told == Told::Ready {
        let asked = BROWSER.with(|browser| browser.try_borrow_mut().ok()?.as_mut()?.told_page.take());
        if let Some(asked) = asked {
            post_page(asked);
        }
        return;
    }
    let running = BROWSER.with(|browser| {
        let browser = browser.try_borrow().ok()?;
        let job = browser.as_ref()?.running.as_ref()?;
        Some((job.mode, job.name.clone(), job.listener))
    });
    let Some((mode, name, listener)) = running else { return };
    match told {
        Told::Ready => {}
        Told::Listening => {
            if let Some(listener) = listener {
                tell(listener, Heard::Listening);
            }
        }
        Told::Heard(words) => {
            if let Some(listener) = listener {
                tell(listener, Heard::Words(words));
            }
        }
        Told::Silent => {
            take_running();
            if let Some(listener) = listener {
                tell(listener, Heard::Silent);
            }
        }
        Told::Sent => {
            take_running();
            if let Some(listener) = listener {
                tell(listener, Heard::Sent);
                show();
            }
        }
        Told::Answer { text, complete } => {
            if mode == Mode::Answer {
                card::answer(&text);
            }
            if complete {
                take_running();
            }
        }
        // The script runs again on the page that comes after Google's own; a question asked aloud
        // doesn't, so as not to listen by itself.
        Told::Show(why) => {
            if let Some(listener) = listener {
                take_running();
                tell_page("abort");
                tell(listener, Heard::Failed(why.clone()));
            }
            card::close();
            show();
            let open = tr("The window with it is open.");
            app::notify(&name, &format!("{why} {open}"));
        }
        Told::Failed(why) => {
            if let Some(job) = take_running() {
                job_failed(job, &why);
            }
        }
    }
}

fn take_running() -> Option<Job> {
    BROWSER.with(|browser| browser.try_borrow_mut().ok()?.as_mut()?.running.take())
}

/// The query couldn't be finished in the window: the screenshot goes to the clipboard, so that
/// Ctrl+V finishes it by hand, and the window comes out, instead of the card for an answer, so
/// that the user sees what happened. A question asked aloud stops listening, and the frozen
/// screen says what happened.
fn job_failed(job: Job, why: &str) {
    if let Some(listener) = job.listener {
        tell_page("abort");
        tell(listener, Heard::Failed(why.into()));
        return;
    }
    if job.mode == Mode::Answer {
        card::close();
    }
    show();
    let owner = window().unwrap_or(null_mut());
    let copied = clipboard::put(owner, &job.picture).is_ok();
    let how = if copied { format!(" {}", query::paste_hint(job.mode)) } else { String::new() };
    app::notify(&job.name, &format!("{why}{how}"));
}

/// Closes the window for good as the program exits. WebView2 has to be let go while the thread
/// that made it still runs: let go later, as the thread's own data is dropped, it brings the
/// program down.
pub fn close() {
    let Some(browser) = BROWSER.with(|browser| browser.borrow_mut().take()) else { return };
    if let Some(controller) = &browser.controller {
        let _ = unsafe { controller.Close() };
    }
    unsafe { DestroyWindow(browser.window) };
    for icon in browser.icons {
        unsafe { DestroyIcon(icon) };
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

fn window() -> Option<HWND> {
    BROWSER.with(|browser| browser.try_borrow().ok()?.as_ref().map(|b| b.window))
}

/// The controller of the page, taken out so that no borrow is held while WebView2 is called:
/// it may call back into the program.
fn controller() -> Option<ICoreWebView2Controller> {
    BROWSER.with(|browser| browser.try_borrow().ok()?.as_ref()?.controller.clone())
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
        // The class stays registered if an earlier window failed to start.
        RegisterClassExW(&class);
        let place = place();
        let window = CreateWindowExW(
            0,
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
/// of the selection's monitor.
fn beside(selection: Rect, mode: Mode) -> Rect {
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
    let seen = layout::place(selection, layout::window_size(size, scale, screen), scale, screen);
    // Windows draws the window's borders a pixel thin and keeps the rest of them, by which the
    // window is resized, unseen: the window reaches that much past what is seen of it.
    let (side, bottom) = (-frame.left - 1, frame.bottom - 1);
    Rect::new(seen.left - side, seen.top, seen.right + side, seen.bottom + bottom)
}

fn move_to(window: HWND, place: Rect) {
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
            if let Some(controller) = controller() {
                let _ = unsafe { controller.SetBounds(client_rect(window)) };
            }
        }
        WM_MOVE => {
            // Lists and hints of the page open in the right place.
            if let Some(controller) = controller() {
                let _ = unsafe { controller.NotifyParentWindowPositionChanged() };
            }
        }
        WM_ACTIVATE => {
            let handled = unsafe { DefWindowProcW(window, message, wparam, lparam) };
            if (wparam & 0xFFFF) as u32 != WA_INACTIVE
                && let Some(controller) = controller()
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
            let waiting = BROWSER.with(|browser| browser.try_borrow_mut().ok()?.as_mut()?.waiting.take());
            if let (Some(url), Some(controller)) = (waiting, controller())
                && let Err(error) = navigate(&controller, &url)
            {
                failed(&error);
            }
        }
        WM_CLOSE => {
            // Closing only hides the window, the sign-in stays, and the page leaves: a fresh AI
            // Mode is made ready in its place. Hidden first, the page stops listening, if it was.
            if let Some(controller) = controller() {
                let _ = unsafe { controller.SetIsVisible(false) };
            }
            unsafe { ShowWindow(window, SW_HIDE) };
            prepare();
            // The window may have been where the user signed in to Google, or out.
            app::check_account();
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
        DispatchMessageW, GetWindowRect, GetWindowTextW, MSG, PM_REMOVE, PeekMessageW,
    };

    use super::super::gdi;
    use super::*;
    use crate::settings::{Mode, QueryButton};

    thread_local! {
        /// The addresses that the window gave to the user's own browser.
        pub static OPENED_OUTSIDE: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        /// What the page told the window that listens to a question asked aloud.
        static HEARD: RefCell<Vec<Heard>> = const { RefCell::new(Vec::new()) };
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

    /// Handles the window's messages until `done` says so, at most for `seconds`; says whether
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
        BROWSER.with(|b| b.borrow().as_ref().is_some_and(|b| b.job.is_none() && b.running.is_none()))
    }

    /// What a script returns on the window's page, as JSON.
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

    /// The page of the window.
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
        ask(&url, Job { name: button.name, script, picture, mode: button.mode, listener: None }, Some(selection)).unwrap();
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
        ask(&url, Job { name: button.name, script, picture, mode: button.mode, listener: None }, Some(selection)).unwrap();
        let sent = pump_until(90, idle);
        let complete = r#"!!document.querySelector('[data-xid="aim-mars-turn-root"] [data-container-id="main-col"][data-sfc-inited]')"#;
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
        ask(&url, Job { name: button.name, script, picture, mode: button.mode, listener: None }, Some(selection)).unwrap();
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

    /// Asks AI Mode a question aloud as a held "Ask Google" does, on the fresh AI Mode kept
    /// ready: the microphone goes on soon in the hidden window, the words come as they are
    /// heard, and once the button is let go the question goes with the screenshot and the window
    /// comes out next to the selection. The browser hears a recording instead of the microphone,
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
        let job = Job { name: "Спросить Google".into(), script, picture, mode: Mode::Chat, listener: Some(listener) };
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
        close();
        assert!(ready, "no fresh AI Mode was made ready");
        assert!(on, "the microphone didn't go on: {log}");
        assert!(hidden, "the window showed while it listened");
        assert!(all, "the words weren't heard: {log}");
        assert!(sent && shown, "the question didn't go: {log}");
        assert!(address.contains(&words), "AI Mode was asked another question: {address}");
        let (left, top) = (frame.left, frame.top);
        assert!(left > selection.right && top == selection.top, "the window isn't to the right of the selection: {left}, {top}");
        println!("the microphone went on {on_after:.1} s after the question was asked");
    }

    /// The microphone never stays on in the background: a question asked aloud and given up, as
    /// Esc or the frozen screen closing do, stops listening at once; so does a recognition on a
    /// page that is hidden, as the window is when closed; and one that hears nothing ends after
    /// eight seconds by itself. The browser hears silence instead of the microphone:
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
        let listening = || evaluate("window.__screenshotListening");
        // A question asked aloud, given up.
        let query::Way::Window { url, script } = query::listen(&english()) else { panic!("not the window") };
        let picture = Image { width: 1, height: 1, pixels: vec![255; 4] };
        let job = Job { name: "Спросить Google".into(), script, picture, mode: Mode::Chat, listener: Some(listener) };
        ask(&url, job, Some(Rect::new(300, 200, 780, 290))).unwrap();
        let on = pump_until(15, || heard(|h| *h == Heard::Listening)) && listening() == "1";
        abort_listening();
        let given_up = pump_until(5, || listening() == "0");
        // A recognition on a page that is hidden.
        let start = r#"(() => { const r = new window.webkitSpeechRecognition(); r.continuous = true; r.start(); window.__test = r; return 1; })()"#;
        evaluate(start);
        let on_again = pump_until(15, || listening() == "1");
        unsafe { controller().unwrap().SetIsVisible(false) }.unwrap();
        let hidden_off = pump_until(5, || listening() == "0");
        unsafe { controller().unwrap().SetIsVisible(true) }.unwrap();
        // A recognition that hears nothing.
        evaluate(start);
        let on_once_more = pump_until(15, || listening() == "1");
        let since = Instant::now();
        let silent_off = pump_until(15, || listening() == "0");
        let silent_after = since.elapsed().as_secs_f32();
        close();
        assert!(on, "the question asked aloud didn't start listening: {}", all_heard());
        assert!(given_up, "the question given up still listens");
        assert!(on_again && hidden_off, "the hidden page still listens");
        assert!(on_once_more && silent_off, "the silent recognition still listens");
        assert!((7.0..10.0).contains(&silent_after), "the silent recognition ended after {silent_after:.1} s");
    }
}
