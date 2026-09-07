use std::f64::consts::PI;

use cairo::{Context, Format, ImageSurface, Operator};
use poppler::{Color, Rectangle, SelectionStyle};

use crate::doc::OpenDoc;

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

pub struct Layout {
    pub pages: Vec<PageGeom>,
    pub scale: f64,
}

pub const PAGE_GAP: f64 = 16.0;
pub const PAGE_PAD: f64 = 16.0;
pub const MAX_ZOOM: f64 = 16.0;

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
    let PageDrawing {
        doc,
        geom,
        rotation,
        invert,
        selection,
        search_hits,
        current_hit,
    } = drawing;
    let local = PageGeom {
        x: 0.0,
        y: 0.0,
        ..*geom
    };
    cr.set_source_rgb(1.0, 1.0, 1.0);
    cr.paint().ok();

    if let Some(page) = doc.page(local.index) {
        cr.save().ok();
        match rotation.rem_euclid(360) {
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
        if invert {
            cr.set_operator(Operator::Difference);
            cr.set_source_rgb(1.0, 1.0, 1.0);
            cr.paint().ok();
        }
        cr.restore().ok();
    }

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

pub fn thumbnail(doc: &OpenDoc, index: i32, max_w: f64, invert: bool) -> Option<ImageSurface> {
    let page = doc.page(index)?;
    let (pw, ph) = page.size();
    let scale = (max_w / pw.max(1.0)).min(1.5);
    let w = (pw * scale).ceil().max(1.0) as i32;
    let h = (ph * scale).ceil().max(1.0) as i32;
    let surf = ImageSurface::create(Format::ARgb32, w, h).ok()?;
    {
        let cr = Context::new(&surf).ok()?;
        cr.set_source_rgb(1.0, 1.0, 1.0);
        cr.paint().ok()?;
        cr.scale(scale, scale);
        page.render(&cr);
        if invert {
            cr.set_operator(Operator::Difference);
            cr.set_source_rgb(1.0, 1.0, 1.0);
            cr.paint().ok()?;
        }
    }
    Some(surf)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
