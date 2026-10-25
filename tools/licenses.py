"""Writes src/win/licenses.json: the open source components ainitShot is made of, each with its
version, license and address, and the texts of the licenses, for the section "About" of the
settings window. The components are the program's dependencies built for Windows, from
`cargo tree`, and the texts are the license files in their sources, which cargo has downloaded;
the models the program downloads by itself come after them. Run it from the repository's root
after the dependencies change:

    python -I tools/licenses.py

With --check it only says what it would write and which components have no license file.
"""
import json
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "src" / "win" / "licenses.json"
REGISTRY = pathlib.Path.home() / ".cargo" / "registry" / "src"
LICENSE_FILE = re.compile(r"^(licen[cs]e|copying|notice|unlicense)([-_. ].*)?$", re.IGNORECASE)

# Components whose sources come without a license file: the license of their repository.
WEBVIEW2_RS = """MIT License

Copyright (c) 2021 Bill Avery

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE."""
REPOSITORY_LICENSES = {"webview2-com": WEBVIEW2_RS, "webview2-com-macros": WEBVIEW2_RS, "webview2-com-sys": WEBVIEW2_RS}

# Built into the program by webview2-com-sys: the loader of WebView2 from Microsoft's SDK, under
# the license of the SDK.
WEBVIEW2_SDK = {
    "name": "WebView2Loader (Microsoft Edge WebView2 SDK)",
    "version": "",
    "license": "BSD-3-Clause",
    "url": "https://aka.ms/webview2",
    "text": """Copyright (C) Microsoft Corporation. All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are
met:

   * Redistributions of source code must retain the above copyright
notice, this list of conditions and the following disclaimer.
   * Redistributions in binary form must reproduce the above
copyright notice, this list of conditions and the following disclaimer
in the documentation and/or other materials provided with the
distribution.
   * The name of Microsoft Corporation, or the names of its contributors
may not be used to endorse or promote products derived from this
software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
"AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.""",
}

# The models the program downloads by itself, with what runs them: not built into it, but used.
DOWNLOADED = [
    {
        "name": "ONNX Runtime",
        "version": "1.31.0",
        "license": "MIT",
        "url": "https://github.com/microsoft/onnxruntime",
        "copyright": "Copyright (c) Microsoft Corporation",
    },
    {
        "name": "OpenAI Privacy Filter",
        "version": "7ffa9a0",
        "license": "Apache-2.0",
        "url": "https://huggingface.co/openai/privacy-filter",
        "copyright": None,
    },
]

MIT = """Permission is hereby granted, free of charge, to any person obtaining a copy of this software and
associated documentation files (the "Software"), to deal in the Software without restriction,
including without limitation the rights to use, copy, modify, merge, publish, distribute,
sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or
substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT
NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES
OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE."""


def components():
    """The dependencies of the program built for Windows: (name, version, license, address)."""
    tree = subprocess.run(
        ["cargo", "tree", "-p", "ainitshot", "-e", "normal", "--target", "x86_64-pc-windows-msvc",
         "--prefix", "none", "--format", "{p}|{l}|{r}"],
        cwd=ROOT, capture_output=True, text=True, encoding="utf-8", check=True,
    ).stdout
    seen = {}
    for line in tree.splitlines():
        package, license, url = (line.split("|") + ["", ""])[:3]
        package = package.replace(" (*)", "").replace(" (proc-macro)", "").strip()
        name, version = package.split(" ")[:2]
        if name == "ainitshot":
            continue
        seen[(name, version.lstrip("v"))] = (license.replace(" (*)", "").strip(), url.replace(" (*)", "").strip())
    return [(name, version, license, url) for (name, version), (license, url) in sorted(seen.items())]


def source(name, version):
    for index in REGISTRY.iterdir():
        folder = index / f"{name}-{version}"
        if folder.is_dir():
            return folder
    return None


def texts_of(name, version, license):
    """The license texts a component comes with: of MIT alone when it may be had under MIT, as most
    of them may; all of its license files otherwise."""
    folder = source(name, version)
    files = sorted(p for p in folder.iterdir() if p.is_file() and LICENSE_FILE.match(p.name)) if folder else []
    choices = re.split(r"\s+OR\s+|/", license)
    if len(choices) > 1 and any(c.strip() == "MIT" for c in choices):
        mit = [p for p in files if "mit" in p.name.lower()]
        if mit:
            files = mit
        elif len(files) > 1:
            files = [p for p in files if "apache" not in p.name.lower()] or files
    return [p.read_text(encoding="utf-8", errors="replace").strip() for p in files]


def main():
    check = "--check" in sys.argv
    texts, numbers, listed, bare = [], {}, [], []

    def number(text):
        if text not in numbers:
            numbers[text] = len(texts)
            texts.append(text)
        return numbers[text]

    for name, version, license, url in components():
        found = texts_of(name, version, license) or ([REPOSITORY_LICENSES[name]] if name in REPOSITORY_LICENSES else [])
        if not found:
            bare.append(f"{name} {version} ({license})")
        listed.append({"name": name, "version": version, "license": license, "url": url, "texts": [number(t) for t in found]})
    sdk = WEBVIEW2_SDK
    listed.append({"name": sdk["name"], "version": sdk["version"], "license": sdk["license"], "url": sdk["url"], "texts": [number(sdk["text"])]})
    listed.sort(key=lambda component: component["name"].lower())
    apache = next((t for t in texts if t.startswith("Apache License") or "Apache License" in t[:200]), None)
    for model in DOWNLOADED:
        if model["license"] == "MIT":
            text = f"MIT License\n\n{model['copyright']}\n\n{MIT}"
        else:
            text = apache
        listed.append({**{k: model[k] for k in ("name", "version", "license", "url")}, "downloaded": True, "texts": [number(text)] if text else []})
    data = {"components": listed, "texts": texts}
    size = len(json.dumps(data, ensure_ascii=False).encode("utf-8"))
    print(f"{len(listed)} components, {len(texts)} texts, {size // 1024} KB")
    if bare:
        print("without a license file:", *bare, sep="\n  ")
    if not check:
        OUT.write_text(json.dumps(data, ensure_ascii=False, indent=1) + "\n", encoding="utf-8", newline="\n")
        print("written", OUT.relative_to(ROOT))


main()
