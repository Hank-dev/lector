use std::collections::{HashMap, VecDeque};
use std::f64::consts::PI;

use cairo::{Context, Format, ImageSurface, Operator};
use poppler::{Color, Rectangle, SelectionStyle};

use crate::annotations::{document_rect_to_display, Annotation, AnnotationKind, DocumentRect};
use crate::doc::OpenDoc;
use crate::theme::PdfPalette;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PdfMode {
    Normal,
    Inverted,
    Omarchy,
}

impl PdfMode {
    pub fn key(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Inverted => "inverted",
            Self::Omarchy => "omarchy",
        }
    }

    pub fn parse(value: &str, legacy_invert: bool) -> Self {
        match value {
            "normal" => Self::Normal,
            "inverted" => Self::Inverted,
            "omarchy" => Self::Omarchy,
            _ if legacy_invert => Self::Inverted,
            _ => Self::Normal,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Zoom {
    FitWidth,
    FitPage,
    FitHeight,
    Actual,
    Custom(f64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutMode {
    Continuous,
    Single,
    Dual,
}

#[derive(Clone, Copy, Debug)]
pub struct PageGeom {
    pub index: i32,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub scale: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct Selection {
    pub page: i32,
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

pub struct PageDrawing<'a> {
    pub doc: &'a OpenDoc,
    pub geom: &'a PageGeom,
    pub rotation: i32,
    pub invert: bool,
    pub selection: Option<Selection>,
    pub search_hits: &'a [(i32, Rectangle)],
    pub current_hit: Option<usize>,
}

pub struct AnnotationDrawing<'a> {
    pub annotations: &'a [Annotation],
    pub page: i32,
    pub page_width: f64,
    pub page_height: f64,
    pub rotation: i32,
    pub scale: f64,
    pub targeted: Option<&'a str>,
}

pub struct Layout {
    pub pages: Vec<PageGeom>,
    pub scale: f64,
}

pub const PAGE_GAP: f64 = 16.0;
pub const PAGE_PAD: f64 = 16.0;
pub const MAX_ZOOM: f64 = 16.0;
pub const VIEW_PAGE_PAD: f64 = 400.0;
pub const THUMB_CSS_WIDTH: f64 = 180.0;

pub fn pages_intersecting_view(
    pages: &[PageGeom],
    view_top: f64,
    view_h: f64,
    pad: f64,
) -> Vec<i32> {
    let top = view_top - pad;
    let bottom = view_top + view_h.max(0.0) + pad;
    pages
        .iter()
        .filter(|page| page.y + page.h >= top && page.y <= bottom)
        .map(|page| page.index)
        .collect()
}

pub fn page_at_content_y(pages: &[PageGeom], y: f64) -> i32 {
    for page in pages {
        if y < page.y + page.h {
            return page.index;
        }
    }
    pages.last().map(|page| page.index).unwrap_or(0)
}

pub fn page_scroll_y(pages: &[PageGeom], page: i32) -> f64 {
    pages
        .iter()
        .find(|geom| geom.index == page)
        .map(|geom| (geom.y - 8.0).max(0.0))
        .unwrap_or(0.0)
}

pub fn raster_idle_is_stale(job_gen: u64, live_gen: u64) -> bool {
    job_gen != live_gen
}

pub fn raster_cache_needs_reset(
    old_scale: f64,
    new_scale: f64,
    old_rotation: i32,
    new_rotation: i32,
    old_mode: PdfMode,
    new_mode: PdfMode,
) -> bool {
    old_rotation != new_rotation || old_mode != new_mode || (old_scale - new_scale).abs() > 0.001
}

pub fn raster_cache_needs_clear(
    _old_scale: f64,
    _new_scale: f64,
    old_rotation: i32,
    new_rotation: i32,
    old_mode: PdfMode,
    new_mode: PdfMode,
) -> bool {
    old_rotation != new_rotation || old_mode != new_mode
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RasterKey {
    pub index: i32,
    pub width: i32,
    pub height: i32,
    pub rotation: i32,
    pub pdf_mode: PdfMode,
    pub palette: PdfPalette,
}

impl RasterKey {
    pub fn from_geom(
        geom: &PageGeom,
        rotation: i32,
        pdf_mode: PdfMode,
        palette: PdfPalette,
        dpr: f64,
    ) -> Self {
        let dpr = dpr.max(1.0);
        Self {
            index: geom.index,
            width: (geom.w * dpr).ceil().max(1.0) as i32,
            height: (geom.h * dpr).ceil().max(1.0) as i32,
            rotation,
            pdf_mode,
            palette: if pdf_mode == PdfMode::Omarchy {
                palette
            } else {
                PdfPalette::default()
            },
        }
    }
}

pub struct RasterCache {
    map: HashMap<RasterKey, ImageSurface>,
    lru: VecDeque<RasterKey>,
    cap: usize,
}

impl RasterCache {
    pub fn new(cap: usize) -> Self {
        Self {
            map: HashMap::new(),
            lru: VecDeque::new(),
            cap: cap.max(1),
        }
    }

    pub fn get(&mut self, key: &RasterKey) -> Option<ImageSurface> {
        let surf = self.map.get(key)?.clone();
        self.touch(key);
        Some(surf)
    }

    pub fn get_page(&mut self, index: i32) -> Option<ImageSurface> {
        let key = self
            .lru
            .iter()
            .rev()
            .find(|key| key.index == index)
            .copied()?;
        self.get(&key)
    }

    pub fn contains(&self, key: &RasterKey) -> bool {
        self.map.contains_key(key)
    }

    pub fn insert(&mut self, key: RasterKey, surf: ImageSurface) {
        use std::collections::hash_map::Entry;
        match self.map.entry(key) {
            Entry::Occupied(mut occupied) => {
                occupied.insert(surf);
            }
            Entry::Vacant(vacant) => {
                vacant.insert(surf);
                while self.map.len() > self.cap {
                    if let Some(old) = self.lru.pop_front() {
                        self.map.remove(&old);
                    } else {
                        break;
                    }
                }
            }
        }
        self.touch(&key);
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.lru.clear();
    }

    fn touch(&mut self, key: &RasterKey) {
        self.lru.retain(|item| item != key);
        self.lru.push_back(*key);
    }
}

pub fn raster_page(
    doc: &OpenDoc,
    geom: &PageGeom,
    rotation: i32,
    pdf_mode: PdfMode,
    palette: PdfPalette,
    dpr: f64,
) -> Option<ImageSurface> {
    let dpr = dpr.max(1.0);
    let key = RasterKey::from_geom(geom, rotation, pdf_mode, palette, dpr);
    let mut surf = ImageSurface::create(Format::ARgb32, key.width, key.height).ok()?;
    {
        let cr = Context::new(&surf).ok()?;
        cr.scale(dpr, dpr);
        draw_one_page(
            &cr,
            PageDrawing {
                doc,
                geom,
                rotation,
                invert: pdf_mode == PdfMode::Inverted,
                selection: None,
                search_hits: &[],
                current_hit: None,
            },
        );
    }
    if pdf_mode == PdfMode::Omarchy {
        theme_surface(&mut surf, palette)?;
    }
    Some(surf)
}

fn rendered_rgb(source: [u8; 3], mode: PdfMode, palette: PdfPalette) -> [u8; 3] {
    match mode {
        PdfMode::Normal => source,
        PdfMode::Inverted => source.map(|channel| 255 - channel),
        PdfMode::Omarchy => themed_rgb(source, palette, false),
    }
}

fn themed_rgb(source: [u8; 3], palette: PdfPalette, invert: bool) -> [u8; 3] {
    let source = source.map(|channel| {
        let channel = if invert { 255 - channel } else { channel };
        channel as f64 / 255.0
    });
    let luma = 0.2126 * source[0] + 0.7152 * source[1] + 0.0722 * source[2];
    let chroma = source.iter().copied().fold(f64::MIN, f64::max)
        - source.iter().copied().fold(f64::MAX, f64::min);
    let preserve = (chroma * 1.1).clamp(0.0, 0.72);
    let mut output = [0; 3];
    for channel in 0..3 {
        let paper = palette.paper[channel] as f64 / 255.0;
        let ink = palette.ink[channel] as f64 / 255.0;
        let toned = ink * (1.0 - luma) + paper * luma;
        let mixed = toned * (1.0 - preserve) + source[channel] * preserve;
        output[channel] = (mixed.clamp(0.0, 1.0) * 255.0).round() as u8;
    }

    let color_luma = |rgb: [u8; 3]| {
        (0.2126 * rgb[0] as f64 + 0.7152 * rgb[1] as f64 + 0.0722 * rgb[2] as f64) / 255.0
    };
    if color_luma(palette.paper) + 0.1 < color_luma(palette.ink) && chroma > 0.08 {
        let lift = ((chroma - 0.05) * 0.8).clamp(0.0, 0.30);
        for (channel, ink) in output.iter_mut().zip(palette.ink) {
            *channel = (*channel as f64 * (1.0 - lift) + ink as f64 * lift).round() as u8;
        }
    }
    output
}

fn theme_surface(surface: &mut ImageSurface, palette: PdfPalette) -> Option<()> {
    surface.flush();
    let width = surface.width() as usize;
    let height = surface.height() as usize;
    let stride = surface.stride() as usize;
    {
        let mut data = surface.data().ok()?;
        for row in data.chunks_exact_mut(stride).take(height) {
            let (pixels, _) = row[..width * 4].as_chunks_mut::<4>();
            for pixel in pixels {
                let mapped =
                    rendered_rgb([pixel[2], pixel[1], pixel[0]], PdfMode::Omarchy, palette);
                pixel[0] = mapped[2];
                pixel[1] = mapped[1];
                pixel[2] = mapped[0];
            }
        }
    }
    surface.mark_dirty();
    Some(())
}

pub fn paint_raster(cr: &Context, surf: &ImageSurface, css_w: f64, css_h: f64) {
    let sw = surf.width() as f64;
    let sh = surf.height() as f64;
    if sw < 1.0 || sh < 1.0 || css_w < 1.0 || css_h < 1.0 {
        return;
    }
    cr.save().ok();
    cr.scale(css_w / sw, css_h / sh);
    cr.set_source_surface(surf, 0.0, 0.0).ok();
    cr.paint().ok();
    cr.restore().ok();
}

pub fn annotation_render_bounds(
    rect: DocumentRect,
    page_width: f64,
    page_height: f64,
    rotation: i32,
    scale: f64,
) -> DocumentRect {
    let display = document_rect_to_display(rect, page_width, page_height, rotation);
    let scale = scale.max(0.01);
    DocumentRect {
        x: display.x * scale,
        y: display.y * scale,
        width: display.width * scale,
        height: display.height * scale,
    }
}

pub fn draw_annotations(cr: &Context, drawing: AnnotationDrawing<'_>) {
    for annotation in drawing
        .annotations
        .iter()
        .filter(|annotation| annotation.page == drawing.page)
    {
        let bounds = annotation_render_bounds(
            annotation.rect,
            drawing.page_width,
            drawing.page_height,
            drawing.rotation,
            drawing.scale,
        );
        match annotation.kind {
            AnnotationKind::Highlight => cr.set_source_rgba(1.0, 0.82, 0.08, 0.34),
            AnnotationKind::Note { .. } => cr.set_source_rgba(1.0, 0.82, 0.08, 0.16),
        }
        cr.rectangle(bounds.x, bounds.y, bounds.width, bounds.height);
        cr.fill().ok();

        if matches!(annotation.kind, AnnotationKind::Note { .. }) {
            let marker_x = bounds.x + bounds.width;
            let marker_y = bounds.y;
            cr.set_source_rgb(1.0, 0.72, 0.0);
            cr.arc(marker_x, marker_y, 7.0, 0.0, PI * 2.0);
            cr.fill().ok();
            cr.set_source_rgb(0.22, 0.16, 0.0);
            cr.set_line_width(1.5);
            cr.move_to(marker_x, marker_y - 3.5);
            cr.line_to(marker_x, marker_y + 1.5);
            cr.stroke().ok();
            cr.arc(marker_x, marker_y + 4.0, 0.8, 0.0, PI * 2.0);
            cr.fill().ok();
        }
        if drawing.targeted == Some(annotation.id.as_str()) {
            cr.set_source_rgba(0.25, 0.55, 1.0, 0.9);
            cr.set_line_width(2.0);
            cr.rectangle(bounds.x, bounds.y, bounds.width, bounds.height);
            cr.stroke().ok();
        }
    }
}

impl Layout {
    pub fn compute(
        doc: &OpenDoc,
        mode: LayoutMode,
        zoom: Zoom,
        rotation: i32,
        current: i32,
        view_w: f64,
        view_h: f64,
    ) -> Self {
        let n = doc.n_pages;
        if n <= 0 {
            return Self {
                pages: Vec::new(),
                scale: 1.0,
            };
        }

        let current = current.clamp(0, n - 1);

        match mode {
            LayoutMode::Single => {
                let (pw, ph) = doc.size(current, rotation);
                let scale = scale_for(
                    zoom,
                    pw,
                    ph,
                    view_w - PAGE_PAD * 2.0,
                    view_h - PAGE_PAD * 2.0,
                );
                let w = pw * scale;
                let h = ph * scale;
                let x = ((view_w - w) / 2.0).max(PAGE_PAD / 2.0);
                let y = PAGE_PAD;
                Self {
                    pages: vec![PageGeom {
                        index: current,
                        x,
                        y,
                        w,
                        h,
                        scale,
                    }],
                    scale,
                }
            }
            LayoutMode::Dual => {
                let a = current - (current % 2);
                let b = (a + 1).min(n - 1);
                let (aw, ah) = doc.size(a, rotation);
                let (bw, bh) = if b != a {
                    doc.size(b, rotation)
                } else {
                    (0.0, 0.0)
                };
                let pair_w = aw + if b != a { bw + PAGE_GAP } else { 0.0 };
                let pair_h = ah.max(bh);
                let scale = scale_for(
                    zoom,
                    pair_w,
                    pair_h,
                    view_w - PAGE_PAD * 2.0,
                    view_h - PAGE_PAD * 2.0,
                );
                let mut pages = Vec::new();
                let mut x = ((view_w - pair_w * scale) / 2.0).max(PAGE_PAD / 2.0);
                let y = PAGE_PAD;
                pages.push(PageGeom {
                    index: a,
                    x,
                    y,
                    w: aw * scale,
                    h: ah * scale,
                    scale,
                });
                if b != a {
                    x += aw * scale + PAGE_GAP;
                    pages.push(PageGeom {
                        index: b,
                        x,
                        y,
                        w: bw * scale,
                        h: bh * scale,
                        scale,
                    });
                }
                Self { pages, scale }
            }
            LayoutMode::Continuous => {
                // Use first page as the fit reference (Evince-like).
                let (rw, rh) = doc.size(0, rotation);
                let scale = scale_for(
                    zoom,
                    rw,
                    rh,
                    view_w - PAGE_PAD * 2.0,
                    view_h - PAGE_PAD * 2.0,
                );
                let mut pages = Vec::new();
                let mut y = PAGE_PAD;
                for i in 0..n {
                    let (pw, ph) = doc.size(i, rotation);
                    let w = pw * scale;
                    let h = ph * scale;
                    let x = ((view_w - w) / 2.0).max(PAGE_PAD / 2.0);
                    pages.push(PageGeom {
                        index: i,
                        x,
                        y,
                        w,
                        h,
                        scale,
                    });
                    y += h + PAGE_GAP;
                }
                Self { pages, scale }
            }
        }
    }

    pub fn page_geom(&self, index: i32) -> Option<PageGeom> {
        self.pages.iter().copied().find(|p| p.index == index)
    }
}

fn scale_for(zoom: Zoom, pw: f64, ph: f64, vw: f64, vh: f64) -> f64 {
    let pw = pw.max(1.0);
    let ph = ph.max(1.0);
    let vw = vw.max(32.0);
    let vh = vh.max(32.0);
    match zoom {
        Zoom::Actual => 1.0,
        Zoom::Custom(s) => s.clamp(0.1, MAX_ZOOM),
        Zoom::FitWidth => (vw / pw).clamp(0.1, MAX_ZOOM),
        Zoom::FitHeight => (vh / ph).clamp(0.1, MAX_ZOOM),
        Zoom::FitPage => (vw / pw).min(vh / ph).clamp(0.1, MAX_ZOOM),
    }
}

pub fn draw_one_page(cr: &Context, drawing: PageDrawing<'_>) {
    let local = PageGeom {
        x: 0.0,
        y: 0.0,
        ..*drawing.geom
    };
    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.paint().ok();

    if let Some(page) = drawing.doc.page(local.index) {
        cr.save().ok();
        match drawing.rotation.rem_euclid(360) {
            90 => {
                cr.translate(local.w, 0.0);
                cr.rotate(PI / 2.0);
            }
            180 => {
                cr.translate(local.w, local.h);
                cr.rotate(PI);
            }
            270 => {
                cr.translate(0.0, local.h);
                cr.rotate(-PI / 2.0);
            }
            _ => {}
        }
        cr.scale(local.scale.max(0.01), local.scale.max(0.01));
        page.render(cr);
        if drawing.invert {
            cr.set_operator(Operator::Difference);
            cr.set_source_rgb(1.0, 1.0, 1.0);
            cr.paint().ok();
        }
        cr.restore().ok();
    }

    draw_page_overlays(cr, drawing);
}

pub fn draw_page_overlays(cr: &Context, drawing: PageDrawing<'_>) {
    let PageDrawing {
        doc,
        geom,
        rotation,
        selection,
        search_hits,
        current_hit,
        ..
    } = drawing;
    let local = PageGeom {
        x: 0.0,
        y: 0.0,
        ..*geom
    };

    for (i, (page, rect)) in search_hits.iter().enumerate() {
        if *page != local.index {
            continue;
        }
        let (x, y, w, h) = pdf_rect_to_view(&local, doc, rotation, rect);
        if current_hit == Some(i) {
            cr.set_source_rgba(1.0, 0.55, 0.1, 0.40);
        } else {
            cr.set_source_rgba(1.0, 0.85, 0.15, 0.28);
        }
        cr.rectangle(x, y, w, h);
        cr.fill().ok();
    }

    if let Some(sel) = selection {
        if sel.page == local.index {
            if let Some(page) = doc.page(local.index) {
                let mut r = selection_rect(doc, local.index, rotation, sel);
                let mut old = r;
                let mut glyph = Color::new();
                let mut back = Color::new();
                back.set_red(0x3d * 257);
                back.set_green(0xae * 257);
                back.set_blue(0xe6 * 257);
                cr.save().ok();
                apply_page_transform(cr, &local, rotation, doc);
                page.render_selection(
                    cr,
                    &mut r,
                    &mut old,
                    SelectionStyle::Glyph,
                    &mut glyph,
                    &mut back,
                );
                cr.restore().ok();
            }
        }
    }
}

fn apply_page_transform(cr: &Context, geom: &PageGeom, rotation: i32, doc: &OpenDoc) {
    cr.translate(geom.x, geom.y);
    let (pw, ph) = doc.size(geom.index, 0);
    match rotation.rem_euclid(360) {
        90 => {
            cr.translate(geom.w, 0.0);
            cr.rotate(PI / 2.0);
        }
        180 => {
            cr.translate(geom.w, geom.h);
            cr.rotate(PI);
        }
        270 => {
            cr.translate(0.0, geom.h);
            cr.rotate(-PI / 2.0);
        }
        _ => {}
    }
    cr.scale(geom.scale, geom.scale);
    let _ = (pw, ph);
}

pub fn display_point_to_document(
    doc: &OpenDoc,
    page: i32,
    rotation: i32,
    x: f64,
    y: f64,
) -> (f64, f64) {
    let (width, height) = doc.size(page, 0);
    match rotation.rem_euclid(360) {
        90 => (y, height - x),
        180 => (width - x, height - y),
        270 => (width - y, x),
        _ => (x, y),
    }
}

pub fn selection_rect(doc: &OpenDoc, page: i32, rotation: i32, sel: Selection) -> Rectangle {
    let (pw, ph) = doc.size(page, rotation);
    // Selection coordinates are in the displayed page's top-origin space.
    let mut x1 = sel.x1.clamp(0.0, pw);
    let mut x2 = sel.x2.clamp(0.0, pw);
    let mut y1 = sel.y1.clamp(0.0, ph);
    let mut y2 = sel.y2.clamp(0.0, ph);
    if x1 > x2 {
        std::mem::swap(&mut x1, &mut x2);
    }
    if y1 > y2 {
        std::mem::swap(&mut y1, &mut y2);
    }
    // Poppler's selection APIs take top-origin coordinates.  Undo the same
    // rotation used by draw_one_page/apply_page_transform before passing the
    // rectangle through.
    let (uw, uh) = doc.size(page, 0);
    let (px1, py1, px2, py2) = match rotation.rem_euclid(360) {
        90 => (y1, uh - x2, y2, uh - x1),
        180 => (uw - x2, uh - y2, uw - x1, uh - y1),
        270 => (uw - y2, x1, uw - y1, x2),
        _ => (x1, y1, x2, y2),
    };
    let mut r = Rectangle::new();
    r.set_x1(px1);
    r.set_y1(py1);
    r.set_x2(px2);
    r.set_y2(py2);
    r
}

pub fn selected_text(doc: &OpenDoc, rotation: i32, sel: Selection) -> Option<String> {
    let page = doc.page(sel.page)?;
    let mut r = selection_rect(doc, sel.page, rotation, sel);
    page.selected_text(SelectionStyle::Glyph, &mut r)
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

fn pdf_rect_to_view(
    geom: &PageGeom,
    doc: &OpenDoc,
    rotation: i32,
    rect: &Rectangle,
) -> (f64, f64, f64, f64) {
    let (uw, uh) = doc.size(geom.index, 0);
    // poppler find_text returns PDF bottom-left coordinates.
    let x_bl = rect.x1().min(rect.x2());
    let y_bl = rect.y1().min(rect.y2());
    let w = (rect.x2() - rect.x1()).abs();
    let h = (rect.y2() - rect.y1()).abs();
    let top = uh - y_bl - h;
    let (dx, dy, dw, dh) = match rotation.rem_euclid(360) {
        // These are the bounds produced by the same Cairo transforms used
        // when rendering the page, not merely a width/height swap.
        90 => (uh - top - h, x_bl, h, w),
        180 => (uw - x_bl - w, uh - top - h, w, h),
        270 => (top, uw - x_bl - w, h, w),
        _ => (x_bl, top, w, h),
    };
    (
        geom.x + dx * geom.scale,
        geom.y + dy * geom.scale,
        dw * geom.scale,
        dh * geom.scale,
    )
}

pub fn thumbnail_pixel_size(page_w: f64, page_h: f64, css_width: f64, dpr: f64) -> (i32, i32) {
    let dpr = dpr.max(1.0);
    let css_width = css_width.max(1.0);
    let page_w = page_w.max(1.0);
    let scale = (css_width * dpr) / page_w;
    (
        (page_w * scale).ceil().max(1.0) as i32,
        (page_h.max(1.0) * scale).ceil().max(1.0) as i32,
    )
}

pub fn thumbnail_css_size(page_w: f64, page_h: f64, css_width: f64) -> (i32, i32) {
    thumbnail_pixel_size(page_w, page_h, css_width, 1.0)
}

pub fn thumbnail(
    doc: &OpenDoc,
    index: i32,
    css_width: f64,
    dpr: f64,
    pdf_mode: PdfMode,
    palette: PdfPalette,
) -> Option<ImageSurface> {
    let page = doc.page(index)?;
    let (pw, ph) = page.size();
    let (w, h) = thumbnail_pixel_size(pw, ph, css_width, dpr);
    let mut surf = ImageSurface::create(Format::ARgb32, w, h).ok()?;
    {
        let cr = Context::new(&surf).ok()?;
        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.paint().ok()?;
        let scale = w as f64 / pw.max(1.0);
        cr.scale(scale, scale);
        page.render(&cr);
        if pdf_mode == PdfMode::Inverted {
            cr.set_operator(Operator::Difference);
            cr.set_source_rgb(1.0, 1.0, 1.0);
            cr.paint().ok()?;
        }
    }
    if pdf_mode == PdfMode::Omarchy {
        theme_surface(&mut surf, palette)?;
    }
    Some(surf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::PdfPalette;

    #[test]
    fn pdf_modes_keep_normal_invert_and_omarchy_distinct() {
        let palette = PdfPalette {
            paper: [16, 32, 48],
            ink: [208, 224, 240],
        };
        let source = [10, 40, 90];

        assert_eq!(rendered_rgb(source, PdfMode::Normal, palette), source);
        assert_eq!(
            rendered_rgb(source, PdfMode::Inverted, palette),
            [245, 215, 165]
        );
        assert_ne!(rendered_rgb(source, PdfMode::Omarchy, palette), source);
    }

    #[test]
    fn pdf_neutrals_are_mapped_between_omarchy_ink_and_paper() {
        let palette = PdfPalette {
            paper: [16, 32, 48],
            ink: [208, 224, 240],
        };

        assert_eq!(themed_rgb([0, 0, 0], palette, false), palette.ink);
        assert_eq!(themed_rgb([255, 255, 255], palette, false), palette.paper);
        assert_eq!(themed_rgb([0, 0, 0], palette, true), palette.paper);
        assert_eq!(themed_rgb([255, 255, 255], palette, true), palette.ink);

        let middle = themed_rgb([128, 128, 128], palette, false);
        for (channel, value) in middle.iter().enumerate() {
            assert!(*value > palette.paper[channel]);
            assert!(*value < palette.ink[channel]);
        }
    }

    #[test]
    fn pdf_chroma_survives_palette_mapping() {
        let palette = PdfPalette {
            paper: [13, 11, 26],
            ink: [196, 192, 224],
        };
        let red = themed_rgb([230, 30, 40], palette, false);
        assert!(
            red[0] > red[1] + 40,
            "red detail must remain recognizably red"
        );
        assert!(red[0] > red[2] + 30, "red detail must not become neutral");
    }

    #[test]
    fn dark_pdf_palette_lifts_colored_headings_without_losing_hue() {
        let palette = PdfPalette {
            paper: [13, 11, 26],
            ink: [196, 192, 224],
        };
        let blue = themed_rgb([31, 78, 121], palette, false);
        let luma =
            (0.2126 * blue[0] as f64 + 0.7152 * blue[1] as f64 + 0.0722 * blue[2] as f64) / 255.0;
        assert!(luma >= 0.5, "dark-theme headings need readable contrast");
        assert!(blue[2] > blue[0] + 25, "blue heading hue must survive");
    }

    #[test]
    fn displayed_link_points_follow_page_rotation() {
        let path = crate::doc::write_link_test_pdf();
        let doc = OpenDoc::open(&path, None).unwrap();
        for (rotation, point) in [
            (0, (100.0, 90.0)),
            (90, (702.0, 100.0)),
            (180, (512.0, 702.0)),
            (270, (90.0, 512.0)),
        ] {
            let actual = display_point_to_document(&doc, 0, rotation, point.0, point.1);
            assert!((actual.0 - 100.0).abs() < 0.01, "rotation {rotation}");
            assert!((actual.1 - 90.0).abs() < 0.01, "rotation {rotation}");
            assert_eq!(
                doc.link_at(0, actual.0, actual.1),
                Some(crate::doc::DocumentLink::Uri("https://omarchy.org".into())),
                "rotation {rotation}"
            );
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn selection_uses_top_origin_after_each_rotation() {
        let path = crate::doc::write_test_pdf(
            "selection",
            &["BT /F1 20 Tf 72 700 Td (TOPMARK) Tj ET\nBT /F1 20 Tf 72 100 Td (BOTTOMMARK) Tj ET"],
        );
        let doc = OpenDoc::open(&path, None).expect("open test PDF");
        let cases = [
            (
                0,
                Selection {
                    page: 0,
                    x1: 70.0,
                    y1: 70.0,
                    x2: 200.0,
                    y2: 100.0,
                },
            ),
            (
                90,
                Selection {
                    page: 0,
                    x1: 690.0,
                    y1: 70.0,
                    x2: 730.0,
                    y2: 200.0,
                },
            ),
            (
                180,
                Selection {
                    page: 0,
                    x1: 400.0,
                    y1: 680.0,
                    x2: 550.0,
                    y2: 730.0,
                },
            ),
            (
                270,
                Selection {
                    page: 0,
                    x1: 60.0,
                    y1: 400.0,
                    x2: 120.0,
                    y2: 550.0,
                },
            ),
        ];

        for (rotation, selection) in cases {
            assert_eq!(
                selected_text(&doc, rotation, selection).as_deref(),
                Some("TOPMARK"),
                "rotation {rotation}"
            );
        }
        std::fs::remove_file(path).expect("remove test PDF");
    }

    #[test]
    fn search_rectangles_follow_the_render_rotation() {
        let path = crate::doc::write_test_pdf(
            "search-rotation",
            &["BT /F1 20 Tf 72 700 Td (TOPMARK) Tj ET"],
        );
        let doc = OpenDoc::open(&path, None).expect("open test PDF");
        let mut rect = Rectangle::new();
        rect.set_x1(72.0);
        rect.set_y1(695.0);
        rect.set_x2(172.0);
        rect.set_y2(715.0);
        let geom = PageGeom {
            index: 0,
            x: 0.0,
            y: 0.0,
            w: 612.0,
            h: 792.0,
            scale: 1.0,
        };
        let cases = [
            (0, (72.0, 77.0, 100.0, 20.0)),
            (90, (695.0, 72.0, 20.0, 100.0)),
            (180, (440.0, 695.0, 100.0, 20.0)),
            (270, (77.0, 440.0, 20.0, 100.0)),
        ];
        for (rotation, expected) in cases {
            let got = pdf_rect_to_view(&geom, &doc, rotation, &rect);
            for (got, expected) in [got.0, got.1, got.2, got.3]
                .into_iter()
                .zip([expected.0, expected.1, expected.2, expected.3])
            {
                assert!(
                    (got - expected).abs() < 0.01,
                    "rotation {rotation}: {got} != {expected}"
                );
            }
        }
        std::fs::remove_file(path).expect("remove test PDF");
    }

    fn stacked_pages(count: i32, height: f64) -> Vec<PageGeom> {
        (0..count)
            .map(|index| PageGeom {
                index,
                x: 0.0,
                y: 16.0 + index as f64 * (height + 16.0),
                w: 200.0,
                h: height,
                scale: 1.0,
            })
            .collect()
    }

    #[test]
    #[ignore = "writes visual PDF palette samples; set LECTOR_VISUAL_PDF and LECTOR_VISUAL_DIR"]
    fn visual_pdf_palette_samples() {
        use std::fs::File;
        use std::path::PathBuf;

        let path = PathBuf::from(std::env::var_os("LECTOR_VISUAL_PDF").expect("PDF path"));
        let output = PathBuf::from(std::env::var_os("LECTOR_VISUAL_DIR").expect("output dir"));
        std::fs::create_dir_all(&output).expect("create visual output directory");
        let doc = OpenDoc::open(&path, None).expect("open visual PDF");
        let (page_w, page_h) = doc.size(0, 0);
        let scale = 900.0 / page_w.max(1.0);
        let geom = PageGeom {
            index: 0,
            x: 0.0,
            y: 0.0,
            w: page_w * scale,
            h: page_h * scale,
            scale,
        };
        for (name, palette) in [
            (
                "miroir-void",
                PdfPalette {
                    paper: [0x0d, 0x0b, 0x1a],
                    ink: [0xc4, 0xc0, 0xe0],
                },
            ),
            (
                "catppuccin",
                PdfPalette {
                    paper: [0x1e, 0x1e, 0x2e],
                    ink: [0xcd, 0xd6, 0xf4],
                },
            ),
            (
                "flexoki-light",
                PdfPalette {
                    paper: [0xff, 0xfc, 0xf0],
                    ink: [0x10, 0x0f, 0x0f],
                },
            ),
        ] {
            let surface =
                raster_page(&doc, &geom, 0, PdfMode::Omarchy, palette, 1.0).expect("raster");
            let path = output.join(format!("{name}.png"));
            let mut file = File::create(&path).expect("create preview");
            surface.write_to_png(&mut file).expect("write preview");
            eprintln!("{}", path.display());
        }
    }

    #[test]
    fn only_pages_near_the_viewport_are_considered_visible() {
        let pages = stacked_pages(40, 800.0);
        assert_eq!(pages_intersecting_view(&pages, 0.0, 700.0, 0.0), vec![0]);
        assert_eq!(pages_intersecting_view(&pages, 2500.0, 700.0, 0.0), vec![3]);
        assert_eq!(
            pages_intersecting_view(&pages, 2500.0, 700.0, 400.0),
            vec![2, 3, 4]
        );
        assert!(pages_intersecting_view(&pages, 0.0, 700.0, 0.0).len() < 5);
        assert!(pages_intersecting_view(&[], 0.0, 700.0, 100.0).is_empty());
    }

    #[test]
    fn page_raster_cache_can_show_the_previous_scale_while_replacement_renders() {
        let mut cache = RasterCache::new(2);
        let key = RasterKey {
            index: 7,
            width: 100,
            height: 200,
            rotation: 0,
            pdf_mode: PdfMode::Normal,
            palette: PdfPalette::default(),
        };
        let surface = ImageSurface::create(Format::ARgb32, 100, 200).expect("surface");
        cache.insert(key, surface);

        let fallback = cache
            .get_page(7)
            .expect("old-scale page raster should remain available");
        assert_eq!((fallback.width(), fallback.height()), (100, 200));
        assert!(cache.get_page(8).is_none());
    }

    #[test]
    fn page_raster_cache_keeps_a_bounded_lru() {
        let mut cache = RasterCache::new(2);
        let a = RasterKey {
            index: 0,
            width: 10,
            height: 10,
            rotation: 0,
            pdf_mode: PdfMode::Normal,
            palette: PdfPalette::default(),
        };
        let b = RasterKey { index: 1, ..a };
        let c = RasterKey { index: 2, ..a };
        let surf = || ImageSurface::create(Format::ARgb32, 10, 10).expect("surface");
        cache.insert(a, surf());
        cache.insert(b, surf());
        assert!(cache.get(&a).is_some());
        cache.insert(c, surf());
        assert!(
            cache.get(&b).is_none(),
            "the least-recently used raster must be dropped"
        );
        assert!(cache.get(&a).is_some());
        assert!(cache.get(&c).is_some());
        cache.clear();
        assert!(cache.get(&b).is_none());
    }

    #[test]
    fn page_rasters_match_device_pixel_ratio() {
        let geom = PageGeom {
            index: 0,
            x: 0.0,
            y: 0.0,
            w: 100.5,
            h: 200.0,
            scale: 1.0,
        };
        let one = RasterKey::from_geom(&geom, 0, PdfMode::Normal, PdfPalette::default(), 1.0);
        assert_eq!((one.width, one.height), (101, 200));
        let two = RasterKey::from_geom(&geom, 0, PdfMode::Normal, PdfPalette::default(), 2.0);
        assert_eq!((two.width, two.height), (201, 400));
        assert_ne!(one, two);
    }

    #[test]
    fn sidebar_thumbnails_raster_at_css_width_times_device_scale() {
        let (w1, h1) = thumbnail_pixel_size(612.0, 792.0, 180.0, 1.0);
        assert_eq!((w1, h1), (180, 233));
        let (w2, h2) = thumbnail_pixel_size(612.0, 792.0, 180.0, 2.0);
        assert_eq!((w2, h2), (360, 466));
        assert!(
            w2 >= 2 * 140,
            "2× thumbs must beat the old 140px CSS raster"
        );
        assert_eq!(thumbnail_css_size(612.0, 792.0, 180.0), (180, 233));
    }

    #[test]
    fn outline_navigation_uses_layout_y_not_every_widget_bound() {
        let pages = stacked_pages(40, 800.0);
        assert_eq!(page_at_content_y(&pages, 0.0), 0);
        assert_eq!(page_at_content_y(&pages, 2500.0), 3);
        assert_eq!(page_scroll_y(&pages, 3), (pages[3].y - 8.0).max(0.0));
        assert_eq!(page_scroll_y(&pages, 99), 0.0);
    }

    #[test]
    fn a_stale_raster_idle_must_not_drop_the_live_queue() {
        assert!(raster_idle_is_stale(1, 2));
        assert!(!raster_idle_is_stale(2, 2));
        assert!(!raster_cache_needs_reset(
            1.0,
            1.0,
            0,
            0,
            PdfMode::Normal,
            PdfMode::Normal
        ));
        assert!(raster_cache_needs_reset(
            1.0,
            1.25,
            0,
            0,
            PdfMode::Normal,
            PdfMode::Normal
        ));
        assert!(raster_cache_needs_reset(
            1.0,
            1.0,
            0,
            90,
            PdfMode::Normal,
            PdfMode::Normal
        ));
        assert!(raster_cache_needs_reset(
            1.0,
            1.0,
            0,
            0,
            PdfMode::Normal,
            PdfMode::Omarchy
        ));
        assert!(!raster_cache_needs_clear(
            1.0,
            1.5,
            0,
            0,
            PdfMode::Normal,
            PdfMode::Normal
        ));
        assert!(raster_cache_needs_clear(
            1.0,
            1.0,
            0,
            90,
            PdfMode::Normal,
            PdfMode::Normal
        ));
    }

    #[test]
    fn annotation_render_bounds_scale_after_rotation() {
        let rect = DocumentRect {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
        };
        let expected = [
            (0, (20.0, 40.0, 60.0, 80.0)),
            (90, (280.0, 20.0, 80.0, 60.0)),
            (180, (120.0, 280.0, 60.0, 80.0)),
            (270, (40.0, 120.0, 80.0, 60.0)),
        ];
        for (rotation, (x, y, width, height)) in expected {
            assert_eq!(
                annotation_render_bounds(rect, 100.0, 200.0, rotation, 2.0),
                DocumentRect {
                    x,
                    y,
                    width,
                    height,
                },
                "render bounds at {rotation}°"
            );
        }
    }

    #[test]
    fn highlight_paints_over_an_existing_page_raster() {
        let mut surface = ImageSurface::create(Format::ARgb32, 100, 100).unwrap();
        {
            let cr = Context::new(&surface).unwrap();
            cr.set_source_rgb(1.0, 1.0, 1.0);
            cr.paint().unwrap();
            let annotation = Annotation {
                id: "paint".into(),
                page: 0,
                rect: DocumentRect {
                    x: 10.0,
                    y: 20.0,
                    width: 30.0,
                    height: 10.0,
                },
                kind: AnnotationKind::Highlight,
            };
            draw_annotations(
                &cr,
                AnnotationDrawing {
                    annotations: &[annotation],
                    page: 0,
                    page_width: 100.0,
                    page_height: 100.0,
                    rotation: 0,
                    scale: 1.0,
                    targeted: None,
                },
            );
        }
        surface.flush();
        let stride = surface.stride() as usize;
        let data = surface.data().unwrap();
        let inside = &data[25 * stride + 15 * 4..25 * stride + 15 * 4 + 4];
        let outside = &data[5 * stride + 5 * 4..5 * stride + 5 * 4 + 4];
        assert_ne!(
            inside, outside,
            "highlight must alter the cached page pixels"
        );
        assert_eq!(outside, &[255, 255, 255, 255]);
    }
}
