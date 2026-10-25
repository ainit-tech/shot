---
status: accepted
date: 2026-10-21
topic: queries
---

# How the screenshot reaches a search engine

The screenshot reaches Google through the user's own browser and account, and the answer opens there, in the AI mode or the image search. The screenshot is never put on a public host and leaves the computer only when a query button is pressed. The user's browser uploads the screenshot to Google Lens itself, and Google answers on top of its results for the picture and the question, with AI Mode on the next tab. The first query buttons now go through the program's own window instead, where the program puts the screenshot into Google's pages itself ([decision](008-google-in-the-programs-own-window.md)); the user's own browser stays for a button with `mode = "ai"` and for when the window can't do it.

## Question

Google has no official way for another program to hand its AI mode a picture with a question. How does the screenshot get there?

## What is known (October 2026)

- AI Mode in Google takes one picture with a question in its own interface; no documented address does it from outside.
- Google Lens takes a picture through `POST https://lens.google.com/v3/upload` (the picture in `encoded_image`, the text in `q`) and redirects to its results for the picture and the text (`udm=24`) with the id of the uploaded picture (`vsrid`); no parameter of the upload changes where it leads.
- The uploaded picture belongs to the session that uploaded it: the upload sets the `NID` cookie. AI Mode opened in the user's browser with the id of a picture that the program uploaded (`udm=50`) shows the question with an empty picture. So the user's browser has to upload the picture itself.
- The results for a picture and a question open with the answer of Google's AI on top (AI Overview) and the tab of AI Mode next to "All". This was checked in the user's browser.
- The address `https://www.google.com/ai` leads to AI Mode (`/search?udm=50&aep=11`). A question added to it (`q`) is sent at once, without a picture.
- Without a question, the upload leads to the results of the image search (`udm=26`) on `www.google.com`, the same page that the camera of Google Images opens. The entry point of the upload (`ep`) changes only the `source` parameter.
- The cameras of Google Images (`images.google.com`) and of Google's main page send the same form. They shrink the picture to 1024 pixels on its longer side and turn it into JPEG, where a transparent background becomes white, and send it to `lens.google.com/v3/upload` with `ep=gsbubb`, the size of the browser's window (`vpw`, `vph`) and the size of the picture (`processed_image_dimensions`). The program's own upload, with `ep=gisbubb` and the picture as it is, leads to the same page: an AI overview, main facts and visual matches under the tab "All", and the tabs "Exact matches" (`udm=48`) and "Matches" (`udm=44`). A `udm` added to the upload's address doesn't change the tab it leads to. Checked in a browser on October 24, 2026.
- The camera of Google Images also takes a picture pasted into its dialog (Ctrl+V) and sends it in the same way. Checked in a browser on October 24, 2026.
- In the session that uploaded the picture, the address of its results with `udm=48` in place of `udm=26` opens them on the tab "Exact matches". The pages found there are linked through Google's own address `/goto`, which leads to the page in another browser too, without the session's cookies. Checked in a browser on October 24, 2026.
- In the session that uploaded the picture, the same results address with `udm=50` opens AI Mode with the picture and the question, and AI Mode answers. In the user's browser it is the tab "AI Mode" on the results page; a browser without a Google account showed no such tab, but the address worked. Without a question, the results page has the tabs "Exact matches" and "Matches" and usually an AI overview too. Checked in a browser on October 24, 2026.
- AI Mode takes a picture pasted into its question field, but then doesn't paste text from the same clipboard. Opened with the question (`https://www.google.com/ai?q=…`), it answers by asking for the picture, and a picture pasted next is answered in the light of the question. Ctrl+V works only while the cursor is in the question field.
- AI Mode uploads a pasted picture from its own page through Google's internal API, not through the upload of Google Lens: first `lensfrontend-pa.clients6.google.com/v1/gsessionid`, then `v1/crupload`. Checked in a browser on October 24, 2026.
- Lightshot (judging by its files) and ShareX put the screenshot on a public host first and search by its address.

## Options

| Option | For | Against |
|---|---|---|
| Through the user's browser: the program hands the screenshot to the browser, and the answer opens there | The familiar AI mode in the user's own account; no keys | The ways in are unofficial and may change |
| A public image host and a search by the address, as in Lightshot and ShareX | Works with any search engine that searches by an address | Every screenshot becomes public |
| The clipboard: the program opens the AI mode, and the user pastes the screenshot and the question | Works whatever the addresses are | Extra steps every time |
| An AI service's API, such as Gemini, with the answer in the program's own window | Documented and stable | Not the search engine's AI mode; needs a key |

## Why

The point of a query is the search engine's own AI mode, in the user's own browser and account. A public host is ruled out: screenshots often show private things. The clipboard and an API stay as fallbacks in case Google closes its way in.

## How it works

- For a button with `mode = "ai"`, the program opens a page in the default browser, and the page sends the screenshot with the mini prompt to Google Lens from the user's own session: Google's AI answers the mini prompt on top of its results for the picture, and AI Mode, for further questions, is the next tab. The program deletes such pages soon after, since they hold the screenshot.
- Asking and translating go to AI Mode itself in the program's own window, where the program puts the screenshot and the question ([decision](008-google-in-the-programs-own-window.md)). For "Ask Google" (`mode = "chat"`) the window comes out next to the selection with the conversation; for "Translate" (`mode = "answer"`) it stays hidden, and the answer alone shows in a card next to the selection. When the window can't do it, AI Mode opens in the browser with the question sent, and the screenshot, pasted into the question field with Ctrl+V, is answered in the light of the question.
- The image search goes to Google Images in the program's own window, which comes out next to the selection: the program sends the screenshot there as the camera of Google Images does, and the results open on the tab "Exact matches" ([decision](008-google-in-the-programs-own-window.md)). When the window can't do it, Google Images opens in the browser, and the screenshot, pasted into the camera's dialog with Ctrl+V, is searched for.
- While the screenshot is being sent, the toolbar says so; Esc cancels the query.
- When a way in fails, the program says why, puts the screenshot into the clipboard and opens the AI mode or the image search, so that Ctrl+V finishes the query.

## Consequences

- The screenshot leaves the computer only when a query button is pressed, and only for Google.
- The ways in are unofficial. When Google changes them, the query buttons stop working until the program is fixed, and the program says so instead of failing silently.
