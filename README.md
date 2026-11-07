# Lector

Read, annotate, and fill PDFs in a native GTK4 app, built for
[Omarchy](https://omarchy.org) and Arch Linux. Written in Rust and licensed
under [GPL-3.0-or-later](LICENSE).

Lector is under active development. Omarchy is the primary desktop; other
Linux desktops can use the built-in dark theme, but are not the main test
target. There are no bundled Windows/macOS builds.

Same layout as GNOME Document Viewer (Evince): header bar, left side pane
with thumbnails/outline, find bar, zoom drop-down, hamburger menu. Pages
render with Poppler (the same engine Evince uses). Chrome follows the
active Omarchy theme — palette, font, corner radius, transparency — and
reloads when you switch themes. PDF colors can remain normal, be inverted,
or follow the active Omarchy background/foreground palette while preserving
colored diagrams.

## Layout

```
┌──────────────────────────────────────────────────────────────────────────┐
│ [side] [open] [‹] page / N [›]  title  [−] [zoom▾] [+] [find] [☰]       │
├────────────┬─────────────────────────────────────────────────────────────┤
│            │  Find: [        ]   2 of 14   [↑] [↓] [x]                  │
│ outline    ├─────────────────────────────────────────────────────────────┤
│            │                                                             │
│ [Outline]  │              PDF pages                                      │
│ [Thumbs]   │                                                             │
└────────────┴─────────────────────────────────────────────────────────────┘
```

## Features

- Continuous, single, and dual page (optional separate cover so spreads start after page 1)
- Fit width / page / height, explicit zoom, pinch-to-zoom, Ctrl+scroll, two-finger pan
- Thumbnails, an expandable outline, and a Marks list in the side pane (F9)
- Find (Ctrl+F), live results that scroll to the match, match-case and whole-word options, text selection and copy
- Persistent yellow highlights (one mark per selection, including multi-line) and editable text notes; listed in the Marks pane
- Reading History dialog with the latest 20 documents, page positions, and missing-file status
- Clickable internal and external links with Back/Forward navigation
- Normal, inverted, or Omarchy-following PDF colors
- Rotate, fullscreen, presentation (F5)
- Fill existing AcroForms, add embedded answer text, and draw freehand ink (signatures and marks) on nonfillable pages
- Print and save a copy
- Encrypted PDFs
- PDF page labels such as “iv” or “A-3” in the page box
- Remembers page, zoom, and rotation per file in a private, concurrency-safe session store
- Offers Reload when the open file changes on disk
- Live Omarchy theme follow

## Fill out a PDF

- **Existing interactive fields:** choose **☰ → Fill Form…**. Edit text (including
  multiline), checkboxes, radio groups, and choice lists. Read-only fields stay
  disabled. Changes apply immediately; close the dialog when finished.
- **Flat/nonfillable answer sheets:** choose **☰ → Add Text…** (Ctrl+Shift+T)
  and click where the text should go. The caret is centred on the click, so
  clicking on an answer line puts the text on it. Type on the page: arrows,
  Home/End, Delete, Ctrl+Backspace, Ctrl+V and input methods (dead keys,
  compose) work. Enter places the text; Shift+Enter adds a line. Clicking
  elsewhere also places it and starts new text. Click existing added text to
  edit it, or drag it to move it. The preview uses the same font and line
  layout that is embedded, so text does not shift when placed.
- **Handwritten marks and signatures:** choose **☰ → Draw…** (Ctrl+Shift+D) and
  drag on the page. Strokes are smoothed and all strokes made on a page in one
  Draw session (with one pen) form a single drawing, so a signature is one
  object. Strokes are embedded ink annotations, not cryptographic digital
  signatures and not Lector sidecar notes.
- While drawing or adding text, a bar under the toolbar sets the colour
  (black, blue, red), pen width or text size, and has **Undo** (Ctrl+Z) and
  **Done** (Esc). Esc keeps what you wrote and leaves the tool. Ctrl+Z undoes
  the last stroke or text change, including edits of existing text.
- Choose **Save a Copy… (Ctrl+S)** and a **different filename**. This embeds
  form values, added text, and drawings in the PDF while retaining the underlying
  document and selectable text. The original remains unchanged. Reopen the
  saved copy to continue from it. An “Unsaved PDF changes” title and a discard
  confirmation protect edits before opening another file, reloading, or closing;
  text still being typed counts as an edit.

Limitations: XFA, digital signatures, JavaScript calculations/validation and
submit/reset buttons are not supported. Radio widgets use the PDF field name
and widget ID; consult the PDF for each option's meaning. Added text is
Helvetica. Editing free text created by another program rewrites it in
Lector's style. **☰ → Added Text & Drawings…** lists every added text and
drawing by page with **Go to** and **Remove**; this also works after reopening
a saved copy, and clears the Undo history. These are PDF-embedded annotations,
not Lector's separate sidecar notes; the Delete key still removes only targeted
sidecar marks.

## Install

Build requirements: Rust **1.92+**, GTK **4.10+**, Poppler GLib **25.10+**,
a C compiler, `pkg-config`, Git, and Make. The committed lockfile is used for
builds. Current Arch packages provide the native dependencies; distributions
with older Poppler packages need newer libraries before Lector can build.

On Omarchy / Arch Linux:

```bash
sudo pacman -Syu --needed base-devel git rust gtk4 poppler-glib gsfonts desktop-file-utils
git clone https://github.com/Hank-dev/lector.git
cd lector
make install
~/.local/bin/lector /path/to/document.pdf
```

If Rust is already managed with rustup or mise, keep that toolchain and omit
`rust` from the package command. Use a current stable compiler.

`gsfonts` supplies the Helvetica-compatible Nimbus Sans font. Keep it installed
so added-text previews and saved PDFs use matching glyph widths.

`make install` installs the binary, desktop entry, icon, and license below
`~/.local`. It does not change your default PDF application. Make sure
`~/.local/bin` is on your `PATH` for the desktop launcher to find the binary.
To choose Lector as the default, run `make default` after installation.

To build without installing, run `make build`, then
`./target/release/lector document.pdf`. To update a clean checkout, run
`git pull --ff-only && make install`. Remove installed files with
`make uninstall` (reading history and annotations are retained).

Packagers can stage a build with
`make install-files PREFIX=/usr DESTDIR="$pkgdir"` after `make build`.
The executable links to system GTK/Poppler libraries; it is not a portable,
self-contained binary. Custom install prefixes need their `bin` directory
on `PATH`. Install and uninstall do not remove legacy Omadoc installations.

The desktop launcher disables GTK DMA-BUF as a compatibility workaround for
the GTK 4.22.4 issue documented in the desktop entry. It uses normal GTK
rendering otherwise.

## Development checks

See [CONTRIBUTING.md](CONTRIBUTING.md) for dependencies, architecture, and
the full test workflow. The main checks are:

```bash
make check
make test-gtk
make build
./scripts/test-install.sh
```

The GTK runner creates a private Xvfb display, D-Bus session, and temporary
state directories for each test. It exercises windows without showing them
on your desktop or using your reading history. CI also scans Git history for
secrets and checks Rust dependencies against the RustSec advisory database.

## Shortcuts

| Key | Action |
|-----|--------|
| Ctrl+O | Open |
| Ctrl+Shift+T | Add text: click, type, Enter |
| Ctrl+Shift+D | Draw: drag to write or sign |
| Ctrl+Z | Undo last stroke or text change |
| Ctrl+Shift+F | Fill interactive form fields |
| Esc | Finish drawing or adding text |
| Ctrl+S | Save a filled copy |
| Ctrl+F | Find |
| Ctrl+H | Reading History |
| Ctrl+Shift+H | Highlight current selection |
| Ctrl+Shift+N | Add a text note to current selection |
| Delete | Delete targeted/selected annotation |
| F9 | Side pane |
| c / d / s | Continuous / Dual / Single |
| w / a | Fit width / Fit page |
| Ctrl++ − 0 | Zoom in / out / 100% |
| Alt+← / Alt+→ | Back / forward |
| n p | Next / previous page |
| ↑ / ↓ | Scroll up / down in continuous view |
| Ctrl+I | Invert |
| F5 / F11 | Presentation / Fullscreen |
| Ctrl+P | Print |

Menu: Open, Save a Copy, Print, highlight selection, add/delete annotations,
Side Pane (Outline / Thumbnails / Marks), Continuous, Dual, Single page, PDF colors (Normal, Follow Omarchy,
or Inverted), Rotate, Fullscreen,
Presentation, Reload, Reading History, Properties, Keyboard shortcuts, and About Lector.

## Local data and privacy

Lector has no accounts, analytics, or automatic document uploads. Opening an
external link hands it to your system's default application.

Reading history, document paths, and view preferences live in
`$XDG_CONFIG_HOME/lector/session.toml` (default `~/.config/lector/session.toml`).
Annotations are stored by Lector in `$XDG_CONFIG_HOME/lector/annotations.toml`
(falling back to `~/.config/lector/annotations.toml`), never beside the PDF.
Print includes the current rotation and marks; Save a Copy includes form values,
embedded Add Text answers, and ink drawings, but not Lector sidecar highlights/notes or view rotation. Both annotation and reading-state files are atomically replaced and
private to the user (`0600` on Unix). Clearing Reading History does not remove
annotations.

These files are private by file permissions, **not encrypted**. Back them up
if you want to keep highlights and notes. Avoid attaching them to bug reports.

## Contributing and support

Bug reports and focused pull requests are welcome. Start with
[CONTRIBUTING.md](CONTRIBUTING.md), use the
[issue templates](https://github.com/Hank-dev/lector/issues/new/choose), and
see [SECURITY.md](SECURITY.md) for reporting vulnerabilities. Changes are
tracked in [CHANGELOG.md](CHANGELOG.md).

Lector uses GTK4, Poppler, and Cairo through their Rust bindings. Dependency
licenses remain with their respective projects. Lector's source is distributed
under the [GNU GPL, version 3 or later](LICENSE).
