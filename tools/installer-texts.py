"""Writes installer/languages.iss for the installer of ainitShot (installer/ainitShot.iss): its
languages, which are the program's, each with Inno Setup's own messages in it from
installer/languages laid over the English ones, and the installer's own texts, from the section
"installer" of the program's catalogs in src/locales. It stops with an error when a language of the
program has no translation of Inno Setup's messages or lacks one of the installer's texts, so that
no installer is built without them.

ci/build.sh runs it before compiling the installer; by hand, from the repository's root:
python -I tools/installer-texts.py"""
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
LOCALES = ROOT / "src" / "locales"
OUT = ROOT / "installer" / "languages.iss"

# The installer's texts in English, which the catalogs translate.
ENGLISH = {
    "installTitle": "Install ainitShot {version}",
    "installedTitle": "ainitShot is installed",
    "autostart": "Start with Windows",
    "run": "Run ainitShot",
    "done": "Done",
    "removeData": "Remove the settings of ainitShot and its sign-in to Google too?",
}

# Inno Setup's translation of its messages into each language of the program, laid over Default.isl
# so that a message an older translation lacks is in English. Amharic, Swahili, Filipino and Burmese
# have none in Inno Setup: the files of those translate what the installer of ainitShot shows.
MESSAGES = {
    "en": [],
    "zh": ["ChineseSimplified.isl"],
    "es": ["Spanish.isl"],
    "ar": ["Arabic.isl"],
    "fr": ["French.isl"],
    "ru": ["Russian.isl"],
    "hi": ["Hindi.islu"],
    "pt": ["BrazilianPortuguese.isl"],
    "ja": ["Japanese.isl"],
    "de": ["German.isl"],
    "id": ["Indonesian.isl"],
    "tr": ["Turkish.isl"],
    "vi": ["Vietnamese.isl"],
    "ko": ["Korean.isl"],
    "it": ["Italian.isl"],
    "fa": ["Farsi.isl"],
    "th": ["Thai.isl"],
    "pl": ["Polish.isl"],
    "uk": ["Ukrainian.isl"],
    "ms": ["Malaysian.isl"],
    "uz": ["Uzbek.isl"],
    "nl": ["Dutch.isl"],
    "ro": ["Romanian.isl"],
    "el": ["Greek.isl"],
    "hu": ["Hungarian.isl"],
    "cs": ["Czech.isl"],
    "sv": ["Swedish.isl"],
    "sr": ["SerbianCyrillic.isl"],
    "bg": ["Bulgarian.isl"],
    "da": ["Danish.isl"],
    "fi": ["Finnish.isl"],
    "sk": ["Slovak.isl"],
    "nb": ["Norwegian.isl"],
    "hr": ["Croatian.isl"],
    "lt": ["Lithuanian.isl"],
    "sl": ["Slovenian.isl"],
    "lv": ["Latvian.isl"],
    "et": ["Estonian.isl"],
    "he": ["Hebrew.isl"],
    "kk": ["Kazakh.islu"],
    "az": ["Azerbaijan.isl"],
    "ka": ["Georgian.isl"],
    "hy": ["Armenian.isl"],
    "sq": ["Albanian.isl"],
    "bn": ["Bengali.islu"],
    "ur": ["Urdu.isl", "Urdu-RightToLeft.isl"],
    "am": ["Amharic.isl"],
    "sw": ["Swahili.isl"],
    "fil": ["Filipino.isl"],
    "my": ["Burmese.isl"],
}


def fail(message):
    print(f"tools/installer-texts.py: {message}", file=sys.stderr)
    sys.exit(1)


def inno(text):
    """A text as a value of the script: the version comes from the preprocessor."""
    return text.replace("{version}", "{#Version}")


def main():
    codes = ["en"] + sorted(path.stem for path in LOCALES.glob("*.json"))
    if set(codes) != set(MESSAGES):
        fail(f"the languages of the program and of the installer differ: {sorted(set(codes) ^ set(MESSAGES))}")
    texts = {"en": ENGLISH}
    for code in codes[1:]:
        catalog = json.loads((LOCALES / f"{code}.json").read_text(encoding="utf-8")).get("installer", {})
        missing = [key for key in ENGLISH if not catalog.get(key, "").strip()]
        if missing:
            fail(f"{code}.json lacks the installer's texts {missing}")
        texts[code] = catalog
    for code, files in MESSAGES.items():
        for name in files:
            if not (ROOT / "installer" / "languages" / name).is_file():
                fail(f"installer/languages/{name} for {code} is missing")

    lines = ["; Written by tools/installer-texts.py from src/locales; not to be edited by hand.", "", "[Languages]"]
    for code, files in MESSAGES.items():
        messages = ",".join(["compiler:Default.isl"] + [f"languages\\{name}" for name in files])
        lines.append(f'Name: "{code}"; MessagesFile: "{messages}"')
    # The first page has a title and the box of starting with Windows; the last one only says that
    # the program is installed, with the box that runs it.
    lines += ["", "[Messages]"]
    for code in MESSAGES:
        t = texts[code]
        lines += [
            f"{code}.WelcomeLabel1={inno(t['installTitle'])}",
            f"{code}.WelcomeLabel2=",
            f"{code}.FinishedHeadingLabel={inno(t['installedTitle'])}",
            f"{code}.FinishedLabelNoIcons=",
            f"{code}.FinishedLabel=",
            f"{code}.ClickFinish=",
            f"{code}.ButtonFinish={t['done']}",
        ]
    lines += ["", "[CustomMessages]"]
    for code in MESSAGES:
        t = texts[code]
        lines += [f"{code}.Autostart={t['autostart']}", f"{code}.Run={t['run']}", f"{code}.RemoveData={t['removeData']}"]
    OUT.write_text("\n".join(lines) + "\n", encoding="utf-8-sig", newline="\r\n")
    print(f"installer/languages.iss: {len(MESSAGES)} languages")


if __name__ == "__main__":
    main()
