# AGENTS.md

## Project

Lector is a native GTK4 PDF viewer for Omarchy. It uses Poppler for rendering,
Cairo for drawing, and Rust 2021 edition.

## Source layout

- `src/main.rs` — application ID and open/activate handling.
- `src/window.rs` — GTK widgets, actions, navigation, search, printing, and UI state.
- `src/view.rs` — page geometry, rendering, selection, search highlighting, and thumbnails.
- `src/doc.rs` — Poppler document access, metadata, outline extraction, and PDF fixtures for tests.
- `src/session.rs` — persistent reading state and concurrent session-file merging.
- `src/theme.rs` — Omarchy theme loading and generated GTK CSS.
- `share/` — desktop entry and SVG icon.

## Rules

- Keep document rendering, selection coordinates, and search highlighting correct for rotations 0, 90, 180, and 270.
- Keep GTK work on the main thread. Do not move Poppler/GTK objects into worker threads.
- Preserve session-file privacy (`0600` on Unix), atomic writes, and stale-writer merging.
- New user-visible app identity must use `Lector` and `org.omarchy.Lector`; retain `omadoc` only for explicit legacy migration and removal paths.
- Do not install the application or change the default PDF handler unless the user asks.
- Do not add build output (`target/`) to Git.

## Tests and checks

Use offline checks because the project lockfile is intentionally sufficient for local builds:

```bash
cargo fmt --check
cargo test --offline --release
cargo clippy --offline --release --all-targets -- -D warnings
cargo build --offline --release
```

The GTK integration test requires a display and must run separately on one test thread:

```bash
cargo test --offline --release actual_window_navigation_presentation_search_and_reload -- --ignored --test-threads=1
```

Add a regression test for every bug fix. Prefer headless tests for layout, session,
Cairo, and Poppler behavior. Keep GTK window tests unpresented and ensure they do
not write the user's real session file.
