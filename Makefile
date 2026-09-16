PREFIX ?= $(HOME)/.local
BIN := $(PREFIX)/bin/lector
DESKTOP_ID := org.omarchy.Lector.desktop
DESKTOP := $(PREFIX)/share/applications/$(DESKTOP_ID)
ICON := $(PREFIX)/share/icons/hicolor/scalable/apps/org.omarchy.Lector.svg
LEGACY_BIN := $(PREFIX)/bin/omadoc
LEGACY_DESKTOP := $(PREFIX)/share/applications/org.omarchy.Omdoc.desktop
LEGACY_ICON := $(PREFIX)/share/icons/hicolor/scalable/apps/org.omarchy.Omdoc.svg

.PHONY: build install uninstall default

build:
	cargo build --release

install: build
	install -Dm755 target/release/lector $(BIN)
	install -Dm644 share/org.omarchy.Lector.desktop $(DESKTOP)
	install -Dm644 share/org.omarchy.Lector.svg $(ICON)
	rm -f $(LEGACY_BIN) $(LEGACY_DESKTOP) $(LEGACY_ICON)
	-gtk-update-icon-cache -f $(PREFIX)/share/icons/hicolor >/dev/null 2>&1
	-update-desktop-database $(PREFIX)/share/applications >/dev/null 2>&1
	@echo "Installed $(BIN)"
	@echo "To make it the default PDF handler:"
	@echo "  xdg-mime default $(DESKTOP_ID) application/pdf"

uninstall:
	rm -f $(BIN) $(DESKTOP) $(ICON)

default: install
	xdg-mime default $(DESKTOP_ID) application/pdf
