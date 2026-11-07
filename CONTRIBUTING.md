# Contributing to Lector

Small, focused fixes and reproducible bug reports are welcome. Discuss large
features in an issue before implementing them. Contributions are under the
same GPL-3.0-or-later license as Lector; no separate CLA is required.

## Build and test

Follow the [README](README.md#install) for build dependencies, including
`gsfonts` for Helvetica-compatible text rendering. On Arch, add
the headless test tools:

```bash
sudo pacman -S --needed python xorg-server-xvfb dbus ttf-dejavu desktop-file-utils
cargo fetch --locked
make check CARGO_FLAGS='--offline --locked'
make test-gtk CARGO_FLAGS='--offline --locked'
make build CARGO_FLAGS='--offline --locked'
./scripts/test-install.sh
```

The normal suite covers Poppler rendering, forms, annotations, geometry,
atomic persistence, concurrent writers, and privacy permissions. GTK tests
are ignored by Cargo's default runner: GTK binds to its first thread, so
`scripts/test-gtk.py` starts each test in a separate process. It creates an
isolated Xvfb display and private temporary config/cache/data/runtime paths.
Do not run all ignored tests in a single Cargo invocation.

To run a subset, use e.g.
`./scripts/test-gtk.py --filter zoom_tests --offline --locked`.

The runner opts into the existing GTK display guard only inside its virtual
display. It does not connect to the desktop. Broadway can still be used for
individual tests, but without a browser client some frame/allocation tests
do not receive the events they need. The ignored `visual_pdf_palette_samples`
test is a manual artifact generator and is not part of the GTK suite.

`make check` and `make build` use `Cargo.lock`. Keep it committed and use
`--locked` for reproducible dependency resolution. Rust 1.92 is the declared
dependency floor; CI checks current Arch Rust, not every older compiler.
CI runs in an Arch container because Lector requires recent native libraries.
Use `CARGO_BUILD_JOBS=2` if compilation consumes too much memory.

For dependency and history checks (also run weekly in CI):

```bash
cargo audit
gitleaks git --redact --log-opts=--all
```

These tools are available as Arch packages `cargo-audit` and `gitleaks`.
Cargo audit checks Rust crates; GTK and Poppler security updates come from
the distribution's package manager.

## Code map

| Area | Files |
| --- | --- |
| App startup / open handling | `src/main.rs` |
| Window, navigation, search, edit tools | `src/window/` |
| Page geometry, rendering, selection, raster cache | `src/view.rs` |
| Poppler, PDF metadata, forms and synthetic fixtures | `src/doc.rs` |
| Sidecar highlights and notes | `src/annotations.rs` |
| Reading history and concurrent session merging | `src/session.rs` |
| XDG paths and atomic private writes | `src/persist.rs` |
| Omarchy theme and GTK CSS | `src/theme.rs` |

Keep GTK and Poppler objects on the main thread. Preserve rendering and
selection at 0°, 90°, 180°, and 270°. Keep session writes atomic and private,
and retain stale-writer merging. Add a regression test for bug fixes; generate
small synthetic PDFs instead of committing personal or copyrighted documents.
Prefer the existing architecture over introducing new abstraction layers.

## Pull requests

Explain the user-visible problem and the resulting behavior. Include the
checks run, and mention any checks that could not run. For visual changes,
include before/after images using a synthetic or public document. Keep unrelated
cleanup separate. Do not include build output, credentials, personal PDFs,
reading history, or annotation stores.
