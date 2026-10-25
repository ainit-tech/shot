"""Takes the site's clip into the program for its window "How to use": the clip's script and the
list of the program's own words it shows, into src/win/clip/, and the clip's words in the languages
the site has into the section "clip" of the program's catalogs. The site builds the clip (`npm run
build:clip` in shot-site) and keeps it in shot-site/embed; each repository writes only its own files.

The clip's words in the program's other languages are translated in the catalogs themselves: the
script says which catalogs lack a word, as the test of the catalogs does. English is the clip's own.

From the repository's root, with the site's repository beside it:
python -I tools/clip.py [<the site's repository>]"""
import hashlib
import json
import pathlib
import shutil
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
LOCALES = ROOT / "src" / "locales"
OUT = ROOT / "src" / "win" / "clip"
# The site's languages other than English, whose words come from it.
SITE = ("zh", "es", "ar", "fr", "ru")
PARTS = ("ui", "scenes", "picture", "keys")


def flat(words, prefix=""):
    """The words as flat keys, as the catalogs keep them: "scenes.freeze.title", an array's items
    numbered, "picture.explorer.places.0"."""
    if isinstance(words, dict):
        for key, value in words.items():
            yield from flat(value, f"{prefix}{key}.")
    elif isinstance(words, list):
        for i, value in enumerate(words):
            yield from flat(value, f"{prefix}{i}.")
    else:
        yield prefix[:-1], words


def words_of(path):
    words = json.loads(path.read_text(encoding="utf-8"))
    return dict(flat({part: words[part] for part in PARTS if words.get(part)}))


def write_catalog(path, update):
    """Changes a catalog and writes it back as the catalogs are written."""
    catalog = json.loads(path.read_text(encoding="utf-8"))
    update(catalog)
    path.write_text(json.dumps(catalog, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n")


def main():
    site = pathlib.Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT.parent / "shot-site"
    embed = site / "embed"
    OUT.mkdir(parents=True, exist_ok=True)
    for name in ("ainitshot-clip.js", "program-terms.json"):
        shutil.copyfile(embed / name, OUT / name)
    script = (OUT / "ainitshot-clip.js").read_bytes()
    print(f"src/win/clip/ainitshot-clip.js: {len(script)} bytes, SHA-256 {hashlib.sha256(script).hexdigest()}")

    english = words_of(embed / "words.en.json")
    for code in SITE:
        words = words_of(embed / f"words.{code}.json")
        write_catalog(LOCALES / f"{code}.json", lambda catalog, words=words: catalog.__setitem__("clip", words))
    # The keys that every language must have; the names of keys are a language's own, if any.
    needed = [key for key in english if not key.startswith("keys.")]
    lacking = {}
    for path in sorted(LOCALES.glob("*.json")):
        clip = json.loads(path.read_text(encoding="utf-8")).get("clip", {})
        missing = [key for key in needed if not clip.get(key)]
        if missing:
            lacking[path.stem] = len(missing)
    print(f"the site's words in {', '.join(SITE)}; {len(needed)} words in all")
    if lacking:
        print("catalogs that lack words of the clip:", ", ".join(f"{code} ({n})" for code, n in lacking.items()))


if __name__ == "__main__":
    main()
