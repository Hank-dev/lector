PREFIX ?= $(HOME)/.local
DESTDIR ?=
CARGO ?= cargo
CARGO_FLAGS ?= --locked
CARGO_TARGET_DIR ?= target
BIN := $(DESTDIR)$(PREFIX)/bin/lector
DESKTOP_ID := org.omarchy.Lector.desktop
DESKTOP := $(DESTDIR)$(PREFIX)/share/applications/$(DESKTOP_ID)
ICON := $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/org.omarchy.Lector.svg
LICENSE_DIR := $(DESTDIR)$(PREFIX)/share/licenses/lector

.PHONY: build check test-gtk install install-files uninstall default

build:
	$(CARGO) build $(CARGO_FLAGS) --release --target-dir "$(CARGO_TARGET_DIR)"

check:
	$(CARGO) fmt --check
	$(CARGO) test $(CARGO_FLAGS) --release --target-dir "$(CARGO_TARGET_DIR)"
	$(CARGO) clippy $(CARGO_FLAGS) --release --target-dir "$(CARGO_TARGET_DIR)" --all-targets -- -D warnings
	desktop-file-validate share/$(DESKTOP_ID)

test-gtk:
	CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)" ./scripts/test-gtk.py $(CARGO_FLAGS)

install: build
	$(MAKE) install-files

# Separate from compilation so packaging can stage an already-built binary.
install-files:
	install -Dm755 "$(CARGO_TARGET_DIR)/release/lector" "$(BIN)"
	install -Dm644 share/$(DESKTOP_ID) "$(DESKTOP)"
	install -Dm644 share/org.omarchy.Lector.svg "$(ICON)"
	install -Dm644 LICENSE "$(LICENSE_DIR)/LICENSE"
ifeq ($(DESTDIR),)
	-gtk-update-icon-cache -q -t -f "$(PREFIX)/share/icons/hicolor" >/dev/null 2>&1
	-update-desktop-database "$(PREFIX)/share/applications" >/dev/null 2>&1
endif
	@echo "Installed $(BIN)"

uninstall:
	rm -f "$(BIN)" "$(DESKTOP)" "$(ICON)" "$(LICENSE_DIR)/LICENSE"
ifeq ($(DESTDIR),)
	-gtk-update-icon-cache -q -t -f "$(PREFIX)/share/icons/hicolor" >/dev/null 2>&1
	-update-desktop-database "$(PREFIX)/share/applications" >/dev/null 2>&1
endif

# Changing the user's default is always an explicit, separate operation.
default:
	@test -z "$(DESTDIR)" || { echo "Cannot set a default in a staging directory"; exit 1; }
	@test -f "$(DESKTOP)" || { echo "Run make install first"; exit 1; }
	xdg-mime default $(DESKTOP_ID) application/pdf
