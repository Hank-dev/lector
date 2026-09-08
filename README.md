# Lector

Native PDF viewer for [Omarchy](https://omarchy.org), written in Rust.

Same layout as GNOME Document Viewer (Evince): header bar, left side pane
with thumbnails/outline, find bar, zoom drop-down, hamburger menu. Pages
render with Poppler (the same engine Evince uses). Chrome follows the
active Omarchy theme — palette, font, corner radius, transparency — and
reloads when you switch themes.

## Layout

```
┌──────────────────────────────────────────────────────────────────────────┐
│ [side] [open] [‹] page / N [›]  title  [−] [zoom▾] [+] [find] [☰]       │
├────────────┬─────────────────────────────────────────────────────────────┤
│            │  Find: [        ]   2 of 14   [↑] [↓] [x]                  │
│ thumbnails ├─────────────────────────────────────────────────────────────┤
│            │                                                             │
│ [Thumbs]   │              PDF pages                                      │
│ [Outline]  │                                                             │
└────────────┴─────────────────────────────────────────────────────────────┘
```

## Features

- Continuous, single, and dual page
- Fit width / page / height, explicit zoom, Ctrl+scroll
- Thumbnails and outline in the side pane (F9)
- Find (Ctrl+F), text selection and copy
- Clickable internal and external links
- Inverted colors, rotate, fullscreen, presentation (F5)
- Print and save a copy
- Encrypted PDFs
- Remembers page and zoom per file
- Live Omarchy theme follow

## Install

Needs `gtk4`, `poppler-glib`, and a Rust toolchain (all present on Omarchy).

```bash
cd ~/Projects/lector
make install
# optional: take over PDF files
make default
```

Installs `~/.local/bin/lector`, `org.omarchy.Lector.desktop`, and a matching icon.

## Development checks

Run the display-independent regression suite and strict checks:

```bash
cargo fmt --check
cargo test --offline --release
cargo clippy --offline --release --all-targets -- -D warnings
cargo build --offline --release
```

The GTK integration test is ignored by default because it requires a display.
Run it separately from the headless tests so GTK stays on a single test thread:

```bash
cargo test --offline --release actual_window_navigation_presentation_search_and_reload -- --ignored --test-threads=1
```

It creates an unpresented window and temporary PDFs, exercises the real navigation,
presentation, search replacement, and reload paths, and never saves the user session.
Session persistence tests use isolated temporary paths and cover concurrent writers,
stale snapshots, reading-order retention, preference merging, and private file modes.

## Shortcuts

| Key | Action |
|-----|--------|
| Ctrl+O | Open |
| Ctrl+F | Find |
| F9 | Side pane |
| c / d / s | Continuous / Dual / Single |
| w / a | Fit width / Fit page |
| Ctrl++ − 0 | Zoom in / out / 100% |
| n p | Next / previous page |
| Ctrl+I | Invert |
| F5 / F11 | Presentation / Fullscreen |
| Ctrl+P | Print |

Menu: Open, Save a Copy, Print, Side Pane, Continuous, Dual, Single page,
Inverted Colors, Rotate, Fullscreen, Presentation, Reload, Properties,
Keyboard shortcuts, and About Lector.
