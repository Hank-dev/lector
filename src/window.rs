use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

use gtk::gdk::Key;
use gtk::prelude::*;
use gtk::{
    gdk, gio, glib, Application, ApplicationWindow, Box as GtkBox, Button, CheckButton,
    CssProvider, DrawingArea, DropDown, Entry, GestureClick, GestureDrag, GestureZoom, Image,
    Label, ListBox, MenuButton, Orientation, PasswordEntry, Popover, Revealer, ScrolledWindow,
    Separator, Stack, StackSwitcher, ToggleButton,
};

use crate::annotations::{
    display_rect_to_document, document_rect_to_display, new_annotation, Annotation, AnnotationKind,
    AnnotationStore, DocumentRect,
};
use crate::doc::{DocumentLink, OpenDoc, OpenError};
use crate::session::{self, Session};
use crate::theme::{self, Theme};
use crate::view::{
    display_point_to_document, draw_annotations, draw_page_overlays, page_at_content_y,
    page_scroll_y, pages_intersecting_view, paint_raster, raster_cache_needs_reset,
    raster_idle_is_stale, raster_page, selected_text, thumbnail, thumbnail_css_size,
    AnnotationDrawing, Layout, LayoutMode, PageDrawing, RasterCache, RasterKey, Selection, Zoom,
    MAX_ZOOM, THUMB_CSS_WIDTH, VIEW_PAGE_PAD,
};

const ZOOM_LABELS: [&str; 16] = [
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
enum Sidebar {
    Hidden,
    Thumbs,
    Outline,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PageStructure {
    mode: LayoutMode,
    indices: Vec<i32>,
}

impl PageStructure {
    fn new(mode: LayoutMode, indices: Vec<i32>) -> Self {
        Self { mode, indices }
    }

    fn from_layout(mode: LayoutMode, layout: &Layout) -> Self {
        Self::new(mode, layout.pages.iter().map(|page| page.index).collect())
    }

    #[cfg(test)]
    fn matches(&self, mode: LayoutMode, indices: &[i32]) -> bool {
        self.mode == mode && self.indices == indices
    }
}

#[derive(Clone, Copy, Debug)]
struct SearchJob {
    next_page: i32,
    remaining: i32,
}

impl SearchJob {
    fn after_first_page(n_pages: i32, current_page: i32) -> Option<Self> {
        (n_pages > 1).then(|| Self {
            next_page: (current_page + 1).rem_euclid(n_pages),
            remaining: n_pages - 1,
        })
    }

    fn take_page(&mut self, n_pages: i32) -> Option<i32> {
        if self.remaining <= 0 || n_pages <= 0 {
            return None;
        }
        let page = self.next_page.rem_euclid(n_pages);
        self.next_page = (page + 1).rem_euclid(n_pages);
        self.remaining -= 1;
        Some(page)
    }
}

fn restore_presentation_state(
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

fn allocation_changed(previous: (i32, i32), current: (i32, i32)) -> bool {
    previous != current && current.0 > 32 && current.1 > 32
}

fn invalidate_search_for_document(
    search_gen: &mut u64,
    search_query: &mut String,
    search_hits: &mut Vec<(i32, poppler::Rectangle)>,
    search_idx: &mut Option<usize>,
) {
    *search_gen = search_gen.wrapping_add(1);
    search_query.clear();
    search_hits.clear();
    *search_idx = None;
}

fn remember_document_state(session: &mut Session, path: &Path, page: i32, zoom: Zoom) {
    session.remember(path, page, zoom);
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ViewportAnchor {
    content_x: f64,
    content_y: f64,
    view_x: f64,
    view_y: f64,
    scale: f64,
}

fn zoom_from_factor(current_scale: f64, factor: f64) -> Zoom {
    Zoom::Custom((current_scale * factor).clamp(0.1, MAX_ZOOM))
}

fn pinch_target_scale(start_scale: f64, gesture_scale: f64) -> f64 {
    (start_scale * gesture_scale).clamp(0.1, MAX_ZOOM)
}

fn pinch_scale_is_meaningful(applied: f64, target: f64) -> bool {
    (target - applied).abs() / applied.max(0.01) >= 0.02
}

fn ctrl_scroll_zoom_factor(dy: f64) -> f64 {
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

fn restored_scroll(anchor: ViewportAnchor, new_scale: f64) -> (f64, f64) {
    let factor = if anchor.scale > 0.0 {
        new_scale / anchor.scale
    } else {
        1.0
    };
    (
        anchor.content_x * factor - anchor.view_x,
        anchor.content_y * factor - anchor.view_y,
    )
}

fn clamp_adjustment(value: f64, lower: f64, upper: f64, page: f64) -> f64 {
    let max = (upper - page).max(lower);
    value.clamp(lower, max)
}

fn continuous_scroll_page_changed(current: i32, visible: i32) -> Option<i32> {
    (visible != current).then_some(visible)
}

#[derive(Clone, Copy, Debug)]
struct ShortcutRow {
    action: &'static str,
    keys: &'static str,
}

#[derive(Clone, Copy, Debug)]
struct ShortcutSection {
    heading: &'static str,
    rows: &'static [ShortcutRow],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AnnotationShortcut {
    History,
    Highlight,
    Note,
}

fn annotation_shortcut(key: Key, mods: gdk::ModifierType) -> Option<AnnotationShortcut> {
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

fn begin_selection_state(
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

fn replace_annotation_target(target: &mut Option<String>, replacement: Option<String>) -> bool {
    if *target == replacement {
        return false;
    }
    *target = replacement;
    true
}

fn rotate_view_state(rotation: &mut i32, selection: &mut Option<Selection>, degrees: i32) {
    *selection = None;
    *rotation = (*rotation + degrees).rem_euclid(360);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AboutCopy {
    name: &'static str,
    tagline: &'static str,
    detail: &'static str,
    version: &'static str,
}

fn shortcut_sections() -> &'static [ShortcutSection] {
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
                    action: "Next page",
                    keys: "n",
                },
                ShortcutRow {
                    action: "Previous page",
                    keys: "p",
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

fn display_meta(value: Option<&str>) -> String {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("—")
        .to_string()
}

fn property_fields(doc: &OpenDoc) -> Vec<(&'static str, String)> {
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

fn about_copy() -> AboutCopy {
    AboutCopy {
        name: "Lector",
        tagline: "Native PDF viewer for Omarchy",
        detail: "Poppler rendering · live theme follow",
        version: env!("CARGO_PKG_VERSION"),
    }
}

#[derive(Clone, Copy, Debug)]
enum ViewRestore {
    Anchor(ViewportAnchor),
    CurrentPage,
}

struct Inner {
    doc: Option<OpenDoc>,
    zoom: Zoom,
    layout_mode: LayoutMode,
    rotation: i32,
    invert: bool,
    page: i32,
    layout: Layout,
    selection: Option<Selection>,
    search_hits: Vec<(i32, poppler::Rectangle)>,
    search_idx: Option<usize>,
    sidebar: Sidebar,
    presentation: bool,
    theme: Theme,
    session: Session,
    annotations: Option<AnnotationStore>,
    annotation_error: Option<String>,
    targeted_annotation: Option<String>,
    view_w: f64,
    view_h: f64,
    ignore_zoom_dropdown: bool,
    ignore_scroll: bool,
    ignore_menu_checks: bool,
    layout_gen: u64,
    search_gen: u64,
    thumb_gen: u64,
    search_query: String,
    pres_restore: Option<(LayoutMode, Zoom, Sidebar)>,
    raster: RasterCache,
    raster_gen: u64,
}

pub struct Lector {
    window: ApplicationWindow,
    pages_box: GtkBox,
    scrolled: ScrolledWindow,
    toolbar: GtkBox,
    page_entry: Entry,
    page_total: Label,
    search_entry: Entry,
    search_info: Label,
    find_bar: Revealer,
    sidebar_revealer: Revealer,
    sidebar_btn: ToggleButton,
    stack: Stack,
    thumbs: ListBox,
    outline: ListBox,
    zoom_drop: DropDown,
    title_label: Label,
    prev_btn: Button,
    next_btn: Button,
    zoom_out_btn: Button,
    zoom_in_btn: Button,
    find_btn: Button,
    cont_item: CheckButton,
    dual_item: CheckButton,
    single_item: CheckButton,
    invert_item: CheckButton,
    highlight_item: Button,
    note_item: Button,
    delete_annotation_item: Button,
    thumbs_sw: ScrolledWindow,
    page_widgets: RefCell<Vec<(i32, DrawingArea)>>,
    rendered_structure: RefCell<Option<PageStructure>>,
    raster_pending: RefCell<HashSet<i32>>,
    raster_busy: Cell<bool>,
    inner: Rc<RefCell<Inner>>,
    css: CssProvider,
    me: RefCell<Weak<Lector>>,
    hold: RefCell<Option<Rc<Lector>>>,
    _monitor: RefCell<Option<gio::FileMonitor>>,
}

#[derive(Clone)]
struct Handle(Weak<Lector>);

impl Handle {
    fn get(&self) -> Option<Rc<Lector>> {
        self.0.upgrade()
    }
}

impl Lector {
    pub fn new(app: &Application) -> Rc<Self> {
        let theme = Theme::load();
        let css = CssProvider::new();
        theme::install_provider(&css);
        theme::apply_css(&css, &theme);

        let window = ApplicationWindow::builder()
            .application(app)
            .title("")
            .default_width(1100)
            .default_height(860)
            .build();
        window.add_css_class("lector-window");
        window.set_titlebar(None::<&gtk::Box>);

        let (annotations, annotation_error) = match AnnotationStore::load() {
            Ok(store) => (Some(store), None),
            Err(error) => (None, Some(error)),
        };

        let inner = Rc::new(RefCell::new(Inner {
            doc: None,
            zoom: Zoom::FitWidth,
            layout_mode: LayoutMode::Continuous,
            rotation: 0,
            invert: false,
            page: 0,
            layout: Layout {
                pages: Vec::new(),
                scale: 1.0,
            },
            selection: None,
            search_hits: Vec::new(),
            search_idx: None,
            sidebar: Sidebar::Thumbs,
            presentation: false,
            theme,
            session: Session::load(),
            annotations,
            annotation_error,
            targeted_annotation: None,
            view_w: 800.0,
            view_h: 700.0,
            ignore_zoom_dropdown: false,
            ignore_scroll: false,
            ignore_menu_checks: false,
            layout_gen: 0,
            search_gen: 0,
            thumb_gen: 0,
            search_query: String::new(),
            pres_restore: None,
            raster: RasterCache::new(12),
            raster_gen: 0,
        }));
        {
            let mut st = inner.borrow_mut();
            st.invert = st.session.invert;
            st.layout_mode = session::parse_layout(&st.session.layout);
            st.sidebar = match st.session.sidebar.as_str() {
                "hidden" => Sidebar::Hidden,
                "outline" => Sidebar::Outline,
                _ => Sidebar::Thumbs,
            };
        }

        // Slim Omarchy-native bar: one flat surface, no boxed clusters.
        let title_label = Label::new(Some("Open a document"));
        title_label.add_css_class("lector-doc-title");
        title_label.set_hexpand(true);
        title_label.set_ellipsize(gtk::pango::EllipsizeMode::End);

        let sidebar_btn = toggle_pill("view-list-symbolic", "Side pane (F9)");
        let open_btn = icon_pill("document-open-symbolic", "Open (Ctrl+O)");
        let prev_btn = icon_pill("go-previous-symbolic", "Previous page (p)");
        let next_btn = icon_pill("go-next-symbolic", "Next page (n)");

        let page_entry = Entry::new();
        page_entry.set_width_chars(3);
        page_entry.set_max_width_chars(5);
        page_entry.set_placeholder_text(Some("1"));
        page_entry.set_tooltip_text(Some("Go to page"));
        page_entry.add_css_class("lector-page-entry");
        page_entry.set_valign(gtk::Align::Center);
        let page_total = Label::new(Some("/ 0"));
        page_total.add_css_class("dim-label");
        page_total.set_valign(gtk::Align::Center);

        let zoom_out_btn = icon_pill("zoom-out-symbolic", "Zoom out (Ctrl+-)");
        let zoom_in_btn = icon_pill("zoom-in-symbolic", "Zoom in (Ctrl++)");
        let zoom_drop = DropDown::from_strings(&ZOOM_LABELS);
        zoom_drop.set_tooltip_text(Some("Zoom"));
        zoom_drop.set_selected(2);
        zoom_drop.set_valign(gtk::Align::Center);
        zoom_drop.add_css_class("lector-zoom");

        let find_btn = icon_pill("edit-find-symbolic", "Find (Ctrl+F)");

        let menu_btn = MenuButton::new();
        menu_btn.set_icon_name("open-menu-symbolic");
        menu_btn.set_tooltip_text(Some("More"));
        menu_btn.add_css_class("lector-pill");

        let cont_item = menu_check_row("Continuous", "c");
        let dual_item = menu_check_row("Dual", "d");
        let single_item = menu_check_row("Single page", "s");
        dual_item.set_group(Some(&cont_item));
        single_item.set_group(Some(&cont_item));
        let invert_item = menu_check_row("Inverted colors", "Ctrl+I");
        let highlight_item = bare_menu_row(
            "format-text-highlight-symbolic",
            "Highlight selection",
            "Ctrl+Shift+H",
        );
        let note_item = bare_menu_row(
            "document-edit-symbolic",
            "Add note to selection…",
            "Ctrl+Shift+N",
        );
        let delete_annotation_item = bare_menu_row(
            "edit-delete-symbolic",
            "Delete targeted annotation",
            "Delete",
        );
        highlight_item.set_sensitive(false);
        note_item.set_sensitive(false);
        delete_annotation_item.set_sensitive(false);
        {
            let st = inner.borrow();
            cont_item.set_active(st.layout_mode == LayoutMode::Continuous);
            dual_item.set_active(st.layout_mode == LayoutMode::Dual);
            single_item.set_active(st.layout_mode == LayoutMode::Single);
            invert_item.set_active(st.invert);
        }

        prev_btn.set_margin_start(8);
        zoom_out_btn.set_margin_start(8);
        find_btn.set_margin_start(8);

        let toolbar = GtkBox::new(Orientation::Horizontal, 2);
        toolbar.add_css_class("lector-toolbar");
        toolbar.set_valign(gtk::Align::Center);
        toolbar.append(&sidebar_btn);
        toolbar.append(&open_btn);
        toolbar.append(&prev_btn);
        toolbar.append(&page_entry);
        toolbar.append(&page_total);
        toolbar.append(&next_btn);
        toolbar.append(&title_label);
        toolbar.append(&zoom_out_btn);
        toolbar.append(&zoom_drop);
        toolbar.append(&zoom_in_btn);
        toolbar.append(&find_btn);
        toolbar.append(&menu_btn);

        // --- find bar (reveals under header) ---
        let search_entry = Entry::new();
        search_entry.set_placeholder_text(Some("Find"));
        search_entry.set_hexpand(true);
        let search_info = Label::new(None);
        search_info.add_css_class("dim-label");
        let find_prev = Button::from_icon_name("go-up-symbolic");
        find_prev.add_css_class("flat");
        find_prev.set_tooltip_text(Some("Previous match"));
        let find_next = Button::from_icon_name("go-down-symbolic");
        find_next.add_css_class("flat");
        find_next.set_tooltip_text(Some("Next match"));
        let find_close = Button::from_icon_name("window-close-symbolic");
        find_close.add_css_class("flat");
        let find_row = GtkBox::new(Orientation::Horizontal, 8);
        find_row.add_css_class("lector-find");
        find_row.append(&Label::new(Some("Find:")));
        find_row.append(&search_entry);
        find_row.append(&search_info);
        find_row.append(&find_prev);
        find_row.append(&find_next);
        find_row.append(&find_close);
        let find_bar = Revealer::new();
        find_bar.set_child(Some(&find_row));

        // --- side pane ---
        let thumbs = ListBox::new();
        thumbs.add_css_class("lector-thumbs");
        thumbs.set_selection_mode(gtk::SelectionMode::Single);
        let outline = ListBox::new();
        outline.add_css_class("lector-outline");
        outline.set_selection_mode(gtk::SelectionMode::Single);

        let thumbs_sw = ScrolledWindow::new();
        thumbs_sw.set_child(Some(&thumbs));
        thumbs_sw.set_vexpand(true);
        let outline_sw = ScrolledWindow::new();
        outline_sw.set_child(Some(&outline));
        outline_sw.set_vexpand(true);

        let stack = Stack::new();
        stack.set_vexpand(true);
        stack.add_titled(&thumbs_sw, Some("thumbs"), "Thumbnails");
        stack.add_titled(&outline_sw, Some("outline"), "Outline");
        let switcher = StackSwitcher::new();
        switcher.set_stack(Some(&stack));
        switcher.set_hexpand(true);
        switcher.add_css_class("lector-side-switcher");

        let side = GtkBox::new(Orientation::Vertical, 0);
        side.add_css_class("lector-sidebar");
        side.set_size_request(220, -1);
        side.set_hexpand(false);
        side.set_hexpand_set(true);
        side.append(&stack);
        side.append(&switcher);

        let sidebar_revealer = Revealer::new();
        sidebar_revealer.set_transition_type(gtk::RevealerTransitionType::SlideRight);
        sidebar_revealer.set_hexpand(false);
        sidebar_revealer.set_child(Some(&side));

        // --- canvas: one drawing area per page (GTK/GSK cannot composite a
        // 30k-px-tall surface, which is how Evince avoids this too) ---
        let pages_box = GtkBox::new(Orientation::Vertical, 16);
        pages_box.add_css_class("lector-canvas");
        // Natural size must drive the viewport. Expanding to fill clips a
        // zoomed-in page and prevents horizontal panning.
        pages_box.set_hexpand(false);
        pages_box.set_vexpand(false);
        pages_box.set_can_focus(true);
        pages_box.set_halign(gtk::Align::Center);
        pages_box.set_valign(gtk::Align::Start);

        let scrolled = ScrolledWindow::new();
        scrolled.set_child(Some(&pages_box));
        scrolled.set_hexpand(true);
        scrolled.set_vexpand(true);
        scrolled.set_kinetic_scrolling(true);
        scrolled.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);

        let body = GtkBox::new(Orientation::Horizontal, 0);
        body.set_vexpand(true);
        body.append(&sidebar_revealer);
        body.append(&scrolled);

        let root = GtkBox::new(Orientation::Vertical, 0);
        root.append(&toolbar);
        root.append(&find_bar);
        root.append(&body);
        window.set_child(Some(&root));

        let ui = Rc::new(Self {
            window: window.clone(),
            pages_box: pages_box.clone(),
            scrolled: scrolled.clone(),
            toolbar: toolbar.clone(),
            page_entry: page_entry.clone(),
            page_total: page_total.clone(),
            search_entry: search_entry.clone(),
            search_info: search_info.clone(),
            find_bar: find_bar.clone(),
            sidebar_revealer: sidebar_revealer.clone(),
            sidebar_btn: sidebar_btn.clone(),
            stack: stack.clone(),
            thumbs: thumbs.clone(),
            outline: outline.clone(),
            zoom_drop: zoom_drop.clone(),
            title_label: title_label.clone(),
            prev_btn: prev_btn.clone(),
            next_btn: next_btn.clone(),
            zoom_out_btn: zoom_out_btn.clone(),
            zoom_in_btn: zoom_in_btn.clone(),
            find_btn: find_btn.clone(),
            cont_item: cont_item.clone(),
            dual_item: dual_item.clone(),
            single_item: single_item.clone(),
            invert_item: invert_item.clone(),
            highlight_item: highlight_item.clone(),
            note_item: note_item.clone(),
            delete_annotation_item: delete_annotation_item.clone(),
            thumbs_sw: thumbs_sw.clone(),
            page_widgets: RefCell::new(Vec::new()),
            rendered_structure: RefCell::new(None),
            raster_pending: RefCell::new(HashSet::new()),
            raster_busy: Cell::new(false),
            inner,
            css,
            me: RefCell::new(Weak::new()),
            hold: RefCell::new(None),
            _monitor: RefCell::new(None),
        });
        *ui.me.borrow_mut() = Rc::downgrade(&ui);
        let h = Handle(Rc::downgrade(&ui));

        let menu_pop = Popover::new();
        menu_pop.add_css_class("lector-menu");
        menu_pop.set_has_arrow(false);
        menu_pop.set_position(gtk::PositionType::Bottom);
        menu_pop.set_offset(0, 6);
        menu_pop.set_halign(gtk::Align::End);
        menu_pop.connect_realize(|p| {
            p.remove_css_class("background");
        });
        menu_pop.connect_show({
            let btn = menu_btn.clone();
            move |p| {
                p.remove_css_class("background");
                let w = btn.width().max(1);
                let h = btn.height().max(1);
                p.set_pointing_to(Some(&gdk::Rectangle::new(w - 2, 0, 2, h)));
            }
        });
        let menu_col = GtkBox::new(Orientation::Vertical, 1);
        menu_col.add_css_class("lector-menu-card");
        append_menu_section(
            &menu_col,
            "FILE",
            &[
                ("document-open-symbolic", "Open…", "Ctrl+O", menu_open),
                (
                    "document-save-as-symbolic",
                    "Save a Copy…",
                    "Ctrl+S",
                    menu_save,
                ),
                ("document-print-symbolic", "Print…", "Ctrl+P", menu_print),
            ],
            &menu_pop,
            &h,
        );
        menu_col.append(&menu_sep());
        menu_col.append(&menu_heading("ANNOTATIONS"));
        for (button, action) in [
            (&highlight_item, menu_highlight as fn(&Lector)),
            (&note_item, menu_note),
            (&delete_annotation_item, menu_delete_annotation),
        ] {
            connect_menu_row(button, &menu_pop, &h, action);
            menu_col.append(button);
        }
        menu_col.append(&menu_sep());
        menu_col.append(&menu_heading("VIEW"));
        menu_col.append(&menu_row(
            "sidebar-show-symbolic",
            "Side pane",
            "F9",
            &menu_pop,
            &h,
            menu_sidebar,
        ));
        menu_col.append(&cont_item);
        menu_col.append(&dual_item);
        menu_col.append(&single_item);
        menu_col.append(&invert_item);
        for (icon, label, shortcut, action) in [
            (
                "object-rotate-left-symbolic",
                "Rotate left",
                "Ctrl+L",
                menu_rot_l as fn(&Lector),
            ),
            (
                "object-rotate-right-symbolic",
                "Rotate right",
                "Ctrl+R",
                menu_rot_r,
            ),
            ("view-fullscreen-symbolic", "Fullscreen", "F11", menu_fs),
            ("video-display-symbolic", "Presentation", "F5", menu_pres),
        ] {
            menu_col.append(&menu_row(icon, label, shortcut, &menu_pop, &h, action));
        }
        menu_col.append(&menu_sep());
        append_menu_section(
            &menu_col,
            "DOCUMENT",
            &[
                (
                    "view-refresh-symbolic",
                    "Reload",
                    "Ctrl+Shift+R",
                    menu_reload,
                ),
                (
                    "document-open-recent-symbolic",
                    "Reading History…",
                    "Ctrl+H",
                    menu_history,
                ),
                ("document-properties-symbolic", "Properties", "", menu_props),
                (
                    "input-keyboard-symbolic",
                    "Keyboard shortcuts",
                    "",
                    menu_keys,
                ),
                ("help-about-symbolic", "About Lector", "", menu_about),
            ],
            &menu_pop,
            &h,
        );
        menu_col.set_overflow(gtk::Overflow::Hidden);
        menu_pop.set_child(Some(&menu_col));
        menu_btn.set_popover(Some(&menu_pop));
        menu_pop.connect_show({
            let h = h.clone();
            move |_| {
                if let Some(ui) = h.get() {
                    ui.update_annotation_actions();
                }
            }
        });

        for (item, mode) in [
            (cont_item.clone(), LayoutMode::Continuous),
            (dual_item.clone(), LayoutMode::Dual),
            (single_item.clone(), LayoutMode::Single),
        ] {
            let h = h.clone();
            let pop = menu_pop.clone();
            item.connect_toggled(move |item| {
                let Some(ui) = h.get() else { return };
                if !item.is_active() || ui.inner.borrow().ignore_menu_checks {
                    return;
                }
                pop.popdown();
                ui.set_layout(mode);
            });
        }
        invert_item.connect_toggled({
            let h = h.clone();
            let pop = menu_pop.clone();
            move |item| {
                let Some(ui) = h.get() else { return };
                if ui.inner.borrow().ignore_menu_checks {
                    return;
                }
                pop.popdown();
                ui.set_invert(item.is_active());
            }
        });
        if std::env::var_os("LECTOR_DEBUG_MENU").is_some() {
            let btn = menu_btn.clone();
            let pop = menu_pop.clone();
            window.connect_map(move |_| {
                let btn = btn.clone();
                let pop = pop.clone();
                glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
                    pop.remove_css_class("background");
                    btn.set_active(true);
                    pop.popup();
                });
            });
        }

        {
            let last = std::rc::Rc::new(std::cell::Cell::new((0, 0)));
            let pending = std::rc::Rc::new(std::cell::Cell::new(false));
            let schedule = {
                let h = h.clone();
                let last = last.clone();
                let pending = pending.clone();
                let sw = scrolled.clone();
                move || {
                    if pending.get() {
                        return;
                    }
                    pending.set(true);
                    let h = h.clone();
                    let last = last.clone();
                    let pending = pending.clone();
                    let sw = sw.clone();
                    glib::timeout_add_local_once(std::time::Duration::from_millis(16), move || {
                        pending.set(false);
                        let sz = (sw.width(), sw.height());
                        if allocation_changed(last.get(), sz) {
                            last.set(sz);
                            if let Some(ui) = h.get() {
                                ui.on_resize(sz.0 as f64, sz.1 as f64);
                            }
                        }
                    });
                }
            };
            // GtkScrolledWindow has no width/height GObject properties.  A
            // tick sees its allocated size, including changes caused by the
            // sidebar and find bar, and the timeout above coalesces reflows.
            scrolled.add_tick_callback({
                let last = last.clone();
                move |sw, _| {
                    if allocation_changed(last.get(), (sw.width(), sw.height())) {
                        schedule();
                    }
                    glib::ControlFlow::Continue
                }
            });
        }
        scrolled.vadjustment().connect_value_changed({
            let h = h.clone();
            move |_| {
                if let Some(ui) = h.get() {
                    ui.on_scroll();
                }
            }
        });

        let drag = GestureDrag::new();
        drag.connect_drag_begin({
            let h = h.clone();
            move |_, x, y| {
                if let Some(ui) = h.get() {
                    ui.drag_begin(x, y);
                }
            }
        });
        drag.connect_drag_update({
            let h = h.clone();
            move |g, dx, dy| {
                if let (Some(ui), Some((ox, oy))) = (h.get(), g.start_point()) {
                    ui.drag_update(ox + dx, oy + dy);
                }
            }
        });
        pages_box.add_controller(drag);

        let click = GestureClick::new();
        click.set_button(1);
        let first_was_link = std::rc::Rc::new(std::cell::Cell::new(false));
        click.connect_released({
            let h = h.clone();
            let first_was_link = first_was_link.clone();
            move |_, n, x, y| {
                if let Some(ui) = h.get() {
                    if n == 1 {
                        if ui.activate_note_at(x, y) {
                            first_was_link.set(false);
                        } else {
                            let link = ui.activate_link_at(x, y);
                            if !link {
                                ui.target_annotation_at(x, y);
                            }
                            first_was_link.set(link);
                        }
                    } else {
                        let has_link = ui.link_at_view(x, y).is_some();
                        if should_toggle_fit(n, has_link, first_was_link.get()) {
                            ui.toggle_fit();
                        }
                        first_was_link.set(false);
                    }
                }
            }
        });
        pages_box.add_controller(click);

        // Capture-phase on the scrolled window so Ctrl+scroll can zoom
        // without a child controller stealing kinetic two-finger pans.
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
        scroll.set_propagation_phase(gtk::PropagationPhase::Capture);
        scroll.connect_scroll({
            let h = h.clone();
            move |c, _dx, dy| {
                if c.current_event_state()
                    .contains(gdk::ModifierType::CONTROL_MASK)
                    && dy != 0.0
                {
                    if let Some(ui) = h.get() {
                        ui.zoom_by_at(ctrl_scroll_zoom_factor(dy), None);
                    }
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            }
        });
        scrolled.add_controller(scroll);

        let pinch = GestureZoom::new();
        pinch.set_propagation_phase(gtk::PropagationPhase::Capture);
        let pinch_start = Rc::new(Cell::new(1.0));
        pinch.connect_begin({
            let h = h.clone();
            let pinch_start = pinch_start.clone();
            move |_, _| {
                if let Some(ui) = h.get() {
                    pinch_start.set(ui.inner.borrow().layout.scale.max(0.01));
                }
            }
        });
        pinch.connect_scale_changed({
            let h = h.clone();
            let pinch_start = pinch_start.clone();
            move |g, scale| {
                if let Some(ui) = h.get() {
                    let target_scale = pinch_target_scale(pinch_start.get(), scale);
                    if !pinch_scale_is_meaningful(ui.inner.borrow().layout.scale, target_scale) {
                        return;
                    }
                    ui.set_zoom_at(Zoom::Custom(target_scale), g.bounding_box_center());
                }
            }
        });
        scrolled.add_controller(pinch);

        let drop = gtk::DropTarget::new(gio::File::static_type(), gdk::DragAction::COPY);
        drop.connect_drop({
            let h = h.clone();
            move |_, val, _, _| {
                if let Ok(file) = val.get::<gio::File>() {
                    if let Some(path) = file.path() {
                        if let Some(ui) = h.get() {
                            ui.open_path(&path, None);
                            return true;
                        }
                    }
                }
                false
            }
        });
        window.add_controller(drop);

        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed({
            let h = h.clone();
            move |_, key, _, mods| {
                h.get()
                    .map(|ui| ui.key(key, mods))
                    .unwrap_or(glib::Propagation::Proceed)
            }
        });
        window.add_controller(keys);

        sidebar_btn.connect_toggled({
            let h = h.clone();
            move |btn| {
                if let Some(ui) = h.get() {
                    ui.set_sidebar_visible(btn.is_active());
                }
            }
        });
        open_btn.connect_clicked({
            let h = h.clone();
            move |_| {
                if let Some(ui) = h.get() {
                    ui.open_dialog();
                }
            }
        });
        prev_btn.connect_clicked({
            let h = h.clone();
            move |_| {
                if let Some(ui) = h.get() {
                    ui.goto_page_rel(-1);
                }
            }
        });
        next_btn.connect_clicked({
            let h = h.clone();
            move |_| {
                if let Some(ui) = h.get() {
                    ui.goto_page_rel(1);
                }
            }
        });
        zoom_out_btn.connect_clicked({
            let h = h.clone();
            move |_| {
                if let Some(ui) = h.get() {
                    ui.zoom_by(1.0 / 1.15);
                }
            }
        });
        zoom_in_btn.connect_clicked({
            let h = h.clone();
            move |_| {
                if let Some(ui) = h.get() {
                    ui.zoom_by(1.15);
                }
            }
        });
        find_btn.connect_clicked({
            let h = h.clone();
            move |_| {
                if let Some(ui) = h.get() {
                    ui.show_search();
                }
            }
        });
        find_prev.connect_clicked({
            let h = h.clone();
            move |_| {
                if let Some(ui) = h.get() {
                    ui.search_step(-1);
                }
            }
        });
        find_next.connect_clicked({
            let h = h.clone();
            move |_| {
                if let Some(ui) = h.get() {
                    ui.search_step(1);
                }
            }
        });
        find_close.connect_clicked({
            let h = h.clone();
            move |_| {
                if let Some(ui) = h.get() {
                    ui.hide_search();
                }
            }
        });
        page_entry.connect_activate({
            let h = h.clone();
            move |e| {
                if let (Some(ui), Ok(n)) = (h.get(), e.text().parse::<i32>()) {
                    ui.goto_page(n - 1);
                    ui.pages_box.grab_focus();
                }
            }
        });
        search_entry.connect_activate({
            let h = h.clone();
            move |e| {
                if let Some(ui) = h.get() {
                    ui.search(&e.text());
                }
            }
        });
        zoom_drop.connect_selected_notify({
            let h = h.clone();
            move |drop| {
                if let Some(ui) = h.get() {
                    ui.zoom_from_dropdown(drop.selected());
                }
            }
        });
        thumbs_sw.vadjustment().connect_value_changed({
            let h = h.clone();
            move |_| {
                if let Some(ui) = h.get() {
                    ui.schedule_thumbs();
                }
            }
        });
        thumbs.connect_row_activated({
            let h = h.clone();
            move |_, row| {
                if let Some(ui) = h.get() {
                    ui.goto_page(row.index());
                }
            }
        });
        outline.connect_row_activated({
            let h = h.clone();
            move |_, row| {
                if let Some(ui) = h.get() {
                    let page = row
                        .widget_name()
                        .trim_start_matches('p')
                        .parse::<i32>()
                        .unwrap_or(0);
                    ui.goto_page(page);
                }
            }
        });
        stack.connect_visible_child_name_notify({
            let h = h.clone();
            move |stack| {
                if let Some(ui) = h.get() {
                    let name = stack.visible_child_name();
                    let mut st = ui.inner.borrow_mut();
                    if st.sidebar == Sidebar::Hidden {
                        return;
                    }
                    st.sidebar = if name.as_deref() == Some("outline") {
                        Sidebar::Outline
                    } else {
                        Sidebar::Thumbs
                    };
                }
            }
        });
        window.connect_close_request({
            let h = h.clone();
            move |_| {
                if let Some(ui) = h.get() {
                    ui.persist();
                    ui.hold.borrow_mut().take();
                }
                glib::Propagation::Proceed
            }
        });

        let monitor = theme::watch({
            let h = h.clone();
            move || {
                if let Some(ui) = h.get() {
                    ui.reload_theme();
                }
            }
        });
        *ui._monitor.borrow_mut() = monitor;

        ui.sync_sidebar();
        ui.update_chrome();
        *ui.hold.borrow_mut() = Some(ui.clone());
        ui
    }

    pub fn present(&self) {
        self.window.present();
        if let Some(error) = self.inner.borrow().annotation_error.clone() {
            self.alert(
                "Could not load annotations",
                &format!("{error}\n\nThe annotation file will not be modified."),
            );
        }
        self.pages_box.grab_focus();
        let h = Handle(self.me.borrow().clone());
        glib::timeout_add_local_once(std::time::Duration::from_millis(50), move || {
            if let Some(ui) = h.get() {
                let w = ui.scrolled.width() as f64;
                let ht = ui.scrolled.height() as f64;
                ui.on_resize(w.max(1.0), ht.max(1.0));
            }
        });
    }

    pub fn open_uri(&self, uri: &str) {
        match OpenDoc::open_uri(uri, None) {
            Ok(doc) => self.adopt_doc(doc),
            Err(OpenError::Encrypted) => {
                if let Ok(path) = glib::filename_from_uri(uri) {
                    self.ask_password(path.0);
                } else {
                    self.alert(
                        "Could not open document",
                        "This file is encrypted and has no local path to unlock.",
                    );
                }
            }
            Err(OpenError::Other(msg)) => self.alert("Could not open document", &msg),
        }
    }

    pub fn open_path(&self, path: &Path, password: Option<&str>) {
        match OpenDoc::open(path, password) {
            Ok(doc) => self.adopt_doc(doc),
            Err(OpenError::Encrypted) => self.ask_password(path.to_path_buf()),
            Err(OpenError::Other(msg)) => self.alert("Could not open document", &msg),
        }
    }

    fn adopt_doc(&self, doc: OpenDoc) {
        if doc.n_pages <= 0 {
            self.alert("Could not open document", "The file has no pages.");
            return;
        }
        let opened_path = doc.path.clone();
        let restore = {
            let mut st = self.inner.borrow_mut();
            if let Some(old) = st.doc.as_ref() {
                let previous = (old.path.clone(), st.page, st.zoom);
                remember_document_state(&mut st.session, &previous.0, previous.1, previous.2);
            }
            let Inner {
                search_gen,
                search_query,
                search_hits,
                search_idx,
                ..
            } = &mut *st;
            invalidate_search_for_document(search_gen, search_query, search_hits, search_idx);
            st.session.restore(&doc.path)
        };
        {
            let mut st = self.inner.borrow_mut();
            if let Some(r) = restore {
                st.page = r.page.clamp(0, (doc.n_pages - 1).max(0));
                st.zoom = session::parse_zoom(&r.zoom, r.custom);
            } else {
                st.page = 0;
                st.zoom = Zoom::FitWidth;
            }
            st.doc = Some(doc);
            st.selection = None;
            st.targeted_annotation = None;
            if st.sidebar == Sidebar::Hidden {
                st.sidebar = Sidebar::Thumbs;
            }
            let page = st.page;
            let zoom = st.zoom;
            remember_document_state(&mut st.session, &opened_path, page, zoom);
        }
        self.search_entry.set_text("");
        self.search_info.set_text("");
        let page = self.inner.borrow().page;
        self.sync_zoom_dropdown();
        self.rebuild_sidebar();
        self.sync_sidebar();
        self.reflow();
        self.goto_page(page);
        self.update_chrome();
        self.queue_pages_draw();
    }

    fn ask_password(&self, path: PathBuf) {
        let pop = gtk::Window::builder()
            .transient_for(&self.window)
            .modal(true)
            .title("Enter password")
            .default_width(360)
            .build();
        let col = GtkBox::new(Orientation::Vertical, 10);
        col.set_margin_top(16);
        col.set_margin_bottom(16);
        col.set_margin_start(16);
        col.set_margin_end(16);
        col.append(&Label::new(Some("This document is encrypted.")));
        let entry = PasswordEntry::new();
        entry.set_show_peek_icon(true);
        col.append(&entry);
        let row = GtkBox::new(Orientation::Horizontal, 8);
        row.set_halign(gtk::Align::End);
        let cancel = Button::with_label("Cancel");
        let ok = Button::with_label("Unlock");
        row.append(&cancel);
        row.append(&ok);
        col.append(&row);
        pop.set_child(Some(&col));
        cancel.connect_clicked({
            let pop = pop.clone();
            move |_| pop.close()
        });
        let h = Handle(self.me.borrow().clone());
        let unlock = {
            let pop = pop.clone();
            let entry = entry.clone();
            let path = path.clone();
            let h = h.clone();
            move || {
                let pw = entry.text().to_string();
                pop.close();
                if let Some(ui) = h.get() {
                    ui.open_path(&path, Some(&pw));
                }
            }
        };
        ok.connect_clicked({
            let unlock = unlock.clone();
            move |_| unlock()
        });
        entry.connect_activate(move |_| unlock());
        pop.present();
        entry.grab_focus();
    }

    fn open_dialog(&self) {
        let dialog = gtk::FileDialog::builder().title("Open Document").build();
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("PDF Documents"));
        filter.add_mime_type("application/pdf");
        filter.add_suffix("pdf");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        dialog.set_filters(Some(&filters));
        let window = self.window.clone();
        let h = Handle(self.me.borrow().clone());
        dialog.open(Some(&window), gio::Cancellable::NONE, move |res| {
            if let Ok(file) = res {
                if let Some(path) = file.path() {
                    if let Some(ui) = h.get() {
                        ui.open_path(&path, None);
                    }
                }
            }
        });
    }

    fn draw_page(&self, cr: &gtk::cairo::Context, index: i32, dpr: f64) {
        let view_top = self.scrolled.vadjustment().value();
        let view_h = self.scrolled.height() as f64;
        let dpr = dpr.max(1.0);
        let prepared = {
            let mut st = self.inner.borrow_mut();
            let Some(geom) = st.layout.page_geom(index) else {
                return;
            };
            let near = pages_intersecting_view(&st.layout.pages, view_top, view_h, VIEW_PAGE_PAD)
                .contains(&index);
            let key = RasterKey::from_geom(&geom, st.rotation, st.invert, dpr);
            let cached = st.raster.get(&key);
            Some((
                geom,
                near,
                cached,
                st.rotation,
                st.invert,
                st.selection,
                st.search_idx,
            ))
        };
        let Some((geom, near, cached, rotation, invert, selection, search_idx)) = prepared else {
            return;
        };
        if let Some(surf) = cached {
            paint_raster(cr, &surf, geom.w, geom.h);
        } else {
            cr.set_source_rgb(1.0, 1.0, 1.0);
            cr.paint().ok();
            if near {
                self.queue_raster(index);
            }
        }
        let st = self.inner.borrow();
        let Some(doc) = st.doc.as_ref() else {
            return;
        };
        let annotations = st
            .annotations
            .as_ref()
            .map(|store| store.for_document(&doc.path))
            .unwrap_or(&[]);
        let (page_width, page_height) = doc.size(index, 0);
        draw_annotations(
            cr,
            AnnotationDrawing {
                annotations,
                page: index,
                page_width,
                page_height,
                rotation,
                scale: geom.scale,
                targeted: st.targeted_annotation.as_deref(),
            },
        );
        draw_page_overlays(
            cr,
            PageDrawing {
                doc,
                geom: &geom,
                rotation,
                invert,
                selection,
                search_hits: &st.search_hits,
                current_hit: search_idx,
            },
        );
    }

    fn on_resize(&self, w: f64, h: f64) {
        let changed = {
            let mut st = self.inner.borrow_mut();
            let nw = w.max(1.0);
            let nh = h.max(1.0);
            let changed = (st.view_w - nw).abs() > 0.5 || (st.view_h - nh).abs() > 0.5;
            st.view_w = nw;
            st.view_h = nh;
            changed
        };
        if changed {
            self.reflow();
        }
    }

    fn on_scroll(&self) {
        let st = self.inner.borrow();
        if st.ignore_scroll {
            return;
        }
        if st.layout_mode == LayoutMode::Continuous && st.doc.is_some() {
            drop(st);
            let page = page_at_content_y(
                &self.inner.borrow().layout.pages,
                self.scrolled.vadjustment().value() + 40.0,
            );
            let chrome = {
                let mut st = self.inner.borrow_mut();
                if let Some(next) = continuous_scroll_page_changed(st.page, page) {
                    st.page = next;
                    let remember = st
                        .doc
                        .as_ref()
                        .map(|doc| (doc.path.clone(), st.page, st.zoom));
                    if let Some((path, page, zoom)) = remember {
                        remember_document_state(&mut st.session, &path, page, zoom);
                    }
                    true
                } else {
                    false
                }
            };
            if chrome {
                self.update_chrome();
            }
            self.ensure_visible_rasters();
        }
    }

    fn queue_pages_draw(&self) {
        let view_top = self.scrolled.vadjustment().value();
        let view_h = self.scrolled.height() as f64;
        let visible = {
            let st = self.inner.borrow();
            pages_intersecting_view(&st.layout.pages, view_top, view_h, VIEW_PAGE_PAD)
        };
        for (index, da) in self.page_widgets.borrow().iter() {
            if visible.contains(index) {
                da.queue_draw();
            }
        }
    }

    fn queue_raster(&self, index: i32) {
        self.raster_pending.borrow_mut().insert(index);
        self.kick_raster_idle();
    }

    fn ensure_visible_rasters(&self) {
        let view_top = self.scrolled.vadjustment().value();
        let view_h = self.scrolled.height() as f64;
        let needed = {
            let st = self.inner.borrow();
            if st.doc.is_none() {
                Vec::new()
            } else {
                pages_intersecting_view(&st.layout.pages, view_top, view_h, VIEW_PAGE_PAD)
            }
        };
        if needed.is_empty() {
            return;
        }
        {
            let dpr = self.page_dpr();
            let st = self.inner.borrow();
            let mut pending = self.raster_pending.borrow_mut();
            for index in needed {
                let Some(geom) = st.layout.page_geom(index) else {
                    continue;
                };
                let key = RasterKey::from_geom(&geom, st.rotation, st.invert, dpr);
                if !st.raster.contains(&key) {
                    pending.insert(index);
                }
            }
        }
        self.kick_raster_idle();
    }

    fn kick_raster_idle(&self) {
        if self.raster_busy.get() || self.raster_pending.borrow().is_empty() {
            return;
        }
        self.raster_busy.set(true);
        let gen = self.inner.borrow().raster_gen;
        let h = Handle(self.me.borrow().clone());
        glib::idle_add_local_once(move || {
            if let Some(ui) = h.get() {
                ui.raster_one(gen);
            }
        });
    }

    fn raster_one(&self, gen: u64) {
        if raster_idle_is_stale(gen, self.inner.borrow().raster_gen) {
            self.raster_busy.set(false);
            self.kick_raster_idle();
            return;
        }
        let index = self.raster_pending.borrow_mut().iter().copied().next();
        let Some(index) = index else {
            self.raster_busy.set(false);
            return;
        };
        self.raster_pending.borrow_mut().remove(&index);
        let rendered = {
            let st = self.inner.borrow();
            match (st.doc.as_ref(), st.layout.page_geom(index)) {
                (Some(doc), Some(geom)) => {
                    let dpr = self.page_dpr();
                    let key = RasterKey::from_geom(&geom, st.rotation, st.invert, dpr);
                    raster_page(doc, &geom, st.rotation, st.invert, dpr).map(|surf| (key, surf))
                }
                _ => None,
            }
        };
        if let Some((key, surf)) = rendered {
            self.inner.borrow_mut().raster.insert(key, surf);
            if let Some((_, da)) = self.page_widgets.borrow().iter().find(|(i, _)| *i == index) {
                da.queue_draw();
            }
        }
        self.raster_busy.set(false);
        self.kick_raster_idle();
    }

    fn page_dpr(&self) -> f64 {
        self.window.scale_factor().max(1) as f64
    }

    fn hit_at(&self, x: f64, y: f64) -> Option<(i32, f64, f64)> {
        let st = self.inner.borrow();
        let widgets = self.page_widgets.borrow();
        for (index, da) in widgets.iter() {
            let Some(b) = da.compute_bounds(&self.pages_box) else {
                continue;
            };
            let x0 = b.x() as f64;
            let y0 = b.y() as f64;
            let w = b.width() as f64;
            let h = b.height() as f64;
            if x >= x0 && x <= x0 + w && y >= y0 && y <= y0 + h {
                let geom = st.layout.page_geom(*index)?;
                let scale = geom.scale.max(0.01);
                return Some((*index, (x - x0) / scale, (y - y0) / scale));
            }
        }
        None
    }

    fn alert(&self, message: &str, detail: &str) {
        let alert = gtk::AlertDialog::builder()
            .modal(true)
            .message(message)
            .detail(detail)
            .build();
        alert.show(Some(&self.window));
    }

    fn capture_anchor(&self, focus: Option<(f64, f64)>) -> ViewportAnchor {
        let (view_x, view_y) = focus.unwrap_or_else(|| {
            (
                (self.scrolled.width() as f64 * 0.5).max(0.0),
                (self.scrolled.height() as f64 * 0.5).max(0.0),
            )
        });
        ViewportAnchor {
            content_x: self.scrolled.hadjustment().value() + view_x,
            content_y: self.scrolled.vadjustment().value() + view_y,
            view_x,
            view_y,
            scale: self.inner.borrow().layout.scale.max(0.01),
        }
    }

    fn apply_anchor(&self, anchor: ViewportAnchor) {
        let scale = self.inner.borrow().layout.scale;
        let (x, y) = restored_scroll(anchor, scale);
        let h = self.scrolled.hadjustment();
        let v = self.scrolled.vadjustment();
        h.set_value(clamp_adjustment(x, h.lower(), h.upper(), h.page_size()));
        v.set_value(clamp_adjustment(y, v.lower(), v.upper(), v.page_size()));
    }

    fn schedule_view_restore(&self, restore: ViewRestore) {
        let gen = {
            let mut st = self.inner.borrow_mut();
            st.ignore_scroll = true;
            st.layout_gen = st.layout_gen.wrapping_add(1);
            st.layout_gen
        };
        let h = Handle(self.me.borrow().clone());
        glib::timeout_add_local_once(std::time::Duration::from_millis(16), move || {
            if let Some(ui) = h.get() {
                if ui.inner.borrow().layout_gen == gen {
                    match restore {
                        ViewRestore::Anchor(anchor) => ui.apply_anchor(anchor),
                        ViewRestore::CurrentPage => ui.scroll_to_current(),
                    }
                    ui.inner.borrow_mut().ignore_scroll = false;
                }
            }
        });
    }

    fn reflow(&self) {
        self.reflow_restore(ViewRestore::Anchor(self.capture_anchor(None)));
    }

    fn reflow_restore(&self, restore: ViewRestore) {
        let reset_rasters = {
            let st = &mut *self.inner.borrow_mut();
            let Some(doc) = st.doc.as_ref() else {
                return;
            };
            let vw = self.scrolled.width() as f64;
            let vh = self.scrolled.height() as f64;
            if vw > 1.0 {
                st.view_w = vw;
            }
            if vh > 1.0 {
                st.view_h = vh;
            }
            let old_scale = st.layout.scale;
            let old_rotation = st.rotation;
            let old_invert = st.invert;
            st.layout = Layout::compute(
                doc,
                st.layout_mode,
                st.zoom,
                st.rotation,
                st.page,
                st.view_w.max(200.0),
                st.view_h.max(200.0),
            );
            let reset = raster_cache_needs_reset(
                old_scale,
                st.layout.scale,
                old_rotation,
                st.rotation,
                old_invert,
                st.invert,
            );
            if reset {
                st.raster.clear();
                st.raster_gen = st.raster_gen.wrapping_add(1);
            }
            reset
        };
        if reset_rasters {
            self.raster_pending.borrow_mut().clear();
            self.raster_busy.set(false);
        }
        let structure = {
            let st = self.inner.borrow();
            PageStructure::from_layout(st.layout_mode, &st.layout)
        };
        if self.rendered_structure.borrow().as_ref() == Some(&structure) {
            self.resize_page_widgets();
        } else {
            self.rebuild_pages();
        }
        self.update_chrome();
        self.schedule_view_restore(restore);
        self.ensure_visible_rasters();
    }

    fn resize_page_widgets(&self) {
        let pages = self.inner.borrow().layout.pages.clone();
        let widgets = self.page_widgets.borrow();
        for geom in pages {
            if let Some((_, da)) = widgets.iter().find(|(i, _)| *i == geom.index) {
                let pw = geom.w.ceil().max(1.0) as i32;
                let ph = geom.h.ceil().max(1.0) as i32;
                da.set_content_width(pw);
                da.set_content_height(ph);
                da.set_size_request(pw, ph);
                da.queue_draw();
            }
        }
    }

    fn scroll_to_current(&self) {
        let y = {
            let st = self.inner.borrow();
            page_scroll_y(&st.layout.pages, st.page)
        };
        let v = self.scrolled.vadjustment();
        let max = (v.upper() - v.page_size()).max(0.0);
        v.set_value(y.min(max));
    }

    fn make_page_da(&self, geom: crate::view::PageGeom, h: &Handle) -> DrawingArea {
        let da = DrawingArea::new();
        let pw = geom.w.ceil().max(1.0) as i32;
        let ph = geom.h.ceil().max(1.0) as i32;
        da.set_content_width(pw);
        da.set_content_height(ph);
        da.set_size_request(pw, ph);
        da.set_halign(gtk::Align::Center);
        da.set_hexpand(false);
        da.add_css_class("lector-page");
        let index = geom.index;
        da.set_draw_func({
            let h = h.clone();
            move |da, cr, width, height| {
                if let Some(ui) = h.get() {
                    ui.draw_page(cr, index, da.scale_factor() as f64);
                } else if width > 0 && height > 0 {
                    cr.set_source_rgb(1.0, 1.0, 1.0);
                    let _ = cr.paint();
                }
            }
        });
        da
    }

    fn rebuild_pages(&self) {
        while let Some(child) = self.pages_box.first_child() {
            self.pages_box.remove(&child);
        }
        self.page_widgets.borrow_mut().clear();
        let pages = self.inner.borrow().layout.pages.clone();
        let mode = self.inner.borrow().layout_mode;
        *self.rendered_structure.borrow_mut() = Some(PageStructure::new(
            mode,
            pages.iter().map(|page| page.index).collect(),
        ));
        let h = Handle(self.me.borrow().clone());
        if pages.is_empty() {
            let lab = Label::new(Some("Open a document — Ctrl+O"));
            lab.set_margin_top(48);
            lab.set_margin_start(48);
            self.pages_box.append(&lab);
            return;
        }
        if mode == LayoutMode::Dual {
            let row = GtkBox::new(Orientation::Horizontal, 16);
            row.set_halign(gtk::Align::Center);
            for geom in pages {
                let da = self.make_page_da(geom, &h);
                self.page_widgets
                    .borrow_mut()
                    .push((geom.index, da.clone()));
                row.append(&da);
            }
            self.pages_box.append(&row);
        } else {
            for geom in pages {
                let da = self.make_page_da(geom, &h);
                self.page_widgets
                    .borrow_mut()
                    .push((geom.index, da.clone()));
                self.pages_box.append(&da);
            }
        }
        self.ensure_visible_rasters();
    }

    fn goto_page(&self, page: i32) {
        let mode = {
            let mut st = self.inner.borrow_mut();
            let n = st.doc.as_ref().map(|d| d.n_pages).unwrap_or(1);
            st.page = page.clamp(0, (n - 1).max(0));
            let remember = st
                .doc
                .as_ref()
                .map(|doc| (doc.path.clone(), st.page, st.zoom));
            if let Some((path, page, zoom)) = remember {
                remember_document_state(&mut st.session, &path, page, zoom);
            }
            st.layout_mode
        };
        if mode != LayoutMode::Continuous {
            self.reflow_restore(ViewRestore::CurrentPage);
            return;
        }
        self.update_chrome();
        self.scroll_to_current();
        self.ensure_visible_rasters();
    }

    fn goto_page_rel(&self, delta: i32) {
        let page = self.inner.borrow().page + delta;
        self.goto_page(page);
    }

    fn set_zoom(&self, zoom: Zoom) {
        self.set_zoom_at(zoom, None);
    }

    fn set_zoom_at(&self, zoom: Zoom, focus: Option<(f64, f64)>) {
        let anchor = self.capture_anchor(focus);
        self.inner.borrow_mut().zoom = zoom;
        self.sync_zoom_dropdown();
        self.reflow_restore(ViewRestore::Anchor(anchor));
    }

    fn zoom_by(&self, factor: f64) {
        self.zoom_by_at(factor, None);
    }

    fn zoom_by_at(&self, factor: f64, focus: Option<(f64, f64)>) {
        let current = self.inner.borrow().layout.scale;
        self.set_zoom_at(zoom_from_factor(current, factor), focus);
    }

    fn zoom_from_dropdown(&self, idx: u32) {
        if self.inner.borrow().ignore_zoom_dropdown {
            return;
        }
        let zoom = match idx {
            0 | 1 => Zoom::FitPage,
            2 => Zoom::FitWidth,
            3 => Zoom::FitHeight,
            4 => Zoom::Custom(0.50),
            5 => Zoom::Custom(0.70),
            6 => Zoom::Custom(0.85),
            7 => Zoom::Actual,
            8 => Zoom::Custom(1.25),
            9 => Zoom::Custom(1.50),
            10 => Zoom::Custom(1.75),
            11 => Zoom::Custom(2.00),
            12 => Zoom::Custom(3.00),
            13 => Zoom::Custom(4.00),
            14 => Zoom::Custom(8.00),
            15 => Zoom::Custom(16.0),
            _ => Zoom::FitWidth,
        };
        self.inner.borrow_mut().zoom = zoom;
        self.reflow();
    }

    fn sync_zoom_dropdown(&self) {
        let z = self.inner.borrow().zoom;
        let idx = match z {
            Zoom::FitPage => 1,
            Zoom::FitWidth => 2,
            Zoom::FitHeight => 3,
            Zoom::Actual => 7,
            Zoom::Custom(s) => {
                let pct = (s * 100.0).round() as i32;
                match pct {
                    50 => 4,
                    70 => 5,
                    85 => 6,
                    100 => 7,
                    125 => 8,
                    150 => 9,
                    175 => 10,
                    200 => 11,
                    300 => 12,
                    400 => 13,
                    800 => 14,
                    1600 => 15,
                    _ => {
                        return;
                    }
                }
            }
        };
        self.inner.borrow_mut().ignore_zoom_dropdown = true;
        self.zoom_drop.set_selected(idx);
        self.inner.borrow_mut().ignore_zoom_dropdown = false;
    }

    fn toggle_fit(&self) {
        let z = self.inner.borrow().zoom;
        let next = match z {
            Zoom::FitWidth => Zoom::FitPage,
            Zoom::FitPage => Zoom::Actual,
            _ => Zoom::FitWidth,
        };
        self.set_zoom(next);
    }

    fn set_layout(&self, mode: LayoutMode) {
        let page = {
            let mut st = self.inner.borrow_mut();
            st.layout_mode = mode;
            st.ignore_menu_checks = true;
            st.page
        };
        self.cont_item.set_active(mode == LayoutMode::Continuous);
        self.dual_item.set_active(mode == LayoutMode::Dual);
        self.single_item.set_active(mode == LayoutMode::Single);
        self.inner.borrow_mut().ignore_menu_checks = false;
        self.reflow();
        self.goto_page(page);
    }

    fn set_invert(&self, on: bool) {
        {
            let mut st = self.inner.borrow_mut();
            st.invert = on;
            st.ignore_menu_checks = true;
        }
        self.invert_item.set_active(on);
        self.inner.borrow_mut().ignore_menu_checks = false;
        self.inner.borrow_mut().raster.clear();
        self.rebuild_sidebar();
        self.queue_pages_draw();
        self.ensure_visible_rasters();
    }

    fn rotate(&self, deg: i32) {
        let mut st = self.inner.borrow_mut();
        let Inner {
            rotation,
            selection,
            ..
        } = &mut *st;
        rotate_view_state(rotation, selection, deg);
        drop(st);
        self.reflow();
    }

    fn toggle_fullscreen(&self) {
        if self.window.is_fullscreen() {
            let ended_presentation = {
                let mut st = self.inner.borrow_mut();
                let mut restore = st.pres_restore.take();
                let Inner {
                    presentation,
                    layout_mode,
                    zoom,
                    sidebar,
                    ..
                } = &mut *st;
                let ended = restore_presentation_state(
                    presentation,
                    &mut restore,
                    layout_mode,
                    zoom,
                    sidebar,
                );
                st.pres_restore = restore;
                ended
            };
            self.window.unfullscreen();
            self.toolbar.set_visible(true);
            if ended_presentation {
                self.sync_sidebar();
                let layout = self.inner.borrow().layout_mode;
                self.set_layout(layout);
                self.sync_zoom_dropdown();
            }
        } else {
            self.window.fullscreen();
        }
    }

    fn toggle_presentation(&self) {
        let going = !self.inner.borrow().presentation;
        if going {
            let restore = {
                let st = self.inner.borrow();
                (st.layout_mode, st.zoom, st.sidebar)
            };
            {
                let mut st = self.inner.borrow_mut();
                st.presentation = true;
                st.pres_restore = Some(restore);
                st.layout_mode = LayoutMode::Single;
                st.zoom = Zoom::FitPage;
            }
            self.find_bar.set_reveal_child(false);
            self.sidebar_revealer.set_reveal_child(false);
            self.toolbar.set_visible(false);
            self.window.fullscreen();
        } else {
            {
                let mut st = self.inner.borrow_mut();
                let mut restore = st.pres_restore.take();
                let Inner {
                    presentation,
                    layout_mode,
                    zoom,
                    sidebar,
                    ..
                } = &mut *st;
                restore_presentation_state(presentation, &mut restore, layout_mode, zoom, sidebar);
                st.pres_restore = restore;
            }
            self.toolbar.set_visible(true);
            self.window.unfullscreen();
            self.sync_sidebar();
        }
        let layout = self.inner.borrow().layout_mode;
        self.set_layout(layout);
        self.sync_zoom_dropdown();
    }

    fn toggle_sidebar(&self) {
        let on = self.inner.borrow().sidebar == Sidebar::Hidden;
        self.sidebar_btn.set_active(on);
    }

    fn set_sidebar_visible(&self, on: bool) {
        {
            let mut st = self.inner.borrow_mut();
            if on {
                if st.sidebar == Sidebar::Hidden {
                    st.sidebar = Sidebar::Thumbs;
                }
            } else {
                st.sidebar = Sidebar::Hidden;
            }
        }
        let side = self.inner.borrow().sidebar;
        self.sidebar_revealer
            .set_reveal_child(side != Sidebar::Hidden);
        match side {
            Sidebar::Thumbs => self.stack.set_visible_child_name("thumbs"),
            Sidebar::Outline => self.stack.set_visible_child_name("outline"),
            Sidebar::Hidden => {}
        }
        if on {
            self.rebuild_sidebar();
        }
    }

    fn sync_sidebar(&self) {
        let on = self.inner.borrow().sidebar != Sidebar::Hidden;
        if self.sidebar_btn.is_active() != on {
            self.sidebar_btn.set_active(on);
        } else {
            self.set_sidebar_visible(on);
        }
    }

    fn rebuild_sidebar(&self) {
        while let Some(c) = self.thumbs.first_child() {
            self.thumbs.remove(&c);
        }
        while let Some(c) = self.outline.first_child() {
            self.outline.remove(&c);
        }
        let st = self.inner.borrow();
        let Some(doc) = st.doc.as_ref() else {
            return;
        };
        let n = doc.n_pages;
        let cap = n.min(1000);
        drop(st);
        {
            let mut st = self.inner.borrow_mut();
            st.thumb_gen = st.thumb_gen.wrapping_add(1);
        }
        for i in 0..cap {
            let row = gtk::ListBoxRow::new();
            let col = GtkBox::new(Orientation::Vertical, 4);
            col.set_margin_top(8);
            col.set_margin_bottom(8);
            col.set_margin_start(10);
            col.set_margin_end(10);
            col.set_widget_name("thumb-col");
            let lab = Label::new(Some(&format!("{}", i + 1)));
            lab.add_css_class("dim-label");
            col.append(&lab);
            row.set_child(Some(&col));
            self.thumbs.append(&row);
        }
        if n > cap {
            let row = gtk::ListBoxRow::new();
            let lab = Label::new(Some(&format!("… {n} pages")));
            lab.add_css_class("dim-label");
            lab.set_margin_top(8);
            lab.set_margin_bottom(8);
            row.set_child(Some(&lab));
            row.set_sensitive(false);
            self.thumbs.append(&row);
        }
        let outline = self.inner.borrow().doc.as_ref().map(|d| d.outline.clone());
        if let Some(items) = outline {
            for item in items {
                let row = gtk::ListBoxRow::new();
                let lab = Label::new(Some(&item.title));
                lab.set_xalign(0.0);
                lab.set_ellipsize(gtk::pango::EllipsizeMode::End);
                lab.set_margin_start(10 + item.depth as i32 * 14);
                lab.set_margin_top(6);
                lab.set_margin_bottom(6);
                lab.set_margin_end(8);
                row.set_child(Some(&lab));
                row.set_widget_name(&format!("p{}", item.page.unwrap_or(0)));
                self.outline.append(&row);
            }
        }
        self.schedule_thumbs();
    }

    fn schedule_thumbs(&self) {
        let gen = self.inner.borrow().thumb_gen;
        let h = Handle(self.me.borrow().clone());
        glib::idle_add_local_once(move || {
            if let Some(ui) = h.get() {
                ui.fill_visible_thumbs(gen);
            }
        });
    }

    fn fill_visible_thumbs(&self, gen: u64) {
        if self.inner.borrow().thumb_gen != gen {
            return;
        }
        if self.inner.borrow().sidebar == Sidebar::Hidden {
            return;
        }
        let v = self.thumbs_sw.vadjustment();
        let top = v.value();
        let bot = top + v.page_size().max(1.0);
        let invert = self.inner.borrow().invert;
        let mut filled = 0;
        let mut child = self.thumbs.first_child();
        while let Some(widget) = child {
            child = widget.next_sibling();
            let Ok(row) = widget.downcast::<gtk::ListBoxRow>() else {
                continue;
            };
            let Some(bounds) = row.compute_bounds(&self.thumbs) else {
                continue;
            };
            let y = bounds.y() as f64;
            let h = bounds.height() as f64;
            if y + h < top - 80.0 || y > bot + 80.0 {
                continue;
            }
            let Some(col) = row.child() else {
                continue;
            };
            if col
                .first_child()
                .and_then(|w| w.downcast::<gtk::Picture>().ok())
                .is_some()
            {
                continue;
            }
            let page = row.index();
            let dpr = self.page_dpr();
            let surf = {
                let st = self.inner.borrow();
                let Some(doc) = st.doc.as_ref() else {
                    return;
                };
                thumbnail(doc, page, THUMB_CSS_WIDTH, dpr, invert)
            };
            if let Some(surf) = surf {
                if let Some(tex) = surface_texture(&surf) {
                    let pic = gtk::Picture::for_paintable(&tex);
                    pic.set_can_shrink(true);
                    pic.set_content_fit(gtk::ContentFit::Contain);
                    let (cw, ch) = thumbnail_css_size(
                        surf.width() as f64 / dpr.max(1.0),
                        surf.height() as f64 / dpr.max(1.0),
                        THUMB_CSS_WIDTH,
                    );
                    pic.set_size_request(cw, ch);
                    if let Some(boxw) = col.downcast_ref::<GtkBox>() {
                        boxw.prepend(&pic);
                    }
                    filled += 1;
                    if filled >= 2 {
                        let h = Handle(self.me.borrow().clone());
                        glib::idle_add_local_once(move || {
                            if let Some(ui) = h.get() {
                                ui.fill_visible_thumbs(gen);
                            }
                        });
                        return;
                    }
                }
            }
        }
    }

    fn search(&self, query: &str) {
        let q = query.trim().to_string();
        let gen = {
            let mut st = self.inner.borrow_mut();
            st.search_hits.clear();
            st.search_idx = None;
            st.search_query = q.clone();
            st.search_gen = st.search_gen.wrapping_add(1);
            st.search_gen
        };
        if q.is_empty() {
            self.search_info.set_text("");
            self.queue_pages_draw();
            return;
        }
        let start = self.inner.borrow().page;
        {
            let mut st = self.inner.borrow_mut();
            if let Some(doc) = st.doc.as_ref() {
                st.search_hits = doc.find_page(start, &q);
                if !st.search_hits.is_empty() {
                    st.search_idx = Some(0);
                }
            }
        }
        self.jump_search();
        self.search_info.set_text("Searching…");
        let n_pages = self
            .inner
            .borrow()
            .doc
            .as_ref()
            .map(|doc| doc.n_pages)
            .unwrap_or(0);
        if let Some(job) = SearchJob::after_first_page(n_pages, start) {
            self.continue_search(gen, job);
        } else {
            self.finish_search();
        }
    }

    fn continue_search(&self, gen: u64, job: SearchJob) {
        let h = Handle(self.me.borrow().clone());
        glib::idle_add_local_once(move || {
            let Some(ui) = h.get() else { return };
            if ui.inner.borrow().search_gen != gen {
                return;
            }
            let n_pages = ui
                .inner
                .borrow()
                .doc
                .as_ref()
                .map(|d| d.n_pages)
                .unwrap_or(0);
            let mut job = job;
            let Some(page) = job.take_page(n_pages) else {
                ui.finish_search();
                return;
            };
            let q = ui.inner.borrow().search_query.clone();
            let extra = ui
                .inner
                .borrow()
                .doc
                .as_ref()
                .map(|d| d.find_page(page, &q))
                .unwrap_or_default();
            if !extra.is_empty() {
                let mut st = ui.inner.borrow_mut();
                st.search_hits.extend(extra);
                if st.search_idx.is_none() {
                    st.search_idx = Some(0);
                    drop(st);
                    ui.jump_search();
                } else {
                    let i = st.search_idx.unwrap_or(0);
                    let n_hits = st.search_hits.len();
                    drop(st);
                    ui.search_info.set_text(&format!("{} of {}", i + 1, n_hits));
                    ui.queue_pages_draw();
                }
            }
            if job.remaining > 0 {
                ui.continue_search(gen, job);
            } else {
                ui.finish_search();
            }
        });
    }

    fn finish_search(&self) {
        let (n_hits, search_idx) = {
            let st = self.inner.borrow();
            (st.search_hits.len(), st.search_idx)
        };
        if n_hits == 0 {
            self.search_info.set_text("Not found");
        } else if search_idx.is_none() {
            self.inner.borrow_mut().search_idx = Some(0);
            self.jump_search();
        } else {
            let index = search_idx.unwrap_or(0);
            self.search_info
                .set_text(&format!("{} of {}", index + 1, n_hits));
            self.queue_pages_draw();
        }
    }

    fn search_step(&self, dir: i32) {
        let mut st = self.inner.borrow_mut();
        let n = st.search_hits.len() as i32;
        if n == 0 {
            return;
        }
        let cur = st.search_idx.unwrap_or(0) as i32;
        st.search_idx = Some((cur + dir).rem_euclid(n) as usize);
        drop(st);
        self.jump_search();
    }

    fn jump_search(&self) {
        let (page, n, i) = {
            let st = self.inner.borrow();
            let n = st.search_hits.len();
            match st.search_idx {
                Some(i) if i < n => (st.search_hits[i].0, n, i),
                _ => {
                    self.search_info
                        .set_text(if n == 0 { "Not found" } else { "" });
                    self.queue_pages_draw();
                    return;
                }
            }
        };
        self.search_info.set_text(&format!("{} of {}", i + 1, n));
        self.goto_page(page);
        self.queue_pages_draw();
    }

    fn hide_search(&self) {
        self.find_bar.set_reveal_child(false);
        {
            let mut st = self.inner.borrow_mut();
            st.search_hits.clear();
            st.search_idx = None;
            st.search_gen = st.search_gen.wrapping_add(1);
        }
        self.queue_pages_draw();
        self.pages_box.grab_focus();
    }

    fn show_search(&self) {
        self.find_bar.set_reveal_child(true);
        self.search_entry.grab_focus();
    }

    fn link_at_view(&self, x: f64, y: f64) -> Option<DocumentLink> {
        let (page, display_x, display_y) = self.hit_at(x, y)?;
        let st = self.inner.borrow();
        let doc = st.doc.as_ref()?;
        let (doc_x, doc_y) =
            display_point_to_document(doc, page, st.rotation, display_x, display_y);
        doc.link_at(page, doc_x, doc_y)
    }

    fn activate_link_at(&self, x: f64, y: f64) -> bool {
        if let Some(link) = self.link_at_view(x, y) {
            self.follow_link(link);
            true
        } else {
            false
        }
    }

    fn follow_link(&self, link: DocumentLink) {
        match link {
            DocumentLink::GotoPage(page) => self.goto_page(page),
            DocumentLink::Uri(uri) => {
                if !external_uri_allowed(&uri) {
                    self.alert(
                        "Link blocked",
                        "Only web (HTTP/HTTPS) and email links can be opened.",
                    );
                    return;
                }
                if let Err(error) =
                    gio::AppInfo::launch_default_for_uri(&uri, None::<&gio::AppLaunchContext>)
                {
                    self.alert("Could not open link", &error.to_string());
                }
            }
        }
    }

    fn drag_begin(&self, x: f64, y: f64) {
        let hit = self.hit_at(x, y);
        {
            let st = &mut *self.inner.borrow_mut();
            begin_selection_state(&mut st.selection, &mut st.targeted_annotation, hit);
        }
        self.queue_pages_draw();
        self.update_annotation_actions();
    }

    fn drag_update(&self, x: f64, y: f64) {
        {
            let hit = self.hit_at(x, y);
            let st = &mut *self.inner.borrow_mut();
            let Some(sel) = st.selection.as_mut() else {
                return;
            };
            if let Some((page, px, py)) = hit {
                if page == sel.page {
                    sel.x2 = px;
                    sel.y2 = py;
                }
            }
        }
        self.queue_pages_draw();
        self.update_annotation_actions();
    }

    fn copy_selection(&self) {
        let st = self.inner.borrow();
        let (Some(doc), Some(sel)) = (st.doc.as_ref(), st.selection) else {
            return;
        };
        if let Some(text) = selected_text(doc, st.rotation, sel) {
            if let Some(display) = gdk::Display::default() {
                display.clipboard().set_text(&text);
            }
        }
    }

    fn selected_annotation_region(&self) -> Option<(PathBuf, i32, DocumentRect)> {
        let st = self.inner.borrow();
        let doc = st.doc.as_ref()?;
        let selection = st.selection?;
        selected_text(doc, st.rotation, selection)?;
        let (page_width, page_height) = doc.size(selection.page, 0);
        let rect = display_rect_to_document(
            selection.x1,
            selection.y1,
            selection.x2,
            selection.y2,
            page_width,
            page_height,
            st.rotation,
        )?;
        Some((doc.path.clone(), selection.page, rect))
    }

    fn update_annotation_actions(&self) {
        let can_create = self.selected_annotation_region().is_some()
            && self.inner.borrow().annotations.is_some();
        self.highlight_item.set_sensitive(can_create);
        self.note_item.set_sensitive(can_create);
        self.delete_annotation_item
            .set_sensitive(self.annotation_to_delete().is_some());
    }

    fn annotation_to_delete(&self) -> Option<String> {
        let selected = self.selected_annotation_region();
        let st = self.inner.borrow();
        let doc = st.doc.as_ref()?;
        let annotations = st.annotations.as_ref()?.for_document(&doc.path);
        if let Some(id) = st.targeted_annotation.as_ref() {
            if annotations.iter().any(|annotation| &annotation.id == id) {
                return Some(id.clone());
            }
        }
        let (_, page, rect) = selected?;
        annotations
            .iter()
            .rev()
            .find(|annotation| {
                annotation.page == page && document_rects_intersect(annotation.rect, rect)
            })
            .map(|annotation| annotation.id.clone())
    }

    fn annotation_store_error(&self) -> String {
        self.inner
            .borrow()
            .annotation_error
            .clone()
            .unwrap_or_else(|| "The annotation store is unavailable.".into())
    }

    fn create_annotation(&self, kind: AnnotationKind) {
        let Some((path, page, rect)) = self.selected_annotation_region() else {
            return;
        };
        let annotation = new_annotation(page, rect, kind);
        let result = {
            let st = self.inner.borrow();
            st.annotations.as_ref().map(|store| {
                let mut candidate = store.clone();
                candidate.add(&path, annotation.clone());
                candidate.save().map(|_| candidate)
            })
        };
        match result {
            Some(Ok(candidate)) => {
                let mut st = self.inner.borrow_mut();
                st.annotations = Some(candidate);
                st.targeted_annotation = Some(annotation.id);
                drop(st);
                self.queue_pages_draw();
                self.update_annotation_actions();
            }
            Some(Err(error)) => self.alert("Could not save annotation", &error),
            None => self.alert("Annotations unavailable", &self.annotation_store_error()),
        }
    }

    fn add_highlight(&self) {
        self.create_annotation(AnnotationKind::Highlight);
    }

    fn add_note(&self) {
        if self.selected_annotation_region().is_none() {
            return;
        }
        if self.inner.borrow().annotations.is_none() {
            self.alert("Annotations unavailable", &self.annotation_store_error());
            return;
        }
        let text = gtk::TextView::new();
        text.set_wrap_mode(gtk::WrapMode::WordChar);
        text.set_size_request(420, 130);
        text.set_tooltip_text(Some("Text attached to the selected PDF region"));
        let save = Button::with_label("Add Note");
        save.add_css_class("suggested-action");
        save.set_sensitive(false);
        let cancel = Button::with_label("Cancel");
        let actions = GtkBox::new(Orientation::Horizontal, 8);
        actions.set_halign(gtk::Align::End);
        actions.append(&cancel);
        actions.append(&save);
        let body = GtkBox::new(Orientation::Vertical, 10);
        body.append(&text);
        body.append(&actions);
        let dialog = self.themed_dialog("Add Note", &body);
        text.buffer().connect_changed({
            let save = save.clone();
            move |buffer| {
                let value = buffer.text(&buffer.start_iter(), &buffer.end_iter(), true);
                save.set_sensitive(!value.trim().is_empty());
            }
        });
        cancel.connect_clicked({
            let dialog = dialog.clone();
            move |_| dialog.close()
        });
        save.connect_clicked({
            let dialog = dialog.clone();
            let text = text.clone();
            let h = Handle(self.me.borrow().clone());
            move |_| {
                let buffer = text.buffer();
                let value = buffer
                    .text(&buffer.start_iter(), &buffer.end_iter(), true)
                    .trim()
                    .to_string();
                if value.is_empty() {
                    return;
                }
                dialog.close();
                if let Some(ui) = h.get() {
                    ui.create_annotation(AnnotationKind::Note { text: value });
                }
            }
        });
        dialog.present();
        text.grab_focus();
    }

    fn delete_targeted_annotation(&self) {
        let Some(id) = self.annotation_to_delete() else {
            return;
        };
        let path = {
            let st = self.inner.borrow();
            let Some(doc) = st.doc.as_ref() else { return };
            doc.path.clone()
        };
        let result = {
            let st = self.inner.borrow();
            st.annotations.as_ref().map(|store| {
                let mut candidate = store.clone();
                if !candidate.remove(&path, &id) {
                    return Ok(candidate);
                }
                candidate.save().map(|_| candidate)
            })
        };
        match result {
            Some(Ok(candidate)) => {
                let mut st = self.inner.borrow_mut();
                st.annotations = Some(candidate);
                if st.targeted_annotation.as_deref() == Some(id.as_str()) {
                    st.targeted_annotation = None;
                }
                drop(st);
                self.queue_pages_draw();
                self.update_annotation_actions();
            }
            Some(Err(error)) => self.alert("Could not delete annotation", &error),
            None => self.alert("Annotations unavailable", &self.annotation_store_error()),
        }
    }

    fn annotation_at_view(&self, x: f64, y: f64, marker_only: bool) -> Option<Annotation> {
        let (page, display_x, display_y) = self.hit_at(x, y)?;
        let st = self.inner.borrow();
        let doc = st.doc.as_ref()?;
        let (page_width, page_height) = doc.size(page, 0);
        let marker_radius = 11.0 / st.layout.scale.max(0.01);
        st.annotations
            .as_ref()?
            .for_document(&doc.path)
            .iter()
            .rev()
            .filter(|annotation| annotation.page == page)
            .find(|annotation| {
                let bounds =
                    document_rect_to_display(annotation.rect, page_width, page_height, st.rotation);
                if matches!(annotation.kind, AnnotationKind::Note { .. }) {
                    let dx = display_x - (bounds.x + bounds.width);
                    let dy = display_y - bounds.y;
                    if dx * dx + dy * dy <= marker_radius * marker_radius {
                        return true;
                    }
                }
                !marker_only && document_rect_contains(bounds, display_x, display_y)
            })
            .cloned()
    }

    fn activate_note_at(&self, x: f64, y: f64) -> bool {
        let Some(annotation) = self.annotation_at_view(x, y, true) else {
            return false;
        };
        let AnnotationKind::Note { text } = annotation.kind else {
            return false;
        };
        self.inner.borrow_mut().targeted_annotation = Some(annotation.id);
        self.queue_pages_draw();
        let label = Label::new(Some(&text));
        label.set_wrap(true);
        label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        label.set_selectable(true);
        label.set_xalign(0.0);
        label.set_max_width_chars(60);
        self.themed_dialog("Annotation Note", &label).present();
        true
    }

    fn target_annotation_at(&self, x: f64, y: f64) -> bool {
        let replacement = self
            .annotation_at_view(x, y, false)
            .map(|annotation| annotation.id);
        let found = replacement.is_some();
        if replace_annotation_target(
            &mut self.inner.borrow_mut().targeted_annotation,
            replacement,
        ) {
            self.queue_pages_draw();
        }
        self.update_annotation_actions();
        found
    }

    fn print_doc(&self) {
        let st = self.inner.borrow();
        let Some(doc) = st.doc.as_ref() else {
            return;
        };
        let n = doc.n_pages;
        let document = doc.document.clone();
        drop(st);
        let op = gtk::PrintOperation::new();
        op.set_n_pages(n);
        op.connect_draw_page(move |_, ctx, nr| {
            let Some(page) = document.page(nr) else {
                return;
            };
            let cr = ctx.cairo_context();
            let (pw, ph) = page.size();
            let scale = (ctx.width() / pw).min(ctx.height() / ph);
            cr.scale(scale, scale);
            page.render_for_printing(&cr);
        });
        match op.run(gtk::PrintOperationAction::PrintDialog, Some(&self.window)) {
            Ok(gtk::PrintOperationResult::Error) => {
                self.alert("Print failed", "The print operation reported an error.");
            }
            Err(e) => self.alert("Print failed", &e.to_string()),
            Ok(_) => {}
        }
    }

    fn save_copy(&self) {
        let st = self.inner.borrow();
        let Some(doc) = st.doc.as_ref() else {
            return;
        };
        let document = doc.document.clone();
        let suggest = doc
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned());
        drop(st);
        let dialog = gtk::FileDialog::builder().title("Save a Copy").build();
        if let Some(name) = suggest {
            dialog.set_initial_name(Some(&name));
        }
        let h = Handle(self.me.borrow().clone());
        dialog.save(
            Some(&self.window),
            gio::Cancellable::NONE,
            move |res| match res {
                Ok(file) => {
                    if let Some(path) = file.path() {
                        let uri = crate::theme::path_uri(&path);
                        if let Err(e) = document.save_a_copy(&uri) {
                            if let Some(ui) = h.get() {
                                ui.alert("Could not save a copy", &e.to_string());
                            }
                        }
                    }
                }
                Err(e) => {
                    if !e.matches(gtk::DialogError::Cancelled)
                        && !e.matches(gtk::DialogError::Dismissed)
                    {
                        if let Some(ui) = h.get() {
                            ui.alert("Could not save a copy", &e.to_string());
                        }
                    }
                }
            },
        );
    }

    fn show_properties(&self) {
        if let Some(dialog) = self.properties_dialog() {
            dialog.present();
        }
    }

    fn show_reading_history(&self) {
        self.reading_history_dialog().present();
    }

    fn show_shortcuts(&self) {
        self.shortcuts_dialog().present();
    }

    fn show_about(&self) {
        self.about_dialog().present();
    }

    fn themed_dialog(&self, title: &str, body: &impl IsA<gtk::Widget>) -> gtk::Window {
        let win = gtk::Window::builder()
            .transient_for(&self.window)
            .modal(true)
            .title(title)
            .resizable(false)
            .default_width(460)
            .build();
        win.add_css_class("lector-window");
        win.add_css_class("lector-dialog");
        win.set_titlebar(None::<&gtk::Box>);

        let title_label = Label::new(Some(title));
        title_label.add_css_class("lector-dialog-title");
        title_label.set_hexpand(true);
        title_label.set_halign(gtk::Align::Start);
        title_label.set_xalign(0.0);

        let close = icon_pill("window-close-symbolic", "Close");
        let header = GtkBox::new(Orientation::Horizontal, 2);
        header.add_css_class("lector-toolbar");
        header.append(&title_label);
        header.append(&close);

        let body_wrap = GtkBox::new(Orientation::Vertical, 0);
        body_wrap.add_css_class("lector-dialog-body");
        body_wrap.append(body);

        let root = GtkBox::new(Orientation::Vertical, 0);
        root.append(&header);
        root.append(&body_wrap);
        win.set_child(Some(&root));

        let win_close = win.clone();
        close.connect_clicked(move |_| {
            win_close.close();
        });
        let keys = gtk::EventControllerKey::new();
        let win_esc = win.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == Key::Escape {
                win_esc.close();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        win.add_controller(keys);
        win
    }

    fn shortcuts_dialog(&self) -> gtk::Window {
        let left = GtkBox::new(Orientation::Vertical, 2);
        let right = GtkBox::new(Orientation::Vertical, 2);
        left.set_hexpand(true);
        right.set_hexpand(true);
        for section in shortcut_sections() {
            let col = match section.heading {
                "VIEW" | "NAVIGATION" => &right,
                _ => &left,
            };
            col.append(&menu_heading(section.heading));
            for row in section.rows {
                col.append(&shortcut_row(row.action, row.keys));
            }
        }
        let columns = GtkBox::new(Orientation::Horizontal, 28);
        columns.append(&left);
        columns.append(&right);
        let scrolled = ScrolledWindow::new();
        scrolled.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scrolled.set_max_content_height(520);
        scrolled.set_propagate_natural_height(true);
        scrolled.set_child(Some(&columns));
        self.themed_dialog("Keyboard shortcuts", &scrolled)
    }

    fn reading_history_dialog(&self) -> gtk::Window {
        let entries = self.inner.borrow().session.history(20);
        let list = ListBox::new();
        list.add_css_class("lector-history");
        list.set_selection_mode(gtk::SelectionMode::Single);
        if entries.is_empty() {
            let empty = Label::new(Some("No reading history yet"));
            empty.add_css_class("dim-label");
            empty.set_margin_top(24);
            empty.set_margin_bottom(24);
            list.append(&empty);
        } else {
            for entry in &entries {
                let row = gtk::ListBoxRow::new();
                let text = GtkBox::new(Orientation::Vertical, 2);
                text.set_hexpand(true);
                let name = Label::new(Some(&entry.basename));
                name.add_css_class("lector-menu-label");
                name.set_halign(gtk::Align::Start);
                name.set_xalign(0.0);
                let path = Label::new(Some(&entry.path.display().to_string()));
                path.add_css_class("lector-history-path");
                path.set_halign(gtk::Align::Start);
                path.set_xalign(0.0);
                path.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
                text.append(&name);
                text.append(&path);
                let status_text = if entry.exists {
                    format!("Page {}", entry.page)
                } else {
                    "Missing".into()
                };
                let status = Label::new(Some(&status_text));
                status.add_css_class(if entry.exists {
                    "lector-history-page"
                } else {
                    "lector-history-missing"
                });
                let content = GtkBox::new(Orientation::Horizontal, 12);
                content.set_margin_top(7);
                content.set_margin_bottom(7);
                content.set_margin_start(4);
                content.set_margin_end(4);
                content.append(&text);
                content.append(&status);
                row.set_child(Some(&content));
                row.set_sensitive(entry.exists);
                list.append(&row);
            }
        }

        let clear = Button::with_label("Clear History…");
        clear.set_tooltip_text(Some("Requires confirmation; annotations are kept"));
        clear.set_sensitive(!entries.is_empty());
        clear.set_halign(gtk::Align::End);
        let body = GtkBox::new(Orientation::Vertical, 10);
        let scrolled = ScrolledWindow::new();
        scrolled.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scrolled.set_min_content_height(160);
        scrolled.set_max_content_height(520);
        scrolled.set_propagate_natural_height(true);
        scrolled.set_child(Some(&list));
        body.append(&scrolled);
        body.append(&clear);
        let dialog = self.themed_dialog("Reading History", &body);
        dialog.set_default_width(620);

        let paths = Rc::new(
            entries
                .iter()
                .map(|entry| entry.path.clone())
                .collect::<Vec<_>>(),
        );
        list.connect_row_activated({
            let paths = paths.clone();
            let dialog = dialog.clone();
            let h = Handle(self.me.borrow().clone());
            move |_, row| {
                let Some(path) = paths.get(row.index() as usize) else {
                    return;
                };
                dialog.close();
                if let Some(ui) = h.get() {
                    ui.open_path(path, None);
                }
            }
        });
        clear.connect_clicked({
            let dialog = dialog.clone();
            let h = Handle(self.me.borrow().clone());
            move |_| {
                let confirmation = gtk::AlertDialog::builder()
                    .modal(true)
                    .message("Clear reading history?")
                    .detail("This removes remembered documents and page positions. Annotations are not deleted.")
                    .buttons(["Cancel", "Clear History"])
                    .cancel_button(0)
                    .default_button(0)
                    .build();
                let dialog_after = dialog.clone();
                let h_after = h.clone();
                confirmation.choose(Some(&dialog), gio::Cancellable::NONE, move |result| {
                    if result != Ok(1) {
                        return;
                    }
                    let Some(ui) = h_after.get() else { return };
                    let result = {
                        let st = ui.inner.borrow();
                        let mut candidate = st.session.clone();
                        candidate.clear_history_for_open_document(
                            st.doc.as_ref().map(|document| document.path.as_path()),
                        );
                        candidate.save().map(|_| candidate)
                    };
                    match result {
                        Ok(session) => {
                            ui.inner.borrow_mut().session = session;
                            dialog_after.close();
                        }
                        Err(error) => ui.alert("Could not clear history", &error),
                    }
                });
            }
        });
        dialog
    }

    fn about_dialog(&self) -> gtk::Window {
        let about = about_copy();
        let col = GtkBox::new(Orientation::Vertical, 6);
        col.set_halign(gtk::Align::Center);
        col.set_valign(gtk::Align::Center);
        let icon = Image::from_icon_name("org.omarchy.Lector");
        icon.set_pixel_size(64);
        let name = Label::new(Some(about.name));
        name.add_css_class("lector-about-name");
        let tagline = Label::new(Some(about.tagline));
        tagline.add_css_class("lector-about-copy");
        let detail = Label::new(Some(about.detail));
        detail.add_css_class("lector-about-copy");
        let version = Label::new(Some(about.version));
        version.add_css_class("lector-about-version");
        col.append(&icon);
        col.append(&name);
        col.append(&tagline);
        col.append(&detail);
        col.append(&version);
        self.themed_dialog("About Lector", &col)
    }

    fn properties_dialog(&self) -> Option<gtk::Window> {
        let st = self.inner.borrow();
        let doc = st.doc.as_ref()?;
        let fields = property_fields(doc);
        drop(st);
        let list = GtkBox::new(Orientation::Vertical, 8);
        for (key, value) in fields {
            let row = GtkBox::new(Orientation::Horizontal, 16);
            let label = Label::new(Some(key));
            label.add_css_class("lector-field-label");
            label.set_xalign(0.0);
            label.set_width_chars(9);
            label.set_halign(gtk::Align::Start);
            let val = Label::new(Some(&value));
            val.add_css_class("lector-field-value");
            val.set_hexpand(true);
            val.set_xalign(0.0);
            val.set_halign(gtk::Align::Start);
            val.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            val.set_selectable(true);
            row.append(&label);
            row.append(&val);
            list.append(&row);
        }
        Some(self.themed_dialog("Properties", &list))
    }

    fn reload(&self) {
        let (path, password) = {
            let st = self.inner.borrow();
            match st.doc.as_ref() {
                Some(d) => (Some(d.path.clone()), d.password.clone()),
                None => (None, None),
            }
        };
        if let Some(p) = path {
            self.open_path(&p, password.as_deref());
        }
    }

    fn persist(&self) {
        let result = {
            let mut st = self.inner.borrow_mut();
            let remember = st.doc.as_ref().map(|d| (d.path.clone(), st.page, st.zoom));
            if let Some((path, page, zoom)) = remember {
                remember_document_state(&mut st.session, &path, page, zoom);
            }
            st.session.invert = st.invert;
            st.session.layout = match st.layout_mode {
                LayoutMode::Single => "single".into(),
                LayoutMode::Dual => "dual".into(),
                LayoutMode::Continuous => "continuous".into(),
            };
            st.session.sidebar = match st.sidebar {
                Sidebar::Hidden => "hidden".into(),
                Sidebar::Thumbs => "thumbs".into(),
                Sidebar::Outline => "outline".into(),
            };
            st.session.save()
        };
        if let Err(error) = result {
            self.alert("Could not save session", &error);
        }
    }

    fn reload_theme(&self) {
        let theme = Theme::load();
        theme::apply_css(&self.css, &theme);
        self.inner.borrow_mut().theme = theme;
        self.queue_pages_draw();
        self.update_chrome();
    }

    fn update_chrome(&self) {
        let st = self.inner.borrow();
        let has_doc = st.doc.is_some();
        self.prev_btn.set_sensitive(has_doc);
        self.next_btn.set_sensitive(has_doc);
        self.page_entry.set_sensitive(has_doc);
        self.zoom_out_btn.set_sensitive(has_doc);
        self.zoom_in_btn.set_sensitive(has_doc);
        self.zoom_drop.set_sensitive(has_doc);
        self.find_btn.set_sensitive(has_doc);
        if let Some(doc) = st.doc.as_ref() {
            self.window.set_title(Some(&doc.title));
            self.title_label.set_text(&doc.title);
            self.page_total.set_text(&format!("/ {}", doc.n_pages));
            self.page_entry.set_text(&format!("{}", st.page + 1));
        } else {
            self.window.set_title(Some(""));
            self.title_label.set_text("Open a document");
            self.page_total.set_text("/ 0");
            self.page_entry.set_text("");
        }
        drop(st);
        self.update_annotation_actions();
    }

    fn key(&self, key: Key, mods: gdk::ModifierType) -> glib::Propagation {
        let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
        let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);

        if self.search_entry.has_focus() && !ctrl {
            if key == Key::Escape {
                self.hide_search();
                return glib::Propagation::Stop;
            }
            return glib::Propagation::Proceed;
        }
        if self.page_entry.has_focus() && !ctrl {
            if key == Key::Escape {
                self.pages_box.grab_focus();
                return glib::Propagation::Stop;
            }
            return glib::Propagation::Proceed;
        }

        if let Some(shortcut) = annotation_shortcut(key, mods) {
            match shortcut {
                AnnotationShortcut::History => self.show_reading_history(),
                AnnotationShortcut::Highlight => self.add_highlight(),
                AnnotationShortcut::Note => self.add_note(),
            }
            return glib::Propagation::Stop;
        }

        if ctrl {
            match key {
                Key::o => {
                    self.open_dialog();
                    return glib::Propagation::Stop;
                }
                Key::s => {
                    self.save_copy();
                    return glib::Propagation::Stop;
                }
                Key::p => {
                    self.print_doc();
                    return glib::Propagation::Stop;
                }
                Key::f => {
                    self.show_search();
                    return glib::Propagation::Stop;
                }
                Key::g => {
                    self.search_step(if shift { -1 } else { 1 });
                    return glib::Propagation::Stop;
                }
                Key::c => {
                    self.copy_selection();
                    return glib::Propagation::Stop;
                }
                Key::w | Key::q => {
                    self.persist();
                    self.window.close();
                    return glib::Propagation::Stop;
                }
                Key::r => {
                    if shift {
                        self.reload();
                    } else {
                        self.rotate(90);
                    }
                    return glib::Propagation::Stop;
                }
                Key::l => {
                    self.rotate(-90);
                    return glib::Propagation::Stop;
                }
                Key::i => {
                    let on = !self.inner.borrow().invert;
                    self.set_invert(on);
                    return glib::Propagation::Stop;
                }
                Key::plus | Key::equal | Key::KP_Add => {
                    self.zoom_by(1.15);
                    return glib::Propagation::Stop;
                }
                Key::minus | Key::KP_Subtract => {
                    self.zoom_by(1.0 / 1.15);
                    return glib::Propagation::Stop;
                }
                Key::_0 | Key::KP_0 => {
                    self.set_zoom(Zoom::Actual);
                    return glib::Propagation::Stop;
                }
                Key::Home => {
                    self.goto_page(0);
                    return glib::Propagation::Stop;
                }
                Key::End => {
                    let n = self
                        .inner
                        .borrow()
                        .doc
                        .as_ref()
                        .map(|d| d.n_pages)
                        .unwrap_or(1);
                    self.goto_page(n - 1);
                    return glib::Propagation::Stop;
                }
                Key::Page_Down => {
                    self.goto_page_rel(1);
                    return glib::Propagation::Stop;
                }
                Key::Page_Up => {
                    self.goto_page_rel(-1);
                    return glib::Propagation::Stop;
                }
                _ => {}
            }
        }

        match key {
            Key::Escape => {
                if self.inner.borrow().presentation {
                    self.toggle_presentation();
                } else if self.find_bar.reveals_child() {
                    self.hide_search();
                } else if self.window.is_fullscreen() {
                    self.toggle_fullscreen();
                }
                glib::Propagation::Stop
            }
            Key::F11 => {
                self.toggle_fullscreen();
                glib::Propagation::Stop
            }
            Key::F5 => {
                self.toggle_presentation();
                glib::Propagation::Stop
            }
            Key::F9 => {
                self.toggle_sidebar();
                glib::Propagation::Stop
            }
            Key::Delete => {
                self.delete_targeted_annotation();
                glib::Propagation::Stop
            }
            Key::slash => {
                self.show_search();
                glib::Propagation::Stop
            }
            Key::w => {
                self.set_zoom(Zoom::FitWidth);
                glib::Propagation::Stop
            }
            Key::a => {
                self.set_zoom(Zoom::FitPage);
                glib::Propagation::Stop
            }
            Key::c => {
                self.set_layout(LayoutMode::Continuous);
                glib::Propagation::Stop
            }
            Key::d => {
                self.set_layout(LayoutMode::Dual);
                glib::Propagation::Stop
            }
            Key::s => {
                self.set_layout(LayoutMode::Single);
                glib::Propagation::Stop
            }
            Key::n | Key::Page_Down | Key::space | Key::Right => {
                if self.inner.borrow().layout_mode == LayoutMode::Continuous && key == Key::space {
                    let v = self.scrolled.vadjustment();
                    v.set_value((v.value() + v.page_size() * 0.9).min(v.upper() - v.page_size()));
                } else {
                    self.goto_page_rel(1);
                }
                glib::Propagation::Stop
            }
            Key::p | Key::Page_Up | Key::BackSpace | Key::Left => {
                self.goto_page_rel(-1);
                glib::Propagation::Stop
            }
            Key::Home => {
                self.goto_page(0);
                glib::Propagation::Stop
            }
            Key::End => {
                let n = self
                    .inner
                    .borrow()
                    .doc
                    .as_ref()
                    .map(|d| d.n_pages)
                    .unwrap_or(1);
                self.goto_page(n - 1);
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    }
}

fn document_rect_contains(rect: DocumentRect, x: f64, y: f64) -> bool {
    x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height
}

fn document_rects_intersect(a: DocumentRect, b: DocumentRect) -> bool {
    a.x <= b.x + b.width && a.x + a.width >= b.x && a.y <= b.y + b.height && a.y + a.height >= b.y
}

fn should_toggle_fit(click_count: i32, has_link: bool, first_was_link: bool) -> bool {
    click_count == 2 && !has_link && !first_was_link
}

fn external_uri_allowed(uri: &str) -> bool {
    if uri.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return false;
    }
    let Some((scheme, _)) = uri.split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    if !chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        || !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        return false;
    }
    matches!(
        scheme.to_ascii_lowercase().as_str(),
        "http" | "https" | "mailto"
    )
}

fn icon_pill(icon: &str, tip: &str) -> Button {
    let b = Button::from_icon_name(icon);
    b.set_tooltip_text(Some(tip));
    b.add_css_class("lector-pill");
    b
}

fn toggle_pill(icon: &str, tip: &str) -> ToggleButton {
    let b = ToggleButton::new();
    b.set_icon_name(icon);
    b.set_tooltip_text(Some(tip));
    b.add_css_class("lector-pill");
    b
}

fn menu_sep() -> Separator {
    let s = Separator::new(Orientation::Horizontal);
    s.add_css_class("lector-menu-sep");
    s
}

fn menu_heading(text: &str) -> Label {
    let l = Label::new(Some(text));
    l.add_css_class("lector-menu-heading");
    l.set_halign(gtk::Align::Start);
    l.set_xalign(0.0);
    l
}

fn shortcut_row(action: &str, keys: &str) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 12);
    row.add_css_class("lector-shortcut-row");
    let name = Label::new(Some(action));
    name.add_css_class("lector-menu-label");
    name.set_halign(gtk::Align::Start);
    name.set_hexpand(true);
    name.set_xalign(0.0);
    let key = Label::new(Some(keys));
    key.add_css_class("lector-menu-key");
    key.set_halign(gtk::Align::End);
    row.append(&name);
    row.append(&key);
    row
}

fn menu_check_row(label: &str, shortcut: &str) -> CheckButton {
    let item = CheckButton::new();
    item.update_property(&[gtk::accessible::Property::Label(label)]);
    item.add_css_class("lector-menu-item");
    item.set_halign(gtk::Align::Fill);
    item.set_hexpand(true);

    let row = GtkBox::new(Orientation::Horizontal, 10);
    let name = Label::new(Some(label));
    name.add_css_class("lector-menu-label");
    name.set_halign(gtk::Align::Start);
    name.set_hexpand(true);
    name.set_xalign(0.0);
    row.append(&name);
    if !shortcut.is_empty() {
        let key = Label::new(Some(shortcut));
        key.add_css_class("lector-menu-key");
        key.set_halign(gtk::Align::End);
        row.append(&key);
    }
    item.set_child(Some(&row));
    item
}

fn bare_menu_row(icon: &str, label: &str, shortcut: &str) -> Button {
    let b = Button::new();
    b.add_css_class("lector-menu-item");
    b.add_css_class("flat");
    b.set_halign(gtk::Align::Fill);
    b.set_hexpand(true);
    b.set_has_frame(false);
    b.set_tooltip_text(Some(label));

    let row = GtkBox::new(Orientation::Horizontal, 10);
    let ico = Image::from_icon_name(icon);
    ico.add_css_class("lector-menu-icon");
    ico.set_pixel_size(16);
    let name = Label::new(Some(label));
    name.add_css_class("lector-menu-label");
    name.set_halign(gtk::Align::Start);
    name.set_hexpand(true);
    name.set_xalign(0.0);
    row.append(&ico);
    row.append(&name);
    if !shortcut.is_empty() {
        let key = Label::new(Some(shortcut));
        key.add_css_class("lector-menu-key");
        key.set_halign(gtk::Align::End);
        row.append(&key);
    }
    b.set_child(Some(&row));
    b
}

fn connect_menu_row(button: &Button, pop: &Popover, h: &Handle, f: fn(&Lector)) {
    let pop = pop.clone();
    let h = h.clone();
    button.connect_clicked(move |_| {
        pop.popdown();
        if let Some(ui) = h.get() {
            f(&ui);
        }
    });
}

fn menu_row(
    icon: &str,
    label: &str,
    shortcut: &str,
    pop: &Popover,
    h: &Handle,
    f: fn(&Lector),
) -> Button {
    let b = bare_menu_row(icon, label, shortcut);
    connect_menu_row(&b, pop, h, f);
    b
}

type MenuItem = (&'static str, &'static str, &'static str, fn(&Lector));

fn append_menu_section(col: &GtkBox, heading: &str, items: &[MenuItem], pop: &Popover, h: &Handle) {
    col.append(&menu_heading(heading));
    for (icon, label, shortcut, f) in items {
        col.append(&menu_row(icon, label, shortcut, pop, h, *f));
    }
}

fn menu_open(ui: &Lector) {
    ui.open_dialog();
}
fn menu_save(ui: &Lector) {
    ui.save_copy();
}
fn menu_print(ui: &Lector) {
    ui.print_doc();
}
fn menu_sidebar(ui: &Lector) {
    ui.toggle_sidebar();
}
fn menu_rot_l(ui: &Lector) {
    ui.rotate(-90);
}
fn menu_rot_r(ui: &Lector) {
    ui.rotate(90);
}
fn menu_fs(ui: &Lector) {
    ui.toggle_fullscreen();
}
fn menu_pres(ui: &Lector) {
    ui.toggle_presentation();
}
fn menu_reload(ui: &Lector) {
    ui.reload();
}
fn menu_highlight(ui: &Lector) {
    ui.add_highlight();
}
fn menu_note(ui: &Lector) {
    ui.add_note();
}
fn menu_delete_annotation(ui: &Lector) {
    ui.delete_targeted_annotation();
}
fn menu_history(ui: &Lector) {
    ui.show_reading_history();
}
fn menu_props(ui: &Lector) {
    ui.show_properties();
}
fn menu_keys(ui: &Lector) {
    ui.show_shortcuts();
}
fn menu_about(ui: &Lector) {
    ui.show_about();
}

fn surface_texture(surf: &cairo::ImageSurface) -> Option<gdk::MemoryTexture> {
    surf.flush();
    let w = surf.width();
    let h = surf.height();
    let stride = surf.stride() as usize;
    let mut bytes = None;
    surf.with_data(|data| bytes = Some(glib::Bytes::from(data)))
        .ok()?;
    let bytes = bytes?;
    Some(gdk::MemoryTexture::new(
        w,
        h,
        gdk::MemoryFormat::B8g8r8a8Premultiplied,
        &bytes,
        stride,
    ))
}

#[cfg(test)]
#[path = "window_ui_tests.rs"]
mod ui_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn double_click_fit_is_suppressed_over_document_links() {
        assert!(should_toggle_fit(2, false, false));
        assert!(!should_toggle_fit(2, true, false));
        assert!(!should_toggle_fit(2, false, true));
        assert!(!should_toggle_fit(1, false, false));
    }

    #[test]
    fn external_link_policy_allows_web_and_mail_but_rejects_local_handlers() {
        for uri in [
            "https://omarchy.org/docs",
            "HTTP://example.com",
            "mailto:hello@example.com",
        ] {
            assert!(external_uri_allowed(uri), "{uri}");
        }
        for uri in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "ssh://host",
            "custom-handler:payload",
            "not a uri",
        ] {
            assert!(!external_uri_allowed(uri), "{uri}");
        }
    }

    #[test]
    fn image_surface_can_be_copied_into_a_texture() {
        let surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 10, 10)
            .expect("create Cairo surface");
        assert!(
            surface_texture(&surface).is_some(),
            "a valid shared Cairo surface must produce a texture"
        );
    }

    #[test]
    fn page_structure_requires_matching_layout_and_page_identities() {
        let continuous = PageStructure::new(LayoutMode::Continuous, vec![0, 1]);
        assert!(continuous.matches(LayoutMode::Continuous, &[0, 1]));
        assert!(!continuous.matches(LayoutMode::Dual, &[0, 1]));
        assert!(!continuous.matches(LayoutMode::Single, &[1]));
        assert!(!continuous.matches(LayoutMode::Continuous, &[1, 2]));
    }

    #[test]
    fn presentation_restore_restores_all_state_before_fullscreen_exit() {
        let mut presentation = true;
        let mut restore = Some((LayoutMode::Dual, Zoom::Custom(1.25), Sidebar::Outline));
        let mut layout = LayoutMode::Single;
        let mut zoom = Zoom::FitPage;
        let mut sidebar = Sidebar::Hidden;

        assert!(restore_presentation_state(
            &mut presentation,
            &mut restore,
            &mut layout,
            &mut zoom,
            &mut sidebar,
        ));
        assert!(!presentation);
        assert_eq!(restore, None);
        assert_eq!(layout, LayoutMode::Dual);
        assert_eq!(zoom, Zoom::Custom(1.25));
        assert_eq!(sidebar, Sidebar::Outline);
    }

    #[test]
    fn allocation_changes_are_filtered_without_missing_sidebar_or_find_resizes() {
        assert!(!allocation_changed((800, 700), (800, 700)));
        assert!(allocation_changed((800, 700), (640, 700)));
        assert!(allocation_changed((640, 700), (640, 650)));
        assert!(!allocation_changed((640, 650), (16, 650)));
    }

    #[test]
    fn replaced_search_work_is_generation_cancelled_and_bounded() {
        use std::cell::{Cell, RefCell};
        use std::rc::Rc;

        let context = glib::MainContext::default();
        let scheduled_generation: u64 = 41;
        let generation = Rc::new(Cell::new(scheduled_generation));
        let cancelled_runs = Rc::new(Cell::new(0));
        glib::idle_add_local_once({
            let generation = generation.clone();
            let cancelled_runs = cancelled_runs.clone();
            move || {
                if generation.get() == scheduled_generation {
                    cancelled_runs.set(cancelled_runs.get() + 1);
                }
            }
        });
        generation.set(scheduled_generation.wrapping_add(1));

        let mut turns = 0;
        while context.pending() {
            turns += 1;
            assert!(
                turns <= 8,
                "cancelled idle work must leave the main context promptly"
            );
            context.iteration(false);
        }
        assert_eq!(cancelled_runs.get(), 0);

        // A defensive bound also prevents a pathological old job from
        // scheduling forever if a document changes size unexpectedly.
        let old_job = Rc::new(RefCell::new(
            SearchJob::after_first_page(8, 6).expect("pages remain to search"),
        ));
        let inspected = Rc::new(RefCell::new(Vec::new()));
        glib::idle_add_local({
            let old_job = old_job.clone();
            let inspected = inspected.clone();
            move || {
                let mut job = old_job.borrow_mut();
                if let Some(page) = job.take_page(1) {
                    inspected.borrow_mut().push(page);
                    if job.remaining > 0 {
                        glib::ControlFlow::Continue
                    } else {
                        glib::ControlFlow::Break
                    }
                } else {
                    glib::ControlFlow::Break
                }
            }
        });
        while context.pending() {
            turns += 1;
            assert!(
                turns <= 16,
                "old work must remain bounded after a short replacement"
            );
            context.iteration(false);
        }
        assert_eq!(inspected.borrow().as_slice(), &[0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn document_replacement_invalidates_query_and_pending_hits() {
        let mut generation = 9;
        let mut query = "needle".to_string();
        let mut rectangle = poppler::Rectangle::new();
        rectangle.set_x1(1.0);
        let mut hits = vec![(4, rectangle)];
        let mut index = Some(0);

        invalidate_search_for_document(&mut generation, &mut query, &mut hits, &mut index);
        assert_eq!(generation, 10);
        assert!(query.is_empty());
        assert!(hits.is_empty());
        assert_eq!(index, None);
    }

    #[test]
    fn outgoing_document_state_is_available_to_a_same_file_reload() {
        let mut session = Session::default();
        let path = Path::new("/documents/reload.pdf");
        remember_document_state(&mut session, path, 6, Zoom::Custom(1.4));

        let restored = session
            .restore(path)
            .expect("state remembered before reload");
        assert_eq!(restored.page, 6);
        assert_eq!(
            session::parse_zoom(&restored.zoom, restored.custom),
            Zoom::Custom(1.4)
        );
    }

    #[test]
    fn ctrl_scroll_zoom_uses_stepped_wheel_and_smooth_touchpad_deltas() {
        assert_eq!(ctrl_scroll_zoom_factor(0.0), 1.0);
        assert_eq!(ctrl_scroll_zoom_factor(-1.0), 1.1);
        assert_eq!(ctrl_scroll_zoom_factor(1.0), 1.0 / 1.1);
        assert!((ctrl_scroll_zoom_factor(-0.2) - 1.1_f64.powf(0.2)).abs() < 1e-12);
    }

    #[test]
    fn pinch_and_factor_zoom_clamp_to_supported_range() {
        assert_eq!(pinch_target_scale(1.0, 2.0), 2.0);
        assert_eq!(pinch_target_scale(1.0, 0.01), 0.1);
        assert_eq!(pinch_target_scale(8.0, 4.0), MAX_ZOOM);
        assert_eq!(zoom_from_factor(1.0, 1.15), Zoom::Custom(1.15));
        assert_eq!(zoom_from_factor(0.1, 0.5), Zoom::Custom(0.1));
        assert_eq!(zoom_from_factor(10.0, 4.0), Zoom::Custom(MAX_ZOOM));
        assert!(pinch_scale_is_meaningful(1.0, 1.03));
        assert!(!pinch_scale_is_meaningful(1.0, 1.01));
    }

    #[test]
    fn zoom_restores_the_content_point_under_the_viewport_focus() {
        let anchor = ViewportAnchor {
            content_x: 100.0,
            content_y: 200.0,
            view_x: 40.0,
            view_y: 50.0,
            scale: 1.0,
        };
        assert_eq!(restored_scroll(anchor, 2.0), (160.0, 350.0));
        assert_eq!(clamp_adjustment(500.0, 0.0, 400.0, 100.0), 300.0);
        assert_eq!(clamp_adjustment(-10.0, 0.0, 400.0, 100.0), 0.0);
    }

    #[test]
    fn same_page_scroll_does_not_request_a_chrome_update() {
        assert_eq!(continuous_scroll_page_changed(0, 0), None);
        assert_eq!(continuous_scroll_page_changed(2, 2), None);
        assert_eq!(continuous_scroll_page_changed(0, 4), Some(4));
    }

    #[test]
    fn shortcut_overlay_is_grouped_like_the_app_menu() {
        let headings: Vec<_> = shortcut_sections().iter().map(|s| s.heading).collect();
        assert_eq!(
            headings,
            [
                "FILE",
                "ANNOTATIONS",
                "VIEW",
                "FIND",
                "NAVIGATION",
                "DOCUMENT"
            ]
        );
        assert!(shortcut_sections().iter().any(|section| {
            section
                .rows
                .iter()
                .any(|row| row.action == "Open" && row.keys == "Ctrl+O")
        }));
        assert!(shortcut_sections()
            .iter()
            .any(|section| section.rows.iter().any(|row| row.action == "Pinch to zoom")));
        assert!(shortcut_sections().iter().any(|section| section
            .rows
            .iter()
            .any(|row| row.action == "Highlight selection" && row.keys == "Ctrl+Shift+H")));
    }

    #[test]
    fn shifted_annotation_shortcuts_accept_uppercase_keyvals() {
        let modifiers = gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK;
        assert_eq!(
            annotation_shortcut(Key::H, modifiers),
            Some(AnnotationShortcut::Highlight)
        );
        assert_eq!(
            annotation_shortcut(Key::N, modifiers),
            Some(AnnotationShortcut::Note)
        );
        assert_eq!(
            annotation_shortcut(Key::h, gdk::ModifierType::CONTROL_MASK),
            Some(AnnotationShortcut::History)
        );
    }

    #[test]
    fn beginning_selection_and_empty_annotation_hits_clear_the_old_target() {
        let mut selection = None;
        let mut targeted = Some("old-annotation".to_string());
        begin_selection_state(&mut selection, &mut targeted, Some((2, 10.0, 20.0)));
        let selection = selection.expect("selection begins at the hit");
        assert_eq!(selection.page, 2);
        assert_eq!((selection.x1, selection.y1), (10.0, 20.0));
        assert_eq!((selection.x2, selection.y2), (10.0, 20.0));
        assert_eq!(targeted, None);

        targeted = Some("another-annotation".to_string());
        assert!(replace_annotation_target(&mut targeted, None));
        assert_eq!(targeted, None);
    }

    #[test]
    fn rotation_clears_display_coordinate_selection() {
        let mut rotation = 270;
        let mut selection = Some(Selection {
            page: 0,
            x1: 10.0,
            y1: 20.0,
            x2: 30.0,
            y2: 40.0,
        });

        rotate_view_state(&mut rotation, &mut selection, 90);

        assert_eq!(rotation, 0);
        assert!(selection.is_none());
    }

    #[test]
    fn document_properties_use_em_dash_for_missing_metadata() {
        assert_eq!(display_meta(None), "—");
        assert_eq!(display_meta(Some("")), "—");
        assert_eq!(display_meta(Some("  ")), "—");
        assert_eq!(display_meta(Some("Hanky")), "Hanky");
        let path = crate::doc::write_test_pdf("props-meta", &["BT /F1 20 Tf 72 700 Td (hi) Tj ET"]);
        let doc = crate::doc::OpenDoc::open(&path, None).expect("fixture pdf");
        let rows = property_fields(&doc);
        assert!(rows.iter().any(|(key, value)| *key == "File"
            && Path::new(value)
                .file_name()
                .is_some_and(|name| name.to_string_lossy().contains("props-meta"))));
        assert!(rows
            .iter()
            .any(|(key, value)| *key == "Pages" && value == "1"));
        for key in ["Author", "Subject"] {
            assert!(
                rows.iter().any(|(k, v)| *k == key && v == "—"),
                "{key} should use an em dash when Poppler has no value"
            );
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn about_copy_names_lector_not_a_generic_toolkit_dialog() {
        let about = about_copy();
        assert_eq!(about.name, "Lector");
        assert_eq!(about.tagline, "Native PDF viewer for Omarchy");
        assert_eq!(about.version, env!("CARGO_PKG_VERSION"));
        assert!(!about.detail.contains("GNOME"));
    }
}
