//! Live Omarchy theme: palette, font, radius, chrome alpha.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gdk, glib, CssProvider, STYLE_PROVIDER_PRIORITY_USER};

#[derive(Clone, Debug)]
pub struct Theme {
    pub mode: String,
    pub accent: String,
    pub selection: String,
    pub muted: String,
    pub background: String,
    pub dark_background: String,
    pub darker_background: String,
    pub lighter_background: String,
    pub foreground: String,
    pub dark_foreground: String,
    pub light_foreground: String,
    pub bright_foreground: String,
    pub red: String,
    pub yellow: String,
    pub orange: String,
    pub green: String,
    pub cyan: String,
    pub blue: String,
    pub magenta: String,
    pub font: String,
    pub font_px: i32,
    pub radius: i32,
    pub chrome_alpha: f32,
    pub name: String,
    pub menu_bg: String,
    pub menu_alpha: f32,
    pub menu_text: String,
    pub menu_sel: String,
    pub menu_sel_alpha: f32,
    pub menu_sel_text: String,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            mode: "dark".into(),
            accent: "#7aa2f7".into(),
            selection: "#292e42".into(),
            muted: "#414868".into(),
            background: "#1a1b26".into(),
            dark_background: "#13141c".into(),
            darker_background: "#0e0e14".into(),
            lighter_background: "#24283b".into(),
            foreground: "#c0caf5".into(),
            dark_foreground: "#565f89".into(),
            light_foreground: "#b4bee6".into(),
            bright_foreground: "#c0caf5".into(),
            red: "#f7768e".into(),
            yellow: "#e0af68".into(),
            orange: "#ff9e64".into(),
            green: "#9ece6a".into(),
            cyan: "#7dcfff".into(),
            blue: "#7aa2f7".into(),
            magenta: "#bb9af7".into(),
            font: "JetBrainsMono Nerd Font".into(),
            font_px: 12,
            radius: 8,
            chrome_alpha: 0.95,
            name: "default".into(),
            menu_bg: "#1a1b26".into(),
            menu_alpha: 0.93,
            menu_text: "#565f89".into(),
            menu_sel: "#7aa2f7".into(),
            menu_sel_alpha: 0.13,
            menu_sel_text: "#c0caf5".into(),
        }
    }
}

impl Theme {
    pub fn load() -> Self {
        let mut t = Self::default();
        if let Some(dir) = theme_dir() {
            t.name = fs::read_to_string(dir.join("../theme.name"))
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| {
                    dir.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "omarchy".into())
                });
            if let Ok(text) = fs::read_to_string(dir.join("colors.toml")) {
                apply_toml(&mut t, &text);
            }
            if let Ok(text) = fs::read_to_string(dir.join("shell.toml")) {
                apply_shell(&mut t, &text);
            }
        }
        t.font = omarchy_font();
        t.radius = hypr_rounding().unwrap_or(t.radius);
        t
    }

    pub fn css(&self) -> String {
        let r = self.radius;
        let font = css_font(&self.font);
        format!(
            r#"
window.lector-window {{
  background-color: {bg};
  color: {fg};
  font-family: "{font}";
  font-size: {px}px;
}}
.lector-toolbar {{
  background-color: {bg};
  color: {fg};
  border-bottom: 1px solid alpha({accent}, 0.28);
  padding: 6px 10px;
  min-height: 40px;
}}
.lector-doc-title {{
  color: {fg};
  opacity: 0.72;
  padding: 0 8px;
}}
.lector-cluster {{
  background: alpha({fg}, 0.04);
  border: 1px solid alpha({fg}, 0.18);
  border-radius: {rplus}px;
  padding: 2px;
}}
.lector-pill, .lector-pill.flat, button.lector-pill {{
  background: transparent;
  color: {fg};
  border: none;
  border-radius: {r}px;
  min-height: 26px;
  min-width: 28px;
  padding: 2px 8px;
  box-shadow: none;
}}
.lector-pill:hover {{
  background: alpha({fg}, 0.10);
}}
.lector-pill:active, togglebutton.lector-pill:checked {{
  background: alpha({accent}, 0.22);
  color: {accent};
}}
.lector-page-entry {{
  background: transparent;
  border: none;
  box-shadow: none;
  min-width: 2.5em;
  padding: 0 4px;
}}
.lector-zoom {{
  min-width: 110px;
  padding: 0 4px;
}}
.lector-search, .lector-sidebar, .lector-find {{
  background-color: {dark};
  color: {fg};
}}
.lector-find {{
  padding: 6px 12px;
  border-bottom: 1px solid alpha({accent}, 0.22);
}}
.lector-sidebar {{
  background-color: {dark};
  border-right: 1px solid alpha({fg}, 0.08);
  min-width: 196px;
}}
.lector-side-switcher {{
  padding: 6px;
  border-top: 1px solid alpha({fg}, 0.08);
}}
.lector-canvas {{
  background-color: {darker};
  padding: 16px 0;
}}
.lector-page {{
  background-color: #ffffff;
  opacity: 1;
  box-shadow: 0 2px 10px alpha(#000000, 0.40);
}}
button {{
  background: alpha({fg}, 0.04);
  color: {fg};
  border: 1px solid alpha({fg}, 0.18);
  border-radius: {r}px;
  padding: 4px 10px;
  min-height: 28px;
  box-shadow: none;
}}
button:hover {{
  background: alpha({fg}, 0.10);
  border-color: alpha({accent}, 0.55);
}}
button:active, button:checked, togglebutton:checked {{
  background: alpha({accent}, 0.22);
  border-color: {accent};
  color: {bright};
}}
button.flat {{
  background: transparent;
  border-color: transparent;
}}
popover.lector-menu button.lector-menu-item,
button.lector-menu-item {{
  background: transparent;
  color: {menu_fg};
  border: 1px solid transparent;
  border-radius: {r}px;
  padding: 4px 8px;
  min-height: 28px;
  min-width: 0;
  font-weight: 500;
  box-shadow: none;
  outline: none;
}}
popover.lector-menu button.lector-menu-item:hover,
popover.lector-menu button.lector-menu-item:focus,
popover.lector-menu button.lector-menu-item:active,
button.lector-menu-item:hover,
button.lector-menu-item:focus {{
  background: alpha({menu_sel}, {menu_sel_a});
  border-color: alpha({accent}, 0.74);
  color: {menu_hi};
}}
popover.lector-menu .lector-menu-icon {{
  color: {menu_fg};
  opacity: 0.85;
}}
popover.lector-menu .lector-menu-label {{
  color: {menu_fg};
  font-weight: 500;
}}
popover.lector-menu .lector-menu-key {{
  color: {menu_fg};
  opacity: 0.48;
  font-size: 10pt;
  font-weight: 500;
}}
popover.lector-menu .lector-menu-heading {{
  color: {menu_fg};
  opacity: 0.50;
  font-size: 9pt;
  font-weight: 700;
  letter-spacing: 0.14em;
  padding: 6px 10px 2px;
}}
popover.lector-menu .lector-menu-heading:first-child {{
  padding-top: 1px;
}}
popover.lector-menu button.lector-menu-item:hover .lector-menu-label,
popover.lector-menu button.lector-menu-item:focus .lector-menu-label,
popover.lector-menu button.lector-menu-item:hover .lector-menu-key,
popover.lector-menu button.lector-menu-item:focus .lector-menu-key,
popover.lector-menu button.lector-menu-item:hover .lector-menu-icon,
popover.lector-menu button.lector-menu-item:focus .lector-menu-icon {{
  color: {menu_hi};
  opacity: 1;
}}
.lector-menu-sep {{
  background: alpha({menu_fg}, 0.22);
  min-height: 1px;
  margin: 4px 8px;
}}
entry, spinbutton {{
  background: alpha({lighter}, 0.9);
  color: {fg};
  border: 1px solid alpha({fg}, 0.18);
  border-radius: {r}px;
  padding: 4px 8px;
  caret-color: {accent};
}}
entry:focus, spinbutton:focus {{
  border-color: {accent};
}}
label {{
  color: {fg};
}}
scrolledwindow {{
  background: transparent;
  border: none;
}}
scrollbar {{
  background: transparent;
}}
scrollbar slider {{
  background: alpha({fg}, 0.28);
  border-radius: 99px;
  min-width: 8px;
  min-height: 8px;
}}
scrollbar slider:hover {{
  background: {accent};
}}
/* gtk.css paints `.background` opaque; GtkPopover uses that class, so the
   xdg-popup is a black rectangle around any rounded contents. Keep the
   popover surface empty and draw only the inner card. Provider is USER
   so this beats ~/.config/gtk-4.0/gtk.css. */
popover, popover.background {{
  background-color: transparent;
  background-image: none;
  color: {fg};
  border: none;
  padding: 0;
  margin: 0;
  box-shadow: none;
  outline: none;
}}
popover contents, popover > contents {{
  background-color: transparent;
  background-image: none;
  color: {fg};
  border: none;
  padding: 0;
  margin: 0;
  box-shadow: none;
  border-radius: 0;
}}
popover arrow, popover > arrow {{
  background-color: transparent;
  background-image: none;
  border: none;
  min-width: 0;
  min-height: 0;
  margin: 0;
  padding: 0;
  box-shadow: none;
}}
popover.lector-menu,
popover.lector-menu.background {{
  background-color: alpha({menu_bg}, {menu_a});
  background-image: none;
  padding: 0;
  margin: 0;
  border: none;
  box-shadow: none;
}}
popover.lector-menu contents,
popover.lector-menu > contents,
popover.lector-menu.background > contents {{
  background-color: alpha({menu_bg}, {menu_a});
  background-image: none;
  color: {menu_fg};
  padding: 6px;
  margin: 0;
  border: 2px solid alpha({accent}, 0.76);
  /* Fill the xdg-popup; Hyprland already rounds the surface. A GTK
     radius here punches black corners around the card. */
  border-radius: 0;
  box-shadow: none;
  min-width: 0;
}}
.lector-menu-card {{
  background-color: transparent;
  background-image: none;
  border: none;
  padding: 0;
  min-width: 0;
}}
.lector-outline row:selected, .lector-thumbs row:selected {{
  background: alpha({accent}, 0.22);
  border-radius: {r}px;
}}
.lector-outline row:hover, .lector-thumbs row:hover {{
  background: alpha({fg}, 0.08);
  border-radius: {r}px;
}}
tooltip {{
  background: alpha({bg}, 0.97);
  color: {fg};
  border: 1px solid {accent};
  border-radius: {r}px;
}}
"#,
            bg = self.background,
            dark = self.dark_background,
            darker = self.darker_background,
            lighter = self.lighter_background,
            fg = self.foreground,
            bright = self.bright_foreground,
            accent = self.accent,
            menu_bg = self.menu_bg,
            menu_a = self.menu_alpha.clamp(0.4, 1.0),
            menu_fg = self.menu_text,
            menu_sel = self.menu_sel,
            menu_sel_a = self.menu_sel_alpha.clamp(0.04, 0.4),
            menu_hi = self.menu_sel_text,
            px = self.font_px.max(10),
            r = r,
            rplus = (r + 2).min(14),
            font = font,
        )
    }
}

pub fn theme_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let p = PathBuf::from(home).join(".config/omarchy/current/theme");
    if p.exists() {
        Some(p)
    } else {
        None
    }
}

pub fn apply_css(provider: &CssProvider, theme: &Theme) {
    provider.load_from_data(&theme.css());
}

pub fn install_provider(provider: &CssProvider) {
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            provider,
            STYLE_PROVIDER_PRIORITY_USER,
        );
    }
}

/// Watch the current theme directory and the theme.name pointer. Debounced.
pub fn watch<F>(callback: F) -> Option<gio::FileMonitor>
where
    F: Fn() + 'static,
{
    let dir = theme_dir()?;
    let file = gio::File::for_path(&dir);
    let monitor = file
        .monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
        .ok()?;

    let pending = std::rc::Rc::new(std::cell::Cell::new(false));
    let cb = std::rc::Rc::new(callback);

    monitor.connect_changed(move |_, _, _, _| {
        if pending.get() {
            return;
        }
        pending.set(true);
        let pending = pending.clone();
        let cb = cb.clone();
        glib::timeout_add_local_once(Duration::from_millis(180), move || {
            pending.set(false);
            cb();
        });
    });
    Some(monitor)
}

fn apply_toml(t: &mut Theme, text: &str) {
    let map = parse_simple_toml(text);
    let g = |k: &str, slot: &mut String| {
        if let Some(v) = hex_token(map.get(k)) {
            *slot = v;
        }
    };
    if let Some(v) = map.get("mode") {
        let m = v.trim();
        if m == "dark" || m == "light" {
            t.mode = m.to_string();
        }
    }
    g("accent", &mut t.accent);
    g("selection", &mut t.selection);
    g("muted", &mut t.muted);
    g("background", &mut t.background);
    g("dark_background", &mut t.dark_background);
    g("darker_background", &mut t.darker_background);
    g("lighter_background", &mut t.lighter_background);
    g("bg", &mut t.background);
    g("fg", &mut t.foreground);
    g("foreground", &mut t.foreground);
    g("dark_foreground", &mut t.dark_foreground);
    g("light_foreground", &mut t.light_foreground);
    g("bright_foreground", &mut t.bright_foreground);
    g("red", &mut t.red);
    g("yellow", &mut t.yellow);
    g("orange", &mut t.orange);
    g("green", &mut t.green);
    g("cyan", &mut t.cyan);
    g("blue", &mut t.blue);
    g("magenta", &mut t.magenta);
    t.menu_bg = t.background.clone();
    t.menu_text = if t.muted != Theme::default().muted {
        t.muted.clone()
    } else {
        t.dark_foreground.clone()
    };
    t.menu_sel = t.accent.clone();
    t.menu_sel_text = t.bright_foreground.clone();
}

fn apply_shell(t: &mut Theme, text: &str) {
    let map = parse_simple_toml(text);
    if let Some(v) = map.get("base-size") {
        if let Ok(n) = v.parse::<i32>() {
            t.font_px = n;
        }
    }
    if let Some(v) = map.get("background-alpha") {
        if let Ok(n) = v.parse::<f32>() {
            t.chrome_alpha = n;
        }
    }
    if let Some(v) = map.get("launcher.background-alpha") {
        if let Ok(n) = v.parse::<f32>() {
            t.chrome_alpha = n;
        }
    }
    if let Some(v) = hex_token(map.get("menu.background")) {
        t.menu_bg = v;
    }
    if let Some(v) = map.get("menu.background-alpha") {
        if let Ok(n) = v.parse::<f32>() {
            t.menu_alpha = n;
        }
    }
    if let Some(v) = hex_token(map.get("menu.text")) {
        t.menu_text = v;
    }
    if let Some(v) = hex_token(map.get("menu.selected-background")) {
        t.menu_sel = v;
    }
    if let Some(v) = map.get("menu.selected-background-alpha") {
        if let Ok(n) = v.parse::<f32>() {
            t.menu_sel_alpha = n;
        }
    }
    if let Some(v) = hex_token(map.get("menu.selected-text")) {
        t.menu_sel_text = v;
    }
}

fn hex_token(val: Option<&String>) -> Option<String> {
    let v = val?.trim();
    if v.starts_with('#') && (v.len() == 7 || v.len() == 9 || v.len() == 4) {
        Some(v.to_string())
    } else {
        None
    }
}

fn parse_simple_toml(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut section = String::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(s) = line.strip_prefix('[') {
            if let Some(name) = s.strip_suffix(']') {
                section = name.trim().to_string();
            }
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let key = k.trim();
        let mut val = v.trim().to_string();
        if val.starts_with('"') && val.ends_with('"') && val.len() >= 2 {
            val = val[1..val.len() - 1].to_string();
        }
        map.insert(key.to_string(), val.clone());
        if !section.is_empty() {
            map.insert(format!("{section}.{key}"), val);
        }
    }
    map
}

fn omarchy_font() -> String {
    if let Ok(out) = Command::new("omarchy").args(["font", "current"]).output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                return s;
            }
        }
    }
    "JetBrainsMono Nerd Font".into()
}

fn hypr_rounding() -> Option<i32> {
    let home = std::env::var_os("HOME")?;
    let p = PathBuf::from(home).join(".config/hypr/looknfeel.lua");
    let text = fs::read_to_string(p).ok()?;
    for line in text.lines() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix("rounding") {
            let rest = rest.trim().trim_start_matches('=').trim();
            let num = rest.trim_end_matches(',').trim();
            if let Ok(n) = num.parse::<i32>() {
                return Some(n);
            }
        }
    }
    None
}

fn css_font(name: &str) -> String {
    name.replace(['\\', '"', ';', '{', '}'], "")
}

pub fn path_uri(path: &Path) -> String {
    glib::filename_to_uri(path, None)
        .map(|s| s.to_string())
        .unwrap_or_else(|_| format!("file://{}", path.display()))
}
