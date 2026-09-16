//! GTK window. Impl files import shared types with `use super::*;`.
#![allow(unused_imports)]

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::time::{Duration, Instant};

use gtk::gdk::Key;
use gtk::prelude::*;
use gtk::{
    gdk, gio, glib, Application, ApplicationWindow, Box as GtkBox, Button, CheckButton,
    CssProvider, DrawingArea, DropDown, Entry, Fixed, GestureClick, GestureDrag, GestureZoom,
    Image, Label, ListBox, MenuButton, Orientation, PasswordEntry, Popover, Revealer,
    ScrolledWindow, Stack, StackSwitcher, ToggleButton,
};

use crate::annotations::{
    document_rect_to_display, new_annotation, new_annotation_from_rects, union_rects, Annotation,
    AnnotationKind, AnnotationStore, DocumentRect,
};
use crate::doc::{
    display_page_label, initially_collapsed_outline, outline_item_has_children,
    page_index_from_input, visible_outline_indices, DocumentLink, OpenDoc, OpenError,
    SearchOptions,
};
use crate::session::{self, Session};
use crate::theme::{self, Theme};
use crate::view::{
    apply_print_page_transform, display_point_to_document, draw_annotations, draw_page_overlays,
    layout_content_size, page_in_view, page_scroll_y, pages_intersecting_view, paint_raster,
    print_content_size, raster_cache_needs_clear, raster_cache_needs_reset, raster_idle_is_stale,
    raster_page, search_hit_scroll_y, selected_text, selection_highlight_rects, thumbnail,
    thumbnail_css_size, AnnotationDrawing, Layout, LayoutMode, PageDrawing, PdfMode, RasterCache,
    RasterKey, Selection, Zoom, MAX_ZOOM, SEARCH_HIT_MARGIN, THUMB_CSS_WIDTH, VIEW_PAGE_PAD,
};

mod annotations_ui;
mod canvas;
mod chrome;
mod construct;
mod document;
mod helpers;
mod nav;
mod search;
mod state;
mod widgets;

pub(crate) use helpers::*;
pub(crate) use state::*;
pub(crate) use widgets::*;

pub struct Lector {
    window: ApplicationWindow,
    pages_box: Fixed,
    scrolled: ScrolledWindow,
    toolbar: GtkBox,
    page_entry: Entry,
    page_total: Label,
    search_entry: Entry,
    search_info: Label,
    search_results: ListBox,
    search_results_sw: ScrolledWindow,
    #[allow(dead_code)]
    case_btn: ToggleButton,
    #[allow(dead_code)]
    word_btn: ToggleButton,
    find_bar: Revealer,
    file_banner: Revealer,
    file_banner_label: Label,
    file_reload_btn: Button,
    sidebar_revealer: Revealer,
    sidebar_btn: ToggleButton,
    stack: Stack,
    thumbs: ListBox,
    outline: ListBox,
    marks: ListBox,
    zoom_drop: DropDown,
    title_label: Label,
    back_btn: Button,
    forward_btn: Button,
    prev_btn: Button,
    next_btn: Button,
    zoom_out_btn: Button,
    zoom_in_btn: Button,
    find_btn: Button,
    cont_item: CheckButton,
    dual_item: CheckButton,
    cover_item: CheckButton,
    single_item: CheckButton,
    normal_item: CheckButton,
    omarchy_item: CheckButton,
    invert_item: CheckButton,
    highlight_item: Button,
    note_item: Button,
    delete_annotation_item: Button,
    document_menu_items: RefCell<Vec<gtk::Widget>>,
    thumbs_sw: ScrolledWindow,
    page_widgets: RefCell<Vec<(i32, DrawingArea)>>,
    rendered_structure: RefCell<Option<PageStructure>>,
    raster_pending: RefCell<HashSet<i32>>,
    sidebar_ready: Cell<u8>,
    raster_busy: Cell<bool>,
    pinch_active: Cell<bool>,
    inner: Rc<RefCell<Inner>>,
    css: CssProvider,
    me: RefCell<Weak<Lector>>,
    hold: RefCell<Option<Rc<Lector>>>,
    _monitor: RefCell<Option<gio::FileMonitor>>,
    doc_monitor: RefCell<Option<gio::FileMonitor>>,
    search_debounce_gen: Cell<u64>,
    file_change_gen: Cell<u64>,
    ignore_search_changed: Cell<bool>,
    ignore_search_results: Cell<bool>,
    ignore_marks: Cell<bool>,
    pending_disk_change: Cell<Option<DiskChange>>,
    file_watch_ignore_until: Cell<Option<Instant>>,
}

#[derive(Clone)]
pub(crate) struct Handle(pub(crate) Weak<Lector>);

impl Handle {
    pub(crate) fn get(&self) -> Option<Rc<Lector>> {
        self.0.upgrade()
    }
}

#[cfg(test)]
#[path = "../window_ui_tests.rs"]
mod ui_tests;

#[cfg(test)]
#[path = "../window_menu_test.rs"]
mod menu_test;

#[cfg(test)]
mod tests;
