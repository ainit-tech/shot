//! The settings: one text file with a comment for every setting (decision 7), which the
//! settings window writes and the user may edit by hand as well. The program writes the file
//! with the defaults on its first start and rereads it when it changes.

use std::fmt::Write;

use serde::{Deserialize, Serialize};

use crate::annotation::Tool;
use crate::color::{self, Rgba};
use crate::text::{self, Choice, Language, tr, tr_in};

/// Whether the text of a settings file chooses the language, "auto" included.
pub fn chooses_language(text: &str) -> bool {
    toml::from_str::<toml::Table>(text).is_ok_and(|table| table.contains_key("language"))
}

/// The settings file with the defaults, in the language in force.
#[cfg(test)]
pub fn default_file() -> String {
    Settings::default().to_file()
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub hotkey: String,
    pub language: Choice,
    pub autostart: bool,
    pub autoupdate: bool,
    /// Whether masking has OpenAI Privacy Filter: the program downloads it, or removes it.
    pub privacy_filter: bool,
    pub colors: Vec<String>,
    pub width: f32,
    pub save: Save,
    pub panels: Panels,
    pub buttons: Vec<QueryButton>,
}

/// Where the two panels stand around the selection: each on a side of it, pressed to one end of
/// that side. Two panels pressed to the same end of one side stand in a row, `first` first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Panels {
    /// The main panel: the tools, the colour, undo and closing.
    pub tools: Place,
    /// The panel of the actions: the query buttons, masking, copying and saving.
    pub queries: Place,
    pub first: Panel,
}

/// A side of the selection and the end of it that a panel is pressed to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Place {
    pub side: Side,
    pub align: Align,
}

/// A side of the selection: above and below it a panel is a row, beside it a column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Bottom,
    Top,
    Left,
    Right,
}

/// The end of its side a panel is pressed to: left or right above and below the selection, top
/// or bottom beside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Panel {
    #[default]
    Tools,
    Queries,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Save {
    pub ask: bool,
    /// Whether the screenshot saved goes into the clipboard too.
    pub copy: bool,
    pub folder: String,
    pub name: String,
}

/// A query button (decision 4): where the screenshot goes and what is asked about it.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QueryButton {
    pub name: String,
    #[serde(default)]
    pub mode: Mode,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub icon: Option<Icon>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Google's AI answers the mini prompt in words, on top of its results.
    #[default]
    Ai,
    /// A conversation with Google's AI: AI Mode itself, in the program's own window
    /// (decision 8), where the program puts the screenshot and the question. The window comes
    /// out next to the selection with the conversation alone.
    Chat,
    /// The same with the window hidden: only the answer shows, in a card next to the selection.
    Answer,
    /// The image search, which finds where the picture comes from and where else it is: in the
    /// program's own window next to the selection, on the tab of exact matches at once.
    Images,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Icon {
    Google,
    Translate,
    Images,
    Question,
}

/// The defaults in the language in force.
impl Default for Settings {
    fn default() -> Self {
        Settings::defaults(text::current())
    }
}

impl Default for Save {
    fn default() -> Self {
        Save::defaults(text::current())
    }
}

impl Save {
    fn defaults(language: Language) -> Save {
        Save { ask: false, copy: true, folder: String::new(), name: tr_in(language, "Screenshot {date} {time}").into() }
    }
}

/// As in Lightshot: the tools under the selection at its right end, the actions to the right of
/// it at its bottom, the two meeting at the corner.
impl Default for Panels {
    fn default() -> Self {
        Panels {
            tools: Place { side: Side::Bottom, align: Align::Right },
            queries: Place { side: Side::Right, align: Align::Bottom },
            first: Panel::Tools,
        }
    }
}

impl Settings {
    /// The settings of a fresh file, with the standard query buttons and the file's name in a
    /// language: the translation is into that language too.
    pub fn defaults(language: Language) -> Settings {
        let button = |name: &str, mode, prompt: &str, key: &str, icon| QueryButton {
            name: name.into(),
            mode,
            prompt: prompt.into(),
            key: Some(key.into()),
            icon: Some(icon),
        };
        let ask = tr_in(language, "Ask Google");
        let ask_prompt = tr_in(language, "Who or what is in the picture? Explain.");
        let translate = tr_in(language, "Translate");
        let translate_prompt =
            tr_in(language, "Translate the text in the picture into English. Answer with the translation only.");
        let images = tr_in(language, "Search by image in Google");
        Settings {
            hotkey: "Alt+~".into(),
            language: Choice::Auto,
            autostart: false,
            autoupdate: true,
            privacy_filter: true,
            colors: ["#E53935", "#FB8C00", "#FDD835", "#43A047", "#1E88E5", "#8E24AA", "#212121", "#FFFFFF"]
                .map(String::from)
                .into(),
            width: 4.0,
            save: Save::defaults(language),
            panels: Panels::default(),
            buttons: vec![
                button(ask, Mode::Chat, ask_prompt, "G", Icon::Google),
                button(translate, Mode::Answer, translate_prompt, "T", Icon::Translate),
                button(images, Mode::Images, "", "I", Icon::Images),
            ],
        }
    }

    /// The settings moved to another language: what is still as the defaults of the language
    /// they were in becomes the defaults of the new one. A query button's name and mini prompt
    /// each follow while they are a standard button's, whatever else was changed, so that the
    /// standard "Translate" translates into the new language; the file's name follows too.
    pub fn speaking(mut self, from: Language, to: Language) -> Settings {
        let (was, now) = (Settings::defaults(from), Settings::defaults(to));
        for button in &mut self.buttons {
            if let Some(i) = was.buttons.iter().position(|standard| standard.name == button.name) {
                button.name.clone_from(&now.buttons[i].name);
            }
            let prompt = |standard: &QueryButton| !standard.prompt.is_empty() && standard.prompt == button.prompt;
            if let Some(i) = was.buttons.iter().position(prompt) {
                button.prompt.clone_from(&now.buttons[i].prompt);
            }
        }
        if self.save.name == was.save.name {
            self.save.name = now.save.name;
        }
        self
    }

    /// The settings after an installation in a language other than that of Windows (decision 11):
    /// settings that speak that of Windows, "auto", speak the installation's from now on, with
    /// the standard texts in it; a language chosen stays.
    pub fn installed_in(self, language: Language, windows: Language) -> Settings {
        if self.language != Choice::Auto || language == windows {
            return self;
        }
        Settings { language: Choice::Own(language), ..self }.speaking(windows, language)
    }

    /// Reads the settings from the text of the file. The error says what is wrong and where.
    pub fn parse(text: &str) -> Result<Settings, String> {
        let settings: Settings = toml::from_str(text).map_err(|e| e.to_string())?;
        settings.check()?;
        Ok(settings)
    }

    /// The settings as the file holds them: every setting with its comment in the language in
    /// force, as the program writes the file the first time and as the settings window writes it.
    pub fn to_file(&self) -> String {
        self.to_file_in(text::current())
    }

    fn to_file_in(&self, language: Language) -> String {
        let mut file = String::new();
        let colors: Vec<String> = self.colors.iter().map(|c| toml_string(c)).collect();
        let width = if self.width.fract() == 0.0 { format!("{:.0}", self.width) } else { self.width.to_string() };
        let notes = NOTES.translated(language);
        let _ = write!(
            file,
            "{head}

{hotkey_note}
hotkey = {hotkey}

{language_note}
language = \"{language}\"

{autostart_note}
autostart = {autostart}

{autoupdate_note}
autoupdate = {autoupdate}

{privacy_filter_note}
privacy_filter = {privacy_filter}

{colors_note}
colors = [{colors}]

{width_note}
width = {width}

[save]
{ask_note}
ask = {ask}
{copy_note}
copy = {copy}
{folder_note}
folder = {folder}
{name_note}
name = {name}

{panels_note}
[panels]
{tools_note}
tools = {tools}
{queries_note}
queries = {queries}
{first_note}
first = \"{first}\"

{buttons_note}
",
            head = notes.head,
            hotkey_note = notes.hotkey,
            hotkey = toml_string(&self.hotkey),
            language_note = notes.language,
            language = self.language.name(),
            autostart_note = notes.autostart,
            autostart = self.autostart,
            autoupdate_note = notes.autoupdate,
            autoupdate = self.autoupdate,
            privacy_filter_note = notes.privacy_filter,
            privacy_filter = self.privacy_filter,
            colors_note = notes.colors,
            colors = colors.join(", "),
            width_note = notes.width,
            ask_note = notes.ask,
            ask = self.save.ask,
            copy_note = notes.copy,
            copy = self.save.copy,
            folder_note = notes.folder,
            folder = toml_string(&self.save.folder),
            name_note = notes.name,
            name = toml_string(&self.save.name),
            panels_note = notes.panels,
            tools_note = notes.tools,
            tools = self.panels.tools.to_file(),
            queries_note = notes.queries,
            queries = self.panels.queries.to_file(),
            first_note = notes.first,
            first = self.panels.first.name(),
            buttons_note = notes.buttons,
        );
        for button in &self.buttons {
            let _ = write!(file, "\n[[buttons]]\nname = {}\nmode = \"{}\"\n", toml_string(&button.name), button.mode.name());
            if !button.prompt.is_empty() {
                let _ = writeln!(file, "prompt = {}", toml_string(&button.prompt));
            }
            if let Some(key) = &button.key {
                let _ = writeln!(file, "key = {}", toml_string(key));
            }
            if let Some(icon) = button.icon {
                let _ = writeln!(file, "icon = \"{}\"", icon.name());
            }
        }
        file
    }

    /// Whether the settings can be put in force, and if not, what is wrong, with the setting's
    /// name in the file first.
    pub fn check(&self) -> Result<(), String> {
        if self.hotkey.trim().is_empty() {
            return Err(tr("hotkey: the hotkey is not set.").into());
        }
        if self.colors.is_empty() {
            return Err(tr("colors: at least one colour is needed.").into());
        }
        if let Some(bad) = self.colors.iter().find(|c| color::parse(c).is_none()) {
            let text = tr("colors: “{bad}” is not a colour; it should look like \"#RRGGBB\".");
            return Err(text.replace("{bad}", bad));
        }
        if !(1.0..=50.0).contains(&self.width) {
            return Err(tr("width: the width must be from 1 to 50.").into());
        }
        for (name, place) in [("tools", self.panels.tools), ("queries", self.panels.queries)] {
            if !place.fits() {
                let side = match place.side {
                    Side::Bottom => tr("below"),
                    Side::Top => tr("above"),
                    Side::Left => tr("on the left"),
                    Side::Right => tr("on the right"),
                };
                let ends = match place.side.upright() {
                    false => tr("\"left\" or \"right\""),
                    true => tr("\"top\" or \"bottom\""),
                };
                let text = tr("panels.{name}: for a panel {side} align can only be {ends}.");
                return Err(text.replace("{name}", name).replace("{side}", side).replace("{ends}", ends));
            }
        }
        // A key picks a tool first, so a button can't have a tool's key, nor another button's.
        let tool = tr("the tool “{}”");
        let mut taken: Vec<(char, String)> = Tool::ALL.iter().map(|t| (t.key(), tool.replace("{}", t.name()))).collect();
        for button in &self.buttons {
            if button.name.trim().is_empty() {
                return Err(tr("buttons: a button has no name (name).").into());
            }
            if button.mode != Mode::Images && button.prompt.trim().is_empty() {
                let text = tr("buttons: the button “{}” has no mini prompt (prompt).");
                return Err(text.replace("{}", &button.name));
            }
            if let Some(key) = &button.key
                && (key.chars().count() != 1 || !key.chars().all(|c| c.is_ascii_alphanumeric()))
            {
                let text = tr("buttons: the key of the button “{}” must be one Latin letter or digit.");
                return Err(text.replace("{}", &button.name));
            }
            if let Some(key) = button.key() {
                if let Some((_, owner)) = taken.iter().find(|(taken, _)| *taken == key) {
                    let text = tr("buttons: the button “{name}” has the key {key}, which {owner} has already.");
                    let text = text.replace("{name}", &button.name).replace("{key}", &key.to_string());
                    return Err(text.replace("{owner}", owner));
                }
                let owner = tr("the button “{}”");
                taken.push((key, owner.replace("{}", &button.name)));
            }
        }
        Ok(())
    }

    /// The colours for drawing, in the order of the settings.
    pub fn palette(&self) -> Vec<Rgba> {
        self.colors.iter().filter_map(|c| color::parse(c)).collect()
    }
}

impl Notes {
    /// The notes in a language, from its catalog.
    fn translated(&self, language: Language) -> Notes {
        Notes {
            head: tr_in(language, self.head),
            hotkey: tr_in(language, self.hotkey),
            language: tr_in(language, self.language),
            autostart: tr_in(language, self.autostart),
            autoupdate: tr_in(language, self.autoupdate),
            privacy_filter: tr_in(language, self.privacy_filter),
            colors: tr_in(language, self.colors),
            width: tr_in(language, self.width),
            ask: tr_in(language, self.ask),
            copy: tr_in(language, self.copy),
            folder: tr_in(language, self.folder),
            name: tr_in(language, self.name),
            panels: tr_in(language, self.panels),
            tools: tr_in(language, self.tools),
            queries: tr_in(language, self.queries),
            first: tr_in(language, self.first),
            buttons: tr_in(language, self.buttons),
        }
    }
}

/// The comments of the settings file in English, for the test that every catalog has them.
#[cfg(test)]
pub fn notes() -> [&'static str; 17] {
    let Notes {
        head,
        hotkey,
        language,
        autostart,
        autoupdate,
        privacy_filter,
        colors,
        width,
        ask,
        copy,
        folder,
        name,
        panels,
        tools,
        queries,
        first,
        buttons,
    } = NOTES;
    [head, hotkey, language, autostart, autoupdate, privacy_filter, colors, width, ask, copy, folder, name, panels, tools, queries, first, buttons]
}

/// The comments of the settings file, in English: one before each setting or group.
struct Notes {
    head: &'static str,
    hotkey: &'static str,
    language: &'static str,
    autostart: &'static str,
    autoupdate: &'static str,
    privacy_filter: &'static str,
    colors: &'static str,
    width: &'static str,
    ask: &'static str,
    copy: &'static str,
    folder: &'static str,
    name: &'static str,
    panels: &'static str,
    tools: &'static str,
    queries: &'static str,
    first: &'static str,
    buttons: &'static str,
}

const NOTES: Notes = Notes {
    head: "# The settings of ainitShot. The program rereads this file as soon as it is saved.
# If it has a mistake, the program says where and keeps the settings it had.
# The settings window changes them too: the menu of the icon in the tray, \"Settings\".",
    hotkey: "# The hotkey that freezes the screen; ~ is the key left of 1.
# Other examples: \"Ctrl+~\", \"PrintScreen\", \"Ctrl+PrintScreen\".",
    language: "# The program's language: \"auto\" is that of Windows, or a language's code, such as \"en\",
# \"es\", \"ru\" or \"zh\"; the settings window lists them all.",
    autostart: "# Start the program with Windows.",
    autoupdate: "# Install new versions by itself. The program looks for them as it starts and once a day
# and installs one once none of its windows is open; if false, it only shows a new version in the
# settings window and the tray menu.",
    privacy_filter: "# OpenAI Privacy Filter, a model with which masking finds names, addresses, dates and links
# too, not only keys, passwords, cards, e-mail and phones. It runs on the computer. The program
# downloads it by itself, about 950 MB, and masks without it meanwhile; false removes it.",
    colors: "# The colours for drawing, as \"#RRGGBB\"; the first one is chosen at once.",
    width: "# The width of the line in pixels at the screen's scale of 100%. While drawing, the mouse wheel changes it.",
    ask: "# Ask where to save. If false, the screenshot is saved into the folder at once.
# Ctrl+Shift+S always asks.",
    copy: "# Copy the saved screenshot into the clipboard too.",
    folder: "# The folder for screenshots. Empty means \"Pictures\\Screenshots\".",
    name: "# The file's name without its extension: {date} is the date, {time} the time.",
    panels: "# The panels next to the selection.
#   side: the side of the selection, \"bottom\", \"top\", \"left\" or \"right\";
#     below and above it a panel is a row, beside it a column;
#   align: the end of the side the panel is pressed to: \"left\" or \"right\" below and
#     above, \"top\" or \"bottom\" beside.
# Without room on its side, a panel goes to the opposite one, and without room there either,
# inside the selection at the same edge.",
    tools: "# The tools: drawing, the colour, undo and closing.",
    queries: "# The actions: the query buttons, masking, copying and saving.",
    first: "# If both panels are pressed to the same end of one side, they stand in a row; first is
# the one on the left or on top: \"tools\" or \"queries\".",
    buttons: "# The query buttons on their panel. Each sends the screenshot to Google:
#   name: the button's hint;
#   mode: where the screenshot goes:
#     \"ai\": Google's AI answers the mini prompt in words above its results for the
#       picture and the question;
#     \"chat\": AI Mode itself (google.com/ai) in the program's window: the program puts the
#       screenshot and the question there and sends them; the window comes out next to the
#       selection with the conversation and the field for the next question alone; held, the
#       button asks one's own question aloud in place of the mini prompt, while it is held;
#     \"answer\": the same with the window hidden: the answer shows in a card next to the
#       selection;
#     \"images\": the image search without a question, in the program's window next to the
#       selection: the exact matches at once, the pages with this very picture, whose titles
#       often say who or what is in it;
#   prompt: the mini prompt, the question to the AI, for \"ai\", \"chat\" and \"answer\";
#   key: the button's key while the screen is frozen;
#   icon: \"google\", \"translate\", \"images\" or \"question\".
# The AI's buttons stand first on the panel, the image search after them.
# One signs in to Google in the program's window from the settings window, \"Google account\";
# while nobody is signed in, the program offers to sign in as it starts.",
};

/// Text as a TOML string in double quotes.
fn toml_string(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for c in text.chars() {
        match c {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(quoted, "\\u{:04X}", c as u32);
            }
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

impl Place {
    /// Whether the end is one of the side's: left or right for a row, top or bottom for a column.
    pub fn fits(self) -> bool {
        match self.side {
            Side::Bottom | Side::Top => matches!(self.align, Align::Left | Align::Right),
            Side::Left | Side::Right => matches!(self.align, Align::Top | Align::Bottom),
        }
    }

    /// Whether the panel is pressed to the far end of its side, the right or the bottom one.
    pub fn at_end(self) -> bool {
        matches!(self.align, Align::Right | Align::Bottom)
    }

    /// The place as the file writes it.
    fn to_file(self) -> String {
        format!("{{ side = \"{}\", align = \"{}\" }}", self.side.name(), self.align.name())
    }
}

impl Side {
    /// The side's name in the file.
    pub fn name(self) -> &'static str {
        match self {
            Side::Bottom => "bottom",
            Side::Top => "top",
            Side::Left => "left",
            Side::Right => "right",
        }
    }

    /// The other side of the selection.
    pub fn opposite(self) -> Side {
        match self {
            Side::Bottom => Side::Top,
            Side::Top => Side::Bottom,
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        }
    }

    /// Whether a panel on this side is a column.
    pub fn upright(self) -> bool {
        matches!(self, Side::Left | Side::Right)
    }
}

impl Align {
    /// The end's name in the file.
    pub fn name(self) -> &'static str {
        match self {
            Align::Left => "left",
            Align::Right => "right",
            Align::Top => "top",
            Align::Bottom => "bottom",
        }
    }
}

impl Panel {
    /// The panel's name in the file.
    pub fn name(self) -> &'static str {
        match self {
            Panel::Tools => "tools",
            Panel::Queries => "queries",
        }
    }
}

impl Mode {
    /// The mode's name in the file.
    pub fn name(self) -> &'static str {
        match self {
            Mode::Ai => "ai",
            Mode::Chat => "chat",
            Mode::Answer => "answer",
            Mode::Images => "images",
        }
    }
}

impl Icon {
    /// The icon's name in the file.
    pub fn name(self) -> &'static str {
        match self {
            Icon::Google => "google",
            Icon::Translate => "translate",
            Icon::Images => "images",
            Icon::Question => "question",
        }
    }
}

impl QueryButton {
    /// The icon on the button; without one in the settings, it follows from where the button
    /// sends the screenshot.
    pub fn icon(&self) -> Icon {
        self.icon.unwrap_or(match self.mode {
            Mode::Images => Icon::Images,
            Mode::Ai | Mode::Chat | Mode::Answer => Icon::Google,
        })
    }

    /// The button's key as an upper-case letter or digit.
    pub fn key(&self) -> Option<char> {
        self.key.as_ref()?.chars().next().map(|c| c.to_ascii_uppercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_file_holds_the_default_settings() {
        assert_eq!(Settings::parse(&default_file()), Ok(Settings::default()));
        assert!(default_file().contains("# Горячая клавиша, которая замораживает экран"), "with its comments");
    }

    #[test]
    fn the_settings_come_back_from_the_file_they_write() {
        let mut settings = Settings { hotkey: "Ctrl+Shift+S".into(), width: 7.5, ..Settings::default() };
        settings.save.folder = r"D:\Скриншоты".into();
        settings.panels.queries = Place { side: Side::Top, align: Align::Left };
        settings.panels.first = Panel::Queries;
        settings.buttons[0].prompt = "Что \"это\"?\nОбъясни коротко.".into();
        settings.buttons[2].key = None;
        settings.buttons[2].icon = None;
        assert_eq!(Settings::parse(&settings.to_file()), Ok(settings));
    }

    #[test]
    fn a_key_belongs_to_one_button_and_never_to_a_tool() {
        let mut settings = Settings::default();
        settings.buttons[1].key = Some("a".into());
        let error = settings.check().unwrap_err();
        assert!(error.contains("Стрелка"), "{error}");
        settings.buttons[1].key = Some("g".into());
        let error = settings.check().unwrap_err();
        assert!(error.contains("Спросить Google"), "{error}");
    }

    #[test]
    fn a_file_from_before_the_languages_chooses_none() {
        assert!(!chooses_language("hotkey = \"Alt+~\""));
        assert!(chooses_language(&default_file()));
    }

    #[test]
    fn the_file_comes_back_in_every_language() {
        for language in Language::all() {
            let settings = Settings { language: Choice::Own(Language::HI), ..Settings::defaults(language) };
            assert_eq!(Settings::parse(&settings.to_file_in(language)), Ok(settings));
        }
    }

    #[test]
    fn the_standard_buttons_follow_the_language_and_ones_own_stay() {
        let english = Settings::defaults(Language::RU).speaking(Language::RU, Language::EN);
        assert_eq!(english, Settings::defaults(Language::EN));
        assert_eq!(english.buttons[0].name, "Ask Google");
        // One's own mini prompt, key, button and file's name stay; the standard names and mini
        // prompts follow, whatever else was changed: "Translate" translates into the new language.
        let mut own = Settings::defaults(Language::RU);
        own.buttons[0].prompt = "Что это за растение?".into();
        own.buttons[2].key = Some("Y".into());
        own.buttons.push(QueryButton { name: "Свой".into(), mode: Mode::Ai, prompt: "Кто это?".into(), key: None, icon: None });
        own.save.name = "Снимок {date}".into();
        let moved = own.clone().speaking(Language::RU, Language::HI);
        let hindi = Settings::defaults(Language::HI);
        assert_eq!((&moved.buttons[0].name, &moved.buttons[0].prompt), (&hindi.buttons[0].name, &own.buttons[0].prompt));
        assert_eq!(moved.buttons[1], hindi.buttons[1]);
        assert!(moved.buttons[1].prompt.contains("हिंदी"), "{}", moved.buttons[1].prompt);
        assert_eq!((&moved.buttons[2].name, &moved.buttons[2].key), (&hindi.buttons[2].name, &own.buttons[2].key));
        assert_eq!((&moved.buttons[3], &moved.save.name), (&own.buttons[3], &own.save.name));
    }

    #[test]
    fn an_installation_in_another_language_moves_settings_that_follow_windows() {
        // English Windows, the installer in Arabic: "auto" becomes Arabic, standard texts and all.
        let english = Settings::defaults(Language::EN);
        let arabic = english.clone().installed_in(Language::AR, Language::EN);
        assert_eq!(arabic, Settings { language: Choice::Own(Language::AR), ..Settings::defaults(Language::AR) });
        // The installer in the language of Windows changes nothing, nor does it a language chosen.
        assert_eq!(english.clone().installed_in(Language::EN, Language::EN), english);
        let russian = Settings { language: Choice::Own(Language::RU), ..Settings::defaults(Language::RU) };
        assert_eq!(russian.clone().installed_in(Language::AR, Language::EN), russian);
    }

    #[test]
    fn a_panel_is_pressed_to_an_end_of_its_own_side() {
        let mut settings = Settings::default();
        settings.panels.tools = Place { side: Side::Right, align: Align::Left };
        let error = settings.check().unwrap_err();
        assert!(error.starts_with("panels.tools") && error.contains("\"top\""), "{error}");
        let error = Settings::parse("[panels]\nqueries = { side = \"middle\", align = \"top\" }").unwrap_err();
        assert!(error.contains("middle"), "{error}");
    }

    #[test]
    fn missing_settings_take_their_defaults() {
        let settings = Settings::parse("hotkey = \"Ctrl+~\"").unwrap();
        assert_eq!(settings.hotkey, "Ctrl+~");
        assert_eq!(settings.colors, Settings::default().colors);
    }

    #[test]
    fn a_file_from_before_copying_on_save_copies() {
        let settings = Settings::parse("[save]\nask = true").unwrap();
        assert!(settings.save.ask && settings.save.copy);
    }

    #[test]
    fn a_mistake_is_reported_with_its_place() {
        let error = Settings::parse("width = \"thick\"").unwrap_err();
        assert!(error.contains("line 1"), "{error}");
        let error = Settings::parse("colors = [\"red\"]").unwrap_err();
        assert!(error.contains("red"), "{error}");
    }
}
