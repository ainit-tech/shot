//! Queries (decisions 4, 5 and 8): the screenshot goes to Google's AI mode or image search, and
//! the answer opens in the user's own browser or in the program's own window. None of the ways
//! in is official, so any can stop working; then the program says so and falls back to the
//! clipboard.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::settings::{Mode, QueryButton};
use crate::text::{self, tr};

/// Where the answer opens after a query.
#[derive(Debug, PartialEq)]
pub enum Way {
    /// A page that the user's browser opens and that sends the screenshot from there, so that
    /// the picture belongs to the user's own session.
    Page(String),
    /// A page of Google's in the program's own window (decision 8), where the script puts the
    /// screenshot: AI Mode with the question, or Google Images. For a button with
    /// `mode = "answer"` the window stays hidden, and the script hands the answer over to be
    /// shown in a card next to the selection.
    Window { url: String, script: String },
}

/// What the script on Google's page tells the program.
#[derive(Debug, PartialEq)]
pub enum Told {
    /// The script of a question asked aloud hears what the program tells it: stop or give up.
    Ready,
    /// The microphone is on, and the question asked aloud can be heard.
    Listening,
    /// The words of the question asked aloud, as far as they have been heard.
    Heard(String),
    /// Nothing was heard of the question asked aloud.
    Silent,
    /// The question has gone, or the results of the image search have come.
    Sent,
    /// The answer as far as it has come, and once it is complete.
    Answer { text: String, complete: bool },
    /// Google shows a page of its own instead of AI Mode, a captcha for one, so the user has to
    /// see the window. The script runs again on the page that comes after it.
    Show(String),
    /// What stopped the script, in words for the user.
    Failed(String),
}

/// Reads a message of the script.
pub fn told(message: &str) -> Told {
    match message {
        "ready" => return Told::Ready,
        "listening" => return Told::Listening,
        "silent" => return Told::Silent,
        "sent" => return Told::Sent,
        _ => {}
    }
    if let Some(words) = message.strip_prefix("heard:") {
        return Told::Heard(words.into());
    }
    if let Some(text) = message.strip_prefix("partial:") {
        return Told::Answer { text: text.into(), complete: false };
    }
    if let Some(text) = message.strip_prefix("answer:") {
        return Told::Answer { text: text.into(), complete: true };
    }
    match message.strip_prefix("show:") {
        Some(why) => Told::Show(why.into()),
        None => Told::Failed(message.into()),
    }
}

/// Google's AI Mode.
pub const AI_MODE: &str = "https://www.google.com/ai";
/// Google Images, where the image search starts, as a person would with its camera.
const IMAGES: &str = "https://images.google.com/";

/// The script that runs on every page of the program's window before the page's own. On AI
/// Mode's page it hides Google's logo, the header with the other tabs and the account, and the
/// rail with the history of conversations, so that the window shows the conversation and the
/// question field alone. The logo and the header are found by where they stand on the page, the
/// rail by Google's own name for it. In a narrow window it also gives the rail's column to the
/// conversation; the column is found by the name of Google's style for it, which may change,
/// and then only a margin comes back. On the results for an uploaded picture it hides the large
/// copy of the picture under the search box: the picture itself is on the screen next to the
/// window. On both the scroll bar is thin.
pub const TIDY_SCRIPT: &str = r##"(() => {
  const page = new URL(location.href);
  if (page.hostname !== "www.google.com" || page.pathname !== "/search") return;
  const hidden = page.searchParams.get("udm") === "50"
    ? `body > header, #cnt > div:has(#gb), [data-xid="m3fCN"] { display: none !important; }
       @media (max-width: 755.98px) {
         .YNk70c { grid-template-columns: 4px repeat(12, minmax(0, 1fr)) 4px !important; }
       }`
    : page.searchParams.has("vsrid")
      ? `#searchform [jsname="ZZZihe"] { display: none !important; }
         body:has(#searchform [jsname="ZZZihe"]) #sfcnt { padding-bottom: 0 !important; }`
      : "";
  if (!hidden) return;
  const sheet = new CSSStyleSheet();
  sheet.replaceSync(hidden + " html { scrollbar-width: thin; scrollbar-color: #80868b transparent; }");
  document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
})();
"##;

/// Invites to sign in to Google on the pages of queries in the program's window, AI Mode's and
/// the results for a picture, where the program hides Google's own header with its sign-in: while
/// nobody is signed in, a small strip at the top says so, and "Войти" opens Google's sign-in,
/// which comes back to the page after it. Google gives its pages the cookie SAPISID only for an
/// account signed in. The cross hides the strip until the program ends. The strip speaks the
/// language in force when the window is made.
pub fn sign_in_script() -> String {
    let texts = [
        (
            "@why@",
            tr("With a Google account, Google asks for a captcha less often, and AI Mode keeps the history of conversations"),
        ),
        ("@out@", tr("You're not signed in to Google")),
        ("@in@", tr("Sign in")),
        ("@hide@", tr("Hide")),
    ];
    texts.iter().fold(SIGN_IN.to_string(), |script, (mark, text)| script.replace(mark, &escape(text)))
}

const SIGN_IN: &str = r##"(() => {
  if (window.top !== window) return;
  const page = new URL(location.href);
  if (page.hostname !== "www.google.com" || page.pathname !== "/search") return;
  if (page.searchParams.get("udm") !== "50" && !page.searchParams.has("vsrid")) return;
  const show = () => {
    if (/(?:^|;\s*)(?:SAPISID|__Secure-3PAPISID)=/.test(document.cookie)) return;
    if (sessionStorage.getItem("ainitShot.signInLater") || document.getElementById("ainitshot-sign-in")) return;
    const holder = document.createElement("div");
    holder.id = "ainitshot-sign-in";
    const root = holder.attachShadow({ mode: "closed" });
    const back = encodeURIComponent(location.href);
    root.innerHTML = `<style>
      .strip { position: fixed; top: 10px; left: 50%; z-index: 2147483647; transform: translateX(-50%);
        display: flex; align-items: center; gap: 10px; padding: 4px 4px 4px 14px; white-space: nowrap;
        border: 1px solid rgba(255, 255, 255, 0.14); border-radius: 18px; background: #2b2b2b; color: #ececec;
        font: 13px/1.3 "Segoe UI Variable Text", "Segoe UI", sans-serif; box-shadow: 0 6px 20px rgba(0, 0, 0, 0.35);
        animation: in 0.2s ease-out; }
      @keyframes in { from { opacity: 0; transform: translate(-50%, -6px); } }
      a { padding: 5px 14px; border-radius: 14px; background: #2f6feb; color: #fff; text-decoration: none; }
      a:hover { background: #4380f3; }
      button { width: 26px; height: 26px; border: 0; border-radius: 13px; background: none; color: #a8a8a8;
        font: 16px/1 "Segoe UI", sans-serif; cursor: pointer; }
      button:hover { background: rgba(255, 255, 255, 0.08); color: #fff; }
    </style>
    <div class="strip" role="note" title="@why@">
      <span>@out@</span>
      <a href="https://accounts.google.com/ServiceLogin?continue=${back}">@in@</a>
      <button aria-label="@hide@" title="@hide@">×</button>
    </div>`;
    root.querySelector("button").addEventListener("click", () => {
      sessionStorage.setItem("ainitShot.signInLater", "1");
      holder.remove();
    });
    document.documentElement.append(holder);
  };
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", show, { once: true });
  else show();
})();
"##;

/// Prepares the screenshot as the button says and returns where the answer opens.
pub fn send(button: &QueryButton, png: &[u8]) -> Way {
    match button.mode {
        // Google Lens takes the picture with the mini prompt as text and redirects to its
        // results for both. It shows an uploaded picture only to the session that uploaded it,
        // so the user's browser uploads it.
        Mode::Ai => Way::Page(form_page(&upload_url(), &[("q", &button.prompt)], "encoded_image", png)),
        Mode::Chat => Way::Window { url: AI_MODE.into(), script: ai_mode_script(png, Some(&button.prompt), false) },
        Mode::Answer => Way::Window { url: AI_MODE.into(), script: ai_mode_script(png, Some(&button.prompt), true) },
        Mode::Images => Way::Window { url: IMAGES.into(), script: images_script(png) },
    }
}

/// Prepares the screenshot for a question asked aloud, as a button for a conversation is held:
/// AI Mode in the program's window, with the words heard in place of the mini prompt.
pub fn listen(png: &[u8]) -> Way {
    Way::Window { url: AI_MODE.into(), script: ai_mode_script(png, None, false) }
}

/// Where to go when the screenshot couldn't be sent: the AI with the mini prompt, or the image
/// search's own page, where the user pastes the screenshot from the clipboard.
pub fn fallback(button: &QueryButton) -> String {
    match button.mode {
        Mode::Ai => format!("https://www.google.com/search?udm=50&hl={}&q={}", text::current().google(), encode(&button.prompt)),
        // AI Mode in the browser with the question already sent: it then asks for the picture,
        // and the screenshot pasted next answers it.
        Mode::Chat | Mode::Answer => format!("{AI_MODE}?q={}", encode(&button.prompt)),
        Mode::Images => IMAGES.into(),
    }
}

/// How the user finishes a query by hand with the screenshot from the clipboard, on the page of
/// [`fallback`] or in the window: the camera of Google Images takes a pasted picture as well.
pub fn paste_hint(mode: Mode) -> &'static str {
    match mode {
        Mode::Images => tr("The screenshot is in the clipboard: click the camera icon and paste it (Ctrl+V)."),
        Mode::Ai | Mode::Chat | Mode::Answer => tr("The screenshot is in the clipboard: paste it into the question field (Ctrl+V) and press Enter."),
    }
}

/// Google's results for an uploaded picture, on the tab "All" (`udm=26`) where Google opens
/// them, moved to the tab of exact matches (`udm=48`): the pages with this very picture, whose
/// titles often say who or what is in it.
pub fn exact_matches(url: &str) -> Option<String> {
    let query = url.strip_prefix("https://www.google.com/search?")?;
    let parameters: Vec<&str> = query.split('&').collect();
    if !parameters.iter().any(|p| p.starts_with("vsrid=")) || !parameters.contains(&"udm=26") {
        return None;
    }
    let moved: Vec<&str> = parameters.into_iter().map(|p| if p == "udm=26" { "udm=48" } else { p }).collect();
    Some(format!("https://www.google.com/search?{}", moved.join("&")))
}

/// Whether an address is AI Mode's own page without a conversation, where [`AI_MODE`] leads.
pub fn is_fresh_ai_mode(url: &str) -> bool {
    url.strip_prefix("https://www.google.com/search?").is_some_and(|query| {
        let parameters: Vec<&str> = query.split('&').collect();
        parameters.contains(&"udm=50") && !parameters.iter().any(|p| p.starts_with("q="))
    })
}

/// Whether an address that the user follows leaves Google's pages: it is another site, or
/// Google's own way out to one (`/goto`, `/url`).
pub fn leaves_google(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://")) else { return false };
    let (host, path) = rest.split_at(rest.find(['/', '?', '#']).unwrap_or(rest.len()));
    let host = host.split(':').next().unwrap_or_default().to_ascii_lowercase();
    let google = host == "google.com" || host.ends_with(".google.com");
    !google || matches!(path.split(['?', '#']).next(), Some("/goto" | "/url"))
}

/// The script that the program runs on AI Mode's page in its own window, as soon as the page's
/// document is there. It does what the user would: pastes the screenshot into the question
/// field, as Ctrl+V does, types the question and sends it; with `answer`, it also waits for the
/// answer and hands its text over, as it comes and once it is complete. Every step starts when
/// the page is ready for it: the field has appeared, the picture shows in the field under the
/// pasted file's name, Google has enabled the send button, which it does once the picture is
/// uploaded, and Google has marked the answer complete. Without a question, the question is
/// asked aloud: the microphone goes on at once, while the picture uploads, and the words heard
/// go into the field once the program has said to stop listening. Blocks are found by Google's own names
/// for them, the same in every language. Time limits only decide when to give up. The script
/// tells the program how it went (see [`told`]).
fn ai_mode_script(png: &[u8], question: Option<&str>, answer: bool) -> String {
    let (listening, typing) = match question {
        Some(question) => {
            (String::new(), format!("field.focus();\n    document.execCommand(\"insertText\", false, {});", js_string(question)))
        }
        None => {
            let failed = tr("Speech recognition didn't work: ");
            (LISTENING.into(), TYPING_HEARD.replace("@failed@", &js_string(failed)))
        }
    };
    format!(
        r#"(async () => {{
  const report = (what) => window.chrome.webview.postMessage(what);
  // Resolves as soon as `find` finds what it looks for: it looks again on every change of the
  // page and once the page has loaded. After `seconds` it gives up.
  const until = (find, seconds) => new Promise((resolve) => {{
    const found = find();
    if (found) return resolve(found);
    const check = () => {{
      const found = find();
      if (found) finish(found);
    }};
    const observer = new MutationObserver(check);
    const limit = setTimeout(() => finish(null), seconds * 1000);
    const finish = (value) => {{
      observer.disconnect();
      clearTimeout(limit);
      window.removeEventListener("load", check);
      resolve(value);
    }};
    observer.observe(document, {{
      subtree: true,
      childList: true,
      characterData: true,
      attributes: true,
      attributeFilter: ["disabled", "class", "style", "hidden", "data-sfc-inited"],
    }});
    window.addEventListener("load", check);
  }});
  try {{
    // Google may show a page of its own first, a captcha for one, which the user has to see.
    if (location.hostname !== "www.google.com" || location.pathname.startsWith("/sorry")) {{
      return report("show:" + {captcha});
    }}{listening}
    const sendButton = () => document.querySelector('button[data-xid="input-plate-send-button"]');
    const field = await until(() => sendButton() && [...document.querySelectorAll("textarea")].find((t) => t.offsetParent), 60);
    if (!field) return report({no_field});
    // Pasted once: if the page's own script isn't ready yet, Google keeps the event and hands it
    // over later, so a second paste would attach the picture twice.
    const files = new DataTransfer();
    files.items.add(new File([Uint8Array.from(atob("{data}"), (c) => c.charCodeAt(0))], "screenshot.png", {{ type: "image/png" }}));
    field.focus();
    field.dispatchEvent(new ClipboardEvent("paste", {{ clipboardData: files, bubbles: true, cancelable: true }}));
    if (!(await until(() => document.querySelector('[aria-label="screenshot.png"]'), 30))) {{
      return report({not_taken});
    }}
    {typing}
    const send = await until(() => {{
      const button = sendButton();
      return button && !button.disabled ? button : null;
    }}, 120);
    if (!send) return report({not_uploaded});
    const before = new Set(document.querySelectorAll('[data-container-id="main-col"]'));
    send.click();
    if (!{answer}) return report("sent");
    // The answer grows in a block of its own, and Google marks the block once it is complete.
    // Its parts have containers of their own; next to them Google may add a warning that AI can
    // make mistakes, which is not part of the answer.
    const answerOf = (block) => {{
      const parts = [...block.children].filter((part) => part.hasAttribute("data-container-id"));
      return (parts.length ? parts.map((part) => part.innerText.trim()).filter(Boolean).join("\n") : block.innerText).trim();
    }};
    let shown = "";
    const complete = await until(() => {{
      const turns = document.querySelectorAll('[data-xid="aim-mars-turn-root"] [data-container-id="main-col"]');
      const block = [...turns].find((b) => !before.has(b));
      const text = block ? answerOf(block) : "";
      if (text && text !== shown) {{
        shown = text;
        report("partial:" + text);
      }}
      return text && block.hasAttribute("data-sfc-inited") ? text : null;
    }}, 180);
    if (complete || shown) return report("answer:" + (complete || shown));
    report({no_answer});
  }} catch (error) {{
    report({stopped} + error.message);
  }}
}})();
"#,
        data = base64(png),
        captcha = js_string(tr("Google showed a page of its own instead of AI Mode, such as a check that you're not a robot.")),
        no_field = js_string(tr("AI Mode's question field didn't appear.")),
        not_taken = js_string(tr("AI Mode didn't take the screenshot.")),
        not_uploaded = js_string(tr("The screenshot never finished uploading to AI Mode.")),
        no_answer = js_string(tr("AI Mode's answer never came.")),
        stopped = js_string(stopped()),
    )
}

/// The beginning of the message that the program's script on a page has stopped.
fn stopped() -> &'static str {
    tr("The program's script stopped: ")
}

/// The part of AI Mode's script that listens to a question asked aloud: it starts at once and
/// tells the program when the microphone is on and what has been heard so far. It stops when
/// the program says so, and gives up when the program does.
const LISTENING: &str = r#"
    const heard = new Promise((resolve) => {
      const recognition = new window.webkitSpeechRecognition();
      let finals = "";
      let words = "";
      let failure = "";
      let aborted = false;
      recognition.lang = document.documentElement.lang || navigator.language;
      recognition.continuous = true;
      recognition.interimResults = true;
      recognition.onaudiostart = () => report("listening");
      recognition.onresult = (event) => {
        let interim = "";
        for (let i = event.resultIndex; i < event.results.length; i++) {
          const text = event.results[i][0].transcript;
          if (event.results[i].isFinal) finals = `${finals} ${text}`.trim();
          else interim = `${interim} ${text}`.trim();
        }
        words = `${finals} ${interim}`.trim();
        report("heard:" + words);
      };
      recognition.onerror = (event) => (failure = event.error);
      recognition.onend = () => resolve({ words, failure, aborted });
      window.chrome.webview.addEventListener("message", (event) => {
        if (event.data === "stop") recognition.stop();
        if (event.data === "abort") {
          aborted = true;
          recognition.abort();
        }
      });
      recognition.start();
      report("ready");
    });"#;

/// The part of AI Mode's script that types the question asked aloud, once it has been heard.
const TYPING_HEARD: &str = r#"const { words, failure, aborted } = await heard;
    if (aborted) return;
    if (!words) {
      return report(failure && failure !== "no-speech" ? @failed@ + failure + "." : "silent");
    }
    field.focus();
    document.execCommand("insertText", false, words);"#;

/// The script of the image search, which the program runs on every page that the window opens
/// for it, as soon as the page's document is there. On Google Images it sends the screenshot as
/// the camera there sends a picture: at most 1024 pixels on its longer side, as JPEG on white,
/// with the sizes of the picture and of the window, to Google Lens. Google answers with its
/// results for the picture, which the program moves to the tab of exact matches before they
/// load (see [`exact_matches`]); on them the script says that it is done (see [`told`]).
fn images_script(png: &[u8]) -> String {
    format!(
        r##"(async () => {{
  const report = (what) => window.chrome.webview.postMessage(what);
  try {{
    // Google may show a page of its own first, a captcha for one, which the user has to see.
    if (!["images.google.com", "www.google.com"].includes(location.hostname) || location.pathname.startsWith("/sorry")) {{
      return report("show:" + {captcha});
    }}
    if (location.pathname === "/search") return report("sent");
    const png = new Blob([Uint8Array.from(atob("{data}"), (c) => c.charCodeAt(0))], {{ type: "image/png" }});
    const picture = await createImageBitmap(png);
    const scale = Math.min(1, 1024 / Math.max(picture.width, picture.height));
    const width = Math.round(picture.width * scale);
    const height = Math.round(picture.height * scale);
    const canvas = Object.assign(document.createElement("canvas"), {{ width, height }});
    const context = canvas.getContext("2d");
    context.fillStyle = "#fff";
    context.fillRect(0, 0, width, height);
    context.drawImage(picture, 0, 0, width, height);
    const jpeg = await new Promise((resolve) => canvas.toBlob(resolve, "image/jpeg"));
    const language = encodeURIComponent(document.documentElement.lang);
    const form = Object.assign(document.createElement("form"), {{
      method: "post",
      enctype: "multipart/form-data",
      action: `https://lens.google.com/v3/upload?ep=gsbubb&st=${{Date.now()}}&hl=${{language}}&vpw=${{innerWidth}}&vph=${{innerHeight}}`,
    }});
    const file = Object.assign(document.createElement("input"), {{ type: "file", name: "encoded_image" }});
    const files = new DataTransfer();
    files.items.add(new File([jpeg], "screenshot.jpg", {{ type: "image/jpeg" }}));
    file.files = files.files;
    const size = Object.assign(document.createElement("input"), {{
      type: "hidden",
      name: "processed_image_dimensions",
      value: `${{width}},${{height}}`,
    }});
    form.append(file, size);
    document.body.append(form);
    form.submit();
  }} catch (error) {{
    report({stopped} + error.message);
  }}
}})();
"##,
        data = base64(png),
        captcha = js_string(tr("Google showed a page of its own instead of the image search, such as a check that you're not a robot.")),
        stopped = js_string(stopped()),
    )
}

/// Text as a JavaScript string in double quotes.
fn js_string(text: &str) -> String {
    let mut literal = String::with_capacity(text.len() + 2);
    literal.push('"');
    for c in text.chars() {
        match c {
            '"' => literal.push_str("\\\""),
            '\\' => literal.push_str("\\\\"),
            '\n' => literal.push_str("\\n"),
            '\r' => literal.push_str("\\r"),
            '\u{2028}' | '\u{2029}' => literal.push_str(&format!("\\u{:04x}", c as u32)),
            c if c < ' ' => literal.push_str(&format!("\\u{:04x}", c as u32)),
            c => literal.push(c),
        }
    }
    literal.push('"');
    literal
}

/// Where Google Lens takes a picture.
fn upload_url() -> String {
    format!("https://lens.google.com/v3/upload?hl={}&st={}", text::current().google(), millis())
}

/// A page that sends a form with the screenshot as soon as the browser opens it. The browser
/// builds the file field from the picture written into the page, then submits the form as if
/// the user had picked the file. Coming back to the page doesn't send it again.
fn form_page(action: &str, fields: &[(&str, &str)], file_field: &str, png: &[u8]) -> String {
    let inputs: String = fields
        .iter()
        .map(|(name, value)| format!("\n<input type=\"hidden\" name=\"{}\" value=\"{}\">", escape(name), escape(value)))
        .collect();
    let (action, file_field, data) = (escape(action), escape(file_field), base64(png));
    let language = text::current().google();
    let sending = escape(tr("Sending the screenshot to Google…"));
    let sent = js_string(tr("The screenshot has been sent already."));
    format!(
        r##"<!doctype html>
<html lang="{language}">
<head>
<meta charset="utf-8">
<title>screenshot</title>
<style>
html {{ background: #202124; color: #e8eaed; font: 15px "Segoe UI Variable Text", "Segoe UI", sans-serif; }}
body {{ display: grid; place-items: center; height: 100vh; margin: 0; }}
</style>
</head>
<body>
<p id="status">{sending}</p>
<form id="form" method="post" enctype="multipart/form-data" action="{action}">
<input type="file" name="{file_field}" hidden>{inputs}
</form>
<script>
if (location.hash === "#sent") {{
  document.getElementById("status").textContent = {sent};
}} else {{
  history.replaceState(null, "", "#sent");
  const bytes = Uint8Array.from(atob("{data}"), (c) => c.charCodeAt(0));
  const files = new DataTransfer();
  files.items.add(new File([bytes], "screenshot.png", {{ type: "image/png" }}));
  const form = document.getElementById("form");
  form.elements["{file_field}"].files = files.files;
  form.submit();
}}
</script>
</body>
</html>
"##
    )
}

/// Text made safe inside an HTML attribute or text.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let bytes = [chunk[0], chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (bytes[0] as u32) << 16 | (bytes[1] as u32) << 8 | bytes[2] as u32;
        let digit = |shift: u32| ALPHABET[(n >> shift) as usize & 63] as char;
        encoded.push(digit(18));
        encoded.push(digit(12));
        encoded.push(if chunk.len() > 1 { digit(6) } else { '=' });
        encoded.push(if chunk.len() > 2 { digit(0) } else { '=' });
    }
    encoded
}

fn millis() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis())
}

/// Text made safe for an address: everything but letters, digits and `-_.~` is written as
/// `%XX` bytes of UTF-8.
pub fn encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len() * 3);
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => encoded.push(byte as char),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    fn button(mode: Mode) -> QueryButton {
        QueryButton { name: "test".into(), mode, prompt: "Что \"это\"?".into(), key: None, icon: None }
    }

    #[test]
    fn google_gets_the_screenshot_and_the_question_from_the_users_browser() {
        let Way::Page(page) = send(&button(Mode::Ai), b"png") else { panic!("not a page") };
        assert!(page.contains("action=\"https://lens.google.com/v3/upload?hl=ru&amp;st="), "{page}");
        assert!(page.contains("<input type=\"file\" name=\"encoded_image\" hidden>"));
        assert!(page.contains("name=\"q\" value=\"Что &quot;это&quot;?\""));
        assert!(page.contains("atob(\"cG5n\")"));
    }

    #[test]
    fn the_image_search_sends_the_screenshot_from_google_images_as_its_camera_does() {
        let Way::Window { url, script } = send(&button(Mode::Images), b"png") else { panic!("not the window") };
        assert_eq!(url, "https://images.google.com/");
        assert!(script.contains("atob(\"cG5n\")"));
        assert!(script.contains("1024 / Math.max(picture.width, picture.height)"), "at most 1024 pixels");
        assert!(script.contains("canvas.toBlob(resolve, \"image/jpeg\")"));
        assert!(script.contains("https://lens.google.com/v3/upload?ep=gsbubb&st="), "{script}");
        assert!(script.contains("processed_image_dimensions"));
        assert!(!script.contains("Что"), "no question");
    }

    #[test]
    fn the_image_searchs_results_move_to_the_exact_matches() {
        let results = "https://www.google.com/search?vsrid=CAI&vsint=CAI&udm=26&lns_mode=un&source=lns.web.gsbubb";
        let exact = "https://www.google.com/search?vsrid=CAI&vsint=CAI&udm=48&lns_mode=un&source=lns.web.gsbubb";
        assert_eq!(exact_matches(results).as_deref(), Some(exact));
        assert_eq!(exact_matches(exact), None, "already there");
        assert_eq!(exact_matches("https://www.google.com/search?udm=26&q=cat"), None, "not an uploaded picture");
        assert_eq!(exact_matches("https://www.google.com/search?aep=11&udm=50&vsrid=CAI"), None, "AI Mode stays");
    }

    #[test]
    fn a_fresh_ai_mode_is_told_from_a_conversation() {
        assert!(is_fresh_ai_mode("https://www.google.com/search?udm=50&aep=11"));
        assert!(!is_fresh_ai_mode("https://www.google.com/search?aep=11&udm=50&q=%D1%87%D1%82%D0%BE"), "a conversation");
        assert!(!is_fresh_ai_mode("https://www.google.com/search?udm=48&vsrid=CAI"), "exact matches");
        assert!(!is_fresh_ai_mode("https://accounts.google.com/"));
    }

    #[test]
    fn links_out_of_google_are_told_apart() {
        assert!(leaves_google("https://en.wikipedia.org/wiki/Google"));
        assert!(leaves_google("https://www.google.com/goto?url=CAESkAEB"), "Google's way out to a site");
        assert!(leaves_google("https://www.google.com/url?q=https://example.com/"));
        assert!(leaves_google("http://example.com"));
        assert!(!leaves_google("https://www.google.com/search?udm=44&vsrid=CAI"));
        assert!(!leaves_google("https://accounts.google.com/signin"));
        assert!(!leaves_google("https://www.google.com/urls"), "only the way out itself");
        assert!(!leaves_google("about:blank"));
    }

    #[test]
    fn a_chat_puts_the_screenshot_and_the_question_into_ai_mode_in_the_programs_window() {
        let Way::Window { url, script } = send(&button(Mode::Chat), b"png") else { panic!("not the window") };
        assert_eq!(url, "https://www.google.com/ai");
        assert!(script.contains("atob(\"cG5n\")"));
        assert!(script.contains(r#"document.execCommand("insertText", false, "Что \"это\"?")"#), "{script}");
        assert!(script.contains("input-plate-send-button"));
        assert!(script.contains("if (!false) return report(\"sent\");"));
    }

    #[test]
    fn an_answer_waits_for_ai_modes_answer_and_hands_its_text_over() {
        let Way::Window { script, .. } = send(&button(Mode::Answer), b"png") else { panic!("not the window") };
        assert!(script.contains("if (!true) return report(\"sent\");"));
        assert!(script.contains("aim-mars-turn-root"));
        assert!(script.contains("data-sfc-inited"));
    }

    #[test]
    fn a_question_asked_aloud_is_heard_while_the_picture_uploads() {
        let Way::Window { url, script } = listen(b"png") else { panic!("not the window") };
        assert_eq!(url, "https://www.google.com/ai");
        assert!(script.contains("atob(\"cG5n\")"));
        let listening = script.find("new window.webkitSpeechRecognition()").expect("no listening");
        let pasting = script.find("new ClipboardEvent(\"paste\"").expect("no pasting");
        assert!(listening < pasting, "the microphone goes on before the picture is pasted");
        assert!(script.contains("document.execCommand(\"insertText\", false, words)"), "{script}");
        assert!(script.contains("if (!true) return report(\"sent\");") || script.contains("if (!false) return report(\"sent\");"));
    }

    #[test]
    fn the_scripts_messages_are_read() {
        assert_eq!(told("ready"), Told::Ready);
        assert_eq!(told("listening"), Told::Listening);
        assert_eq!(told("heard:что это"), Told::Heard("что это".into()));
        assert_eq!(told("silent"), Told::Silent);
        assert_eq!(told("sent"), Told::Sent);
        assert_eq!(told("partial:При"), Told::Answer { text: "При".into(), complete: false });
        assert_eq!(told("answer:Привет"), Told::Answer { text: "Привет".into(), complete: true });
        assert_eq!(told("show:капча"), Told::Show("капча".into()));
        assert_eq!(told("Поле не появилось."), Told::Failed("Поле не появилось.".into()));
    }

    #[test]
    fn without_the_window_a_chat_opens_ai_mode_in_the_browser_with_the_question_sent() {
        let question = "%D0%A7%D1%82%D0%BE%20%22%D1%8D%D1%82%D0%BE%22%3F";
        assert_eq!(fallback(&button(Mode::Chat)), format!("https://www.google.com/ai?q={question}"));
    }

    #[test]
    fn text_becomes_a_javascript_string() {
        let expected = ["\"a \\\"b\\\" \\\\ c\\nd", "\\", "u2028\""].concat();
        assert_eq!(js_string("a \"b\" \\ c\nd\u{2028}"), expected);
    }

    #[test]
    fn the_fallback_asks_the_ai_mode_the_mini_prompt() {
        let url = fallback(&button(Mode::Ai));
        assert_eq!(url, "https://www.google.com/search?udm=50&hl=ru&q=%D0%A7%D1%82%D0%BE%20%22%D1%8D%D1%82%D0%BE%22%3F");
    }

    #[test]
    fn base64_pads_the_last_bytes() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
    }
}
