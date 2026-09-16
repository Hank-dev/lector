use std::collections::HashSet;
use std::path::Path;

use gtk::gdk::Key;
use gtk::{gdk, gio, glib};

use crate::doc::{outline_item_has_children, OpenDoc};
use crate::session::Session;
use crate::view::{page_at_content_y, Layout, LayoutMode, Selection, Zoom, MAX_ZOOM};

pub(crate) const ZOOM_LABELS: [&str; 16] = [
    "Automatic",
    "Fit Page",
    "Fit Width",
    "Fit Height",
    "50%",
    "70%",
    "85%",
    "100%",
    "125%",
    "150%",
    "175%",
    "200%",
    "300%",
    "400%",
    "800%",
    "1600%",
];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sidebar {
    Hidden,
    Thumbs,
    Outline,
    Marks,
}

pub(crate) fn parse_sidebar(value: &str) -> Sidebar {
    match value {
        "outline" => Sidebar::Outline,
        "thumbs" => Sidebar::Thumbs,
        "marks" => Sidebar::Marks,
        _ => Sidebar::Hidden,
    }
}

pub(crate) fn sidebar_when_revealed(current: Sidebar) -> Sidebar {
    match current {
        Sidebar::Hidden => Sidebar::Outline,
        visible => visible,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PageStructure {
    pub(crate) mode: LayoutMode,
    pub(crate) indices: Vec<i32>,
}

impl PageStructure {
    pub(crate) fn new(mode: LayoutMode, indices: Vec<i32>) -> Self {
        Self { mode, indices }
    }

    pub(crate) fn from_layout(mode: LayoutMode, layout: &Layout) -> Self {
        Self::new(mode, layout.pages.iter().map(|page| page.index).collect())
    }

    #[cfg(test)]
    pub(crate) fn matches(&self, mode: LayoutMode, indices: &[i32]) -> bool {
        self.mode == mode && self.indices == indices
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SearchJob {
    pub(crate) next_page: i32,
    pub(crate) remaining: i32,
}

impl SearchJob {
    pub(crate) fn after_first_page(n_pages: i32, current_page: i32) -> Option<Self> {
        (n_pages > 1).then(|| Self {
            next_page: (current_page + 1).rem_euclid(n_pages),
            remaining: n_pages - 1,
        })
    }

    pub(crate) fn take_page(&mut self, n_pages: i32) -> Option<i32> {
        if self.remaining <= 0 || n_pages <= 0 {
            return None;
        }
        let page = self.next_page.rem_euclid(n_pages);
        self.next_page = (page + 1).rem_euclid(n_pages);
        self.remaining -= 1;
        Some(page)
    }
}

pub(crate) fn restore_presentation_state(
    presentation: &mut bool,
    restore: &mut Option<(LayoutMode, Zoom, Sidebar)>,
    layout: &mut LayoutMode,
    zoom: &mut Zoom,
    sidebar: &mut Sidebar,
) -> bool {
    if !*presentation {
        return false;
    }
    *presentation = false;
    if let Some((saved_layout, saved_zoom, saved_sidebar)) = restore.take() {
        *layout = saved_layout;
        *zoom = saved_zoom;
        *sidebar = saved_sidebar;
    }
    true
}

pub(crate) fn toggle_outline_collapsed(
    items: &[crate::doc::OutlineItem],
    collapsed: &mut HashSet<usize>,
    index: usize,
) -> bool {
    if !outline_item_has_children(items, index) {
        return false;
    }
    if !collapsed.insert(index) {
        collapsed.remove(&index);
    }
    true
}

pub(crate) fn allocation_changed(previous: (i32, i32), current: (i32, i32)) -> bool {
    previous != current && current.0 > 32 && current.1 > 32
}

pub(crate) const LIVE_SEARCH_MS: u64 = 220;
pub(crate) const FILE_CHANGE_MS: u64 = 400;
pub(crate) const FILE_WATCH_GRACE_MS: u64 = 500;
pub(crate) const SEARCH_SNIPPET_CHARS: usize = 48;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiskChange {
    Modified,
    Removed,
}

pub(crate) fn classify_document_event(event: gio::FileMonitorEvent) -> Option<DiskChange> {
    match event {
        gio::FileMonitorEvent::Changed
        | gio::FileMonitorEvent::ChangesDoneHint
        | gio::FileMonitorEvent::Created
        | gio::FileMonitorEvent::Moved
        | gio::FileMonitorEvent::Renamed
        | gio::FileMonitorEvent::MovedIn => Some(DiskChange::Modified),
        gio::FileMonitorEvent::Deleted | gio::FileMonitorEvent::MovedOut => {
            Some(DiskChange::Removed)
        }
        _ => None,
    }
}

pub(crate) fn collapsed_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn ellipsize_chars(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    let keep = max_chars.saturating_sub(1);
    format!("{}…", text.chars().take(keep).collect::<String>())
}

pub(crate) fn search_snippet(text: &str, query: &str, case_sensitive: bool) -> String {
    let hay = collapsed_whitespace(text);
    if hay.is_empty() {
        return String::new();
    }
    let query = query.trim();
    let start = if query.is_empty() {
        0
    } else {
        let found = if case_sensitive {
            hay.find(query)
        } else {
            hay.to_lowercase().find(&query.to_lowercase())
        };
        match found {
            Some(index) => hay[..index]
                .trim_end()
                .rfind(' ')
                .map(|space| space + 1)
                .unwrap_or(0),
            None => 0,
        }
    };
    ellipsize_chars(&hay[start..], SEARCH_SNIPPET_CHARS)
}

pub(crate) fn search_result_row_text(label: &str, snippet: &str) -> String {
    if snippet.is_empty() {
        format!("Page {label}")
    } else {
        format!("{label}  {snippet}")
    }
}

pub(crate) fn remember_document_state(
    session: &mut Session,
    path: &Path,
    page: i32,
    zoom: Zoom,
    rotation: i32,
) {
    session.remember_with_rotation(path, page, zoom, rotation);
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ViewLocation {
    pub(crate) page: i32,
    pub(crate) horizontal: f64,
    pub(crate) vertical: f64,
}

impl ViewLocation {
    pub(crate) fn new(page: i32, horizontal: f64, vertical: f64) -> Self {
        Self {
            page,
            horizontal,
            vertical,
        }
    }
}

#[derive(Default)]
pub(crate) struct NavigationHistory {
    pub(crate) back: Vec<ViewLocation>,
    pub(crate) forward: Vec<ViewLocation>,
}

impl NavigationHistory {
    pub(crate) fn visit(&mut self, from: ViewLocation, to: ViewLocation) {
        if from == to {
            return;
        }
        if self.back.last().copied() != Some(from) {
            self.back.push(from);
            if self.back.len() > 100 {
                self.back.remove(0);
            }
        }
        self.forward.clear();
    }

    pub(crate) fn back(&mut self, current: ViewLocation) -> Option<ViewLocation> {
        let target = self.back.pop()?;
        if self.forward.last().copied() != Some(current) {
            self.forward.push(current);
        }
        Some(target)
    }

    pub(crate) fn forward(&mut self, current: ViewLocation) -> Option<ViewLocation> {
        let target = self.forward.pop()?;
        if self.back.last().copied() != Some(current) {
            self.back.push(current);
        }
        Some(target)
    }

    pub(crate) fn clear(&mut self) {
        self.back.clear();
        self.forward.clear();
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ViewportAnchor {
    pub(crate) page: i32,
    pub(crate) rel_x: f64,
    pub(crate) rel_y: f64,
    pub(crate) view_x: f64,
    pub(crate) view_y: f64,
}

pub(crate) fn anchor_from_content(
    pages: &[crate::view::PageGeom],
    content_x: f64,
    content_y: f64,
    view_x: f64,
    view_y: f64,
) -> ViewportAnchor {
    let page = page_at_content_y(pages, content_y);
    let geom = pages
        .iter()
        .copied()
        .find(|page_geom| page_geom.index == page)
        .or_else(|| pages.first().copied());
    let Some(geom) = geom else {
        return ViewportAnchor {
            page: 0,
            rel_x: 0.5,
            rel_y: 0.0,
            view_x,
            view_y,
        };
    };
    ViewportAnchor {
        page: geom.index,
        rel_x: if geom.w > 0.0 {
            (content_x - geom.x) / geom.w
        } else {
            0.5
        },
        rel_y: if geom.h > 0.0 {
            (content_y - geom.y) / geom.h
        } else {
            0.0
        },
        view_x,
        view_y,
    }
}

pub(crate) fn restored_scroll(
    anchor: ViewportAnchor,
    pages: &[crate::view::PageGeom],
) -> (f64, f64) {
    let Some(geom) = pages.iter().find(|page| page.index == anchor.page) else {
        return (0.0, 0.0);
    };
    (
        geom.x + anchor.rel_x * geom.w - anchor.view_x,
        geom.y + anchor.rel_y * geom.h - anchor.view_y,
    )
}

pub(crate) fn zoom_from_factor(current_scale: f64, factor: f64) -> Zoom {
    Zoom::Custom((current_scale * factor).clamp(0.1, MAX_ZOOM))
}

pub(crate) fn pinch_target_scale(start_scale: f64, gesture_scale: f64) -> f64 {
    (start_scale * gesture_scale).clamp(0.1, MAX_ZOOM)
}

pub(crate) fn pinch_scale_is_meaningful(applied: f64, target: f64) -> bool {
    (target - applied).abs() / applied.max(0.01) >= 0.02
}

pub(crate) fn pinch_zoom_update(
    start_scale: f64,
    gesture_scale: f64,
    anchor: ViewportAnchor,
) -> (Zoom, ViewRestore) {
    (
        Zoom::Custom(pinch_target_scale(start_scale, gesture_scale)),
        ViewRestore::Anchor(anchor),
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ZoomChrome {
    Preset(u32),
    Percent(i32),
}

pub(crate) fn zoom_chrome(zoom: Zoom) -> ZoomChrome {
    match zoom {
        Zoom::FitPage => ZoomChrome::Preset(1),
        Zoom::FitWidth => ZoomChrome::Preset(2),
        Zoom::FitHeight => ZoomChrome::Preset(3),
        Zoom::Actual => ZoomChrome::Preset(7),
        Zoom::Custom(scale) => {
            let percent = (scale * 100.0).round() as i32;
            match percent {
                50 => ZoomChrome::Preset(4),
                70 => ZoomChrome::Preset(5),
                85 => ZoomChrome::Preset(6),
                100 => ZoomChrome::Preset(7),
                125 => ZoomChrome::Preset(8),
                150 => ZoomChrome::Preset(9),
                175 => ZoomChrome::Preset(10),
                200 => ZoomChrome::Preset(11),
                300 => ZoomChrome::Preset(12),
                400 => ZoomChrome::Preset(13),
                800 => ZoomChrome::Preset(14),
                1600 => ZoomChrome::Preset(15),
                _ => ZoomChrome::Percent(percent),
            }
        }
    }
}

pub(crate) fn viewport_resize_should_reflow(ignore_scroll: bool) -> bool {
    !ignore_scroll
}

pub(crate) fn canvas_scroll_response(ctrl: bool, dy: f64, pinch_active: bool) -> glib::Propagation {
    if pinch_active || (ctrl && dy != 0.0) {
        glib::Propagation::Stop
    } else {
        glib::Propagation::Proceed
    }
}

pub(crate) fn pinch_begin_ignore_scroll() -> bool {
    false
}

pub(crate) fn resolved_disk_change(
    pending: Option<DiskChange>,
    path_exists: bool,
) -> Option<DiskChange> {
    pending.map(|_| {
        if path_exists {
            DiskChange::Modified
        } else {
            DiskChange::Removed
        }
    })
}

pub(crate) fn ctrl_scroll_zoom_factor(dy: f64) -> f64 {
    if dy == 0.0 {
        1.0
    } else if dy.abs() >= 1.0 {
        if dy < 0.0 {
            1.1
        } else {
            1.0 / 1.1
        }
    } else {
        1.1_f64.powf(-dy)
    }
}

pub(crate) fn clamp_adjustment(value: f64, lower: f64, upper: f64, page: f64) -> f64 {
    let max = (upper - page).max(lower);
    value.clamp(lower, max)
}

pub(crate) fn continuous_scroll_page_changed(current: i32, visible: i32) -> Option<i32> {
    (visible != current).then_some(visible)
}

pub(crate) const ARROW_SCROLL_STEP: f64 = 64.0;

pub(crate) fn continuous_arrow_scroll_target(
    mode: LayoutMode,
    key: Key,
    value: f64,
    lower: f64,
    upper: f64,
    page_size: f64,
) -> Option<f64> {
    if mode != LayoutMode::Continuous {
        return None;
    }
    let delta = match key {
        Key::Up | Key::KP_Up => -ARROW_SCROLL_STEP,
        Key::Down | Key::KP_Down => ARROW_SCROLL_STEP,
        _ => return None,
    };
    Some(clamp_adjustment(value + delta, lower, upper, page_size))
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ShortcutRow {
    pub(crate) action: &'static str,
    pub(crate) keys: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ShortcutSection {
    pub(crate) heading: &'static str,
    pub(crate) rows: &'static [ShortcutRow],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HistoryDirection {
    Back,
    Forward,
}

pub(crate) fn navigation_shortcut(key: Key, mods: gdk::ModifierType) -> Option<HistoryDirection> {
    if !mods.contains(gdk::ModifierType::ALT_MASK) {
        return None;
    }
    match key {
        Key::Left => Some(HistoryDirection::Back),
        Key::Right => Some(HistoryDirection::Forward),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AnnotationShortcut {
    History,
    Highlight,
    Note,
}

pub(crate) fn annotation_shortcut(key: Key, mods: gdk::ModifierType) -> Option<AnnotationShortcut> {
    if !mods.contains(gdk::ModifierType::CONTROL_MASK) {
        return None;
    }
    let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
    match key {
        Key::h | Key::H if shift => Some(AnnotationShortcut::Highlight),
        Key::h | Key::H => Some(AnnotationShortcut::History),
        Key::n | Key::N if shift => Some(AnnotationShortcut::Note),
        _ => None,
    }
}

pub(crate) fn begin_selection_state(
    selection: &mut Option<Selection>,
    targeted_annotation: &mut Option<String>,
    hit: Option<(i32, f64, f64)>,
) {
    *targeted_annotation = None;
    *selection = hit.map(|(page, x, y)| Selection {
        page,
        x1: x,
        y1: y,
        x2: x,
        y2: y,
    });
}

pub(crate) fn replace_annotation_target(
    target: &mut Option<String>,
    replacement: Option<String>,
) -> bool {
    if *target == replacement {
        return false;
    }
    *target = replacement;
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AboutCopy {
    pub(crate) name: &'static str,
    pub(crate) tagline: &'static str,
    pub(crate) detail: &'static str,
    pub(crate) version: &'static str,
}

pub(crate) fn shortcut_sections() -> &'static [ShortcutSection] {
    &[
        ShortcutSection {
            heading: "FILE",
            rows: &[
                ShortcutRow {
                    action: "Open",
                    keys: "Ctrl+O",
                },
                ShortcutRow {
                    action: "Save a copy",
                    keys: "Ctrl+S",
                },
                ShortcutRow {
                    action: "Print",
                    keys: "Ctrl+P",
                },
                ShortcutRow {
                    action: "Reading history",
                    keys: "Ctrl+H",
                },
            ],
        },
        ShortcutSection {
            heading: "ANNOTATIONS",
            rows: &[
                ShortcutRow {
                    action: "Highlight selection",
                    keys: "Ctrl+Shift+H",
                },
                ShortcutRow {
                    action: "Add note to selection",
                    keys: "Ctrl+Shift+N",
                },
                ShortcutRow {
                    action: "Delete targeted annotation",
                    keys: "Delete",
                },
            ],
        },
        ShortcutSection {
            heading: "VIEW",
            rows: &[
                ShortcutRow {
                    action: "Side pane",
                    keys: "F9",
                },
                ShortcutRow {
                    action: "Continuous",
                    keys: "c",
                },
                ShortcutRow {
                    action: "Dual",
                    keys: "d",
                },
                ShortcutRow {
                    action: "Single page",
                    keys: "s",
                },
                ShortcutRow {
                    action: "Fit width",
                    keys: "w",
                },
                ShortcutRow {
                    action: "Fit page",
                    keys: "a",
                },
                ShortcutRow {
                    action: "Actual size",
                    keys: "Ctrl+0",
                },
                ShortcutRow {
                    action: "Zoom in / out",
                    keys: "Ctrl++ / Ctrl+-",
                },
                ShortcutRow {
                    action: "Pinch to zoom",
                    keys: "Pinch",
                },
                ShortcutRow {
                    action: "Invert colors",
                    keys: "Ctrl+I",
                },
                ShortcutRow {
                    action: "Rotate",
                    keys: "Ctrl+R",
                },
                ShortcutRow {
                    action: "Fullscreen",
                    keys: "F11",
                },
                ShortcutRow {
                    action: "Presentation",
                    keys: "F5",
                },
            ],
        },
        ShortcutSection {
            heading: "FIND",
            rows: &[
                ShortcutRow {
                    action: "Find",
                    keys: "Ctrl+F",
                },
                ShortcutRow {
                    action: "Find next",
                    keys: "Ctrl+G",
                },
                ShortcutRow {
                    action: "Copy text",
                    keys: "Ctrl+C",
                },
            ],
        },
        ShortcutSection {
            heading: "NAVIGATION",
            rows: &[
                ShortcutRow {
                    action: "Back / forward",
                    keys: "Alt+← / Alt+→",
                },
                ShortcutRow {
                    action: "Next page",
                    keys: "n",
                },
                ShortcutRow {
                    action: "Previous page",
                    keys: "p",
                },
                ShortcutRow {
                    action: "Scroll continuously",
                    keys: "↑ / ↓",
                },
                ShortcutRow {
                    action: "First / last page",
                    keys: "Home / End",
                },
            ],
        },
        ShortcutSection {
            heading: "DOCUMENT",
            rows: &[ShortcutRow {
                action: "Reload",
                keys: "Ctrl+Shift+R",
            }],
        },
    ]
}

pub(crate) fn display_meta(value: Option<&str>) -> String {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("—")
        .to_string()
}

pub(crate) fn property_fields(doc: &OpenDoc) -> Vec<(&'static str, String)> {
    vec![
        ("File", doc.path.display().to_string()),
        ("Title", doc.title.clone()),
        ("Author", display_meta(doc.author.as_deref())),
        ("Subject", display_meta(doc.subject.as_deref())),
        ("Creator", display_meta(doc.creator.as_deref())),
        ("Producer", display_meta(doc.producer.as_deref())),
        ("PDF", display_meta(doc.pdf_version.as_deref())),
        ("Pages", doc.n_pages.to_string()),
    ]
}

pub(crate) fn about_copy() -> AboutCopy {
    AboutCopy {
        name: "Lector",
        tagline: "Native PDF viewer for Omarchy",
        detail: "Poppler rendering · live theme follow · marks stay in Lector, not in the PDF",
        version: env!("CARGO_PKG_VERSION"),
    }
}

pub(crate) fn marks_empty_copy() -> &'static str {
    "No marks yet. Highlights and notes stay in Lector, not in the PDF."
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ViewRestore {
    Anchor(ViewportAnchor),
    CurrentPage,
    Location(ViewLocation),
}
