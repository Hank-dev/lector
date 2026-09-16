use std::collections::HashSet;

use crate::annotations::AnnotationStore;
use crate::doc::{OpenDoc, SearchOptions};
use crate::session::Session;
use crate::theme::Theme;
use crate::view::{Layout, LayoutMode, PdfMode, RasterCache, Selection, Zoom};

use super::{NavigationHistory, Sidebar};

pub(crate) struct ViewState {
    pub doc: Option<OpenDoc>,
    pub zoom: Zoom,
    pub layout_mode: LayoutMode,
    pub rotation: i32,
    pub pdf_mode: PdfMode,
    pub page: i32,
    pub layout: Layout,
    pub selection: Option<Selection>,
    pub view_w: f64,
    pub view_h: f64,
}

impl ViewState {
    pub fn new() -> Self {
        Self {
            doc: None,
            zoom: Zoom::FitWidth,
            layout_mode: LayoutMode::Continuous,
            rotation: 0,
            pdf_mode: PdfMode::Normal,
            page: 0,
            layout: Layout {
                pages: Vec::new(),
                scale: 1.0,
            },
            selection: None,
            view_w: 800.0,
            view_h: 700.0,
        }
    }

    pub fn rotate_by(&mut self, degrees: i32) {
        self.selection = None;
        self.rotation = (self.rotation + degrees).rem_euclid(360);
    }
}

pub(crate) struct SearchState {
    pub hits: Vec<(i32, poppler::Rectangle)>,
    pub idx: Option<usize>,
    pub gen: u64,
    pub query: String,
    pub options: SearchOptions,
}

impl SearchState {
    pub fn new() -> Self {
        Self {
            hits: Vec::new(),
            idx: None,
            gen: 0,
            query: String::new(),
            options: SearchOptions::default(),
        }
    }

    pub fn invalidate(&mut self) {
        self.gen = self.gen.wrapping_add(1);
        self.query.clear();
        self.hits.clear();
        self.idx = None;
    }
}

pub(crate) struct PaneState {
    pub sidebar: Sidebar,
    pub outline_collapsed: HashSet<usize>,
    pub targeted_annotation: Option<String>,
}

impl PaneState {
    pub fn new() -> Self {
        Self {
            sidebar: Sidebar::Hidden,
            outline_collapsed: HashSet::new(),
            targeted_annotation: None,
        }
    }
}

pub(crate) struct Inner {
    pub view: ViewState,
    pub search: SearchState,
    pub pane: PaneState,
    pub presentation: bool,
    pub theme: Theme,
    pub session: Session,
    pub annotations: Option<AnnotationStore>,
    pub annotation_error: Option<String>,
    pub ignore_zoom_dropdown: bool,
    pub ignore_scroll: bool,
    pub ignore_menu_checks: bool,
    pub layout_gen: u64,
    pub thumb_gen: u64,
    pub pres_restore: Option<(LayoutMode, Zoom, Sidebar)>,
    pub raster: RasterCache,
    pub raster_gen: u64,
    pub navigation: NavigationHistory,
}
