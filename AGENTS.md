# AGENTS.md

## Project

Lector is a native GTK4 PDF viewer for Omarchy. It uses Poppler for rendering,
Cairo for drawing, and Rust 2021 edition.

## Source layout

- `src/main.rs` — application ID and open/activate handling.
- `src/window/` — GTK window: construction, canvas, navigation, search, annotations UI, dialogs.
- `src/window/state.rs` — document, search, and sidebar state inside the session RefCell.
- `src/view.rs` — page geometry, rendering, selection, search highlighting, and thumbnails.
- `src/doc.rs` — Poppler document access, metadata, outline extraction, and PDF fixtures for tests.
- `src/session.rs` — persistent reading state and concurrent session-file merging.
- `src/persist.rs` — XDG config paths and atomic private file writes.
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

GTK integration tests are `#[ignore]`d. They present windows, so `init_test_gtk`
lets them run only on Broadway (`gtk4-broadwayd :7`, then
`GDK_BACKEND=broadway BROADWAY_DISPLAY=:7`); `LECTOR_TEST_ON_DESKTOP=1` overrides
that. Run each one in its own process (loop in README), for example:

```bash
cargo test --offline --release actual_window_navigation_presentation_search_and_reload -- --ignored --exact --test-threads=1
```

Add a regression test for every bug fix. Prefer headless tests for layout, session,
Cairo, and Poppler behavior. Start GTK tests with `init_test_gtk()` and ensure they
do not write the user's real session file.
