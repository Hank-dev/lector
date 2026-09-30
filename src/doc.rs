use std::collections::HashSet;
use std::ffi::CStr;
use std::path::{Path, PathBuf};

use poppler::ffi::{
    PopplerActionAny, PopplerActionGotoDest, PopplerActionUri, PopplerDest,
    POPPLER_ACTION_GOTO_DEST, POPPLER_ACTION_URI,
};
use poppler::{Document, IndexIter, Page};

use crate::theme::path_uri;

#[derive(Clone, Debug)]
pub struct OutlineItem {
    pub title: String,
    pub page: Option<i32>,
    pub depth: u32,
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn outline_item_has_children(items: &[OutlineItem], index: usize) -> bool {
    let Some(item) = items.get(index) else {
        return false;
    };
    items
        .get(index + 1)
        .is_some_and(|next| next.depth > item.depth)
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn visible_outline_indices(items: &[OutlineItem], collapsed: &HashSet<usize>) -> Vec<usize> {
    let mut skip_deeper_than = None;
    let mut visible = Vec::new();
    for (index, item) in items.iter().enumerate() {
        if let Some(depth) = skip_deeper_than {
            if item.depth > depth {
                continue;
            }
            skip_deeper_than = None;
        }
        visible.push(index);
        if collapsed.contains(&index) {
            skip_deeper_than = Some(item.depth);
        }
    }
    visible
}

pub fn initially_collapsed_outline(items: &[OutlineItem]) -> HashSet<usize> {
    items
        .iter()
        .enumerate()
        .filter(|(index, _)| outline_item_has_children(items, *index))
        .map(|(index, _)| index)
        .collect()
}

#[derive(Clone, Debug)]
pub struct PageSize {
    pub w: f64,
    pub h: f64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocumentLink {
    GotoPage(i32),
    Uri(String),
}

pub struct OpenDoc {
    pub path: PathBuf,
    #[allow(dead_code)]
    pub uri: String,
    pub document: Document,
    pub n_pages: i32,
    pub sizes: Vec<PageSize>,
    pub labels: Vec<String>,
    pub outline: Vec<OutlineItem>,
    pub title: String,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub producer: Option<String>,
    pub creator: Option<String>,
    pub pdf_version: Option<String>,
    pub password: Option<String>,
}

#[derive(Debug)]
pub enum OpenError {
    Encrypted,
    Other(String),
}

pub fn page_index_from_input(input: &str, labels: &[String]) -> Option<i32> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    if let Some(index) = labels
        .iter()
        .position(|label| label.eq_ignore_ascii_case(input))
    {
        return Some(index as i32);
    }
    input
        .parse::<i32>()
        .ok()
        .filter(|number| *number >= 1 && *number <= labels.len() as i32)
        .map(|number| number - 1)
}

pub fn display_page_label(labels: &[String], index: i32) -> String {
    labels
        .get(index as usize)
        .filter(|label| !label.is_empty())
        .cloned()
        .unwrap_or_else(|| (index + 1).to_string())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
}

impl SearchOptions {
    pub fn flags(self) -> poppler::FindFlags {
        let mut flags = poppler::FindFlags::DEFAULT;
        if self.case_sensitive {
            flags |= poppler::FindFlags::CASE_SENSITIVE;
        }
        if self.whole_word {
            flags |= poppler::FindFlags::WHOLE_WORDS_ONLY;
        }
        flags
    }
}

pub const INK_STROKE_WIDTH: f64 = 1.6;
pub const INK_MAX_WIDTH: f64 = 3.0;
pub const INK_BLACK: (f64, f64, f64) = (0.10, 0.10, 0.10);
pub const FREE_TEXT_SIZE: f64 = 12.0;
const INK_MIN_LENGTH: f64 = 2.0;
const INK_MIN_SPACING: f64 = 0.6;
const INK_SAMPLE_STEP: f64 = 0.8;
const INK_MAX_POINTS: usize = 2000;

/// Pen colour and width of a handwritten drawing, in PDF points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InkStyle {
    pub rgb: (f64, f64, f64),
    pub width: f64,
}

impl Default for InkStyle {
    fn default() -> Self {
        Self {
            rgb: INK_BLACK,
            width: INK_STROKE_WIDTH,
        }
    }
}

/// An embedded free-text answer in top-origin document coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct FreeTextSpec {
    pub rect: crate::annotations::DocumentRect,
    pub text: String,
    pub size: f64,
    pub rgb: (f64, f64, f64),
}

/// Drops non-finite and near-duplicate input points and rejects dots.
pub fn simplify_ink_stroke(points: &[(f64, f64)]) -> Option<Vec<(f64, f64)>> {
    let mut simplified = Vec::new();
    for &(x, y) in points {
        if !x.is_finite() || !y.is_finite() {
            continue;
        }
        if let Some(&(last_x, last_y)) = simplified.last() {
            let dx = x - last_x;
            let dy = y - last_y;
            if dx * dx + dy * dy < INK_MIN_SPACING * INK_MIN_SPACING {
                if simplified.len() > 1 {
                    if let Some(last) = simplified.last_mut() {
                        *last = (x, y);
                    }
                }
                continue;
            }
        }
        simplified.push((x, y));
    }
    if simplified.len() < 2 || polyline_length(&simplified) < INK_MIN_LENGTH {
        None
    } else {
        Some(simplified)
    }
}

/// Interpolates a curve through every point (chord-length Catmull-Rom as
/// cubic Béziers), sampled finely enough that any viewer draws it smoothly.
pub fn smooth_polyline(points: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let n = points.len();
    if n < 3 {
        return points.to_vec();
    }
    let at = |i: isize| points[i.clamp(0, n as isize - 1) as usize];
    let dist = |a: (f64, f64), b: (f64, f64)| ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
    let mut out = Vec::with_capacity(n * 3);
    out.push(points[0]);
    for i in 0..n as isize - 1 {
        let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        let (d01, d12, d23) = (dist(p0, p1), dist(p1, p2), dist(p2, p3));
        let handle = |a: (f64, f64), b: (f64, f64), span: f64| {
            if span <= f64::EPSILON {
                (0.0, 0.0)
            } else {
                let k = d12 / (3.0 * span);
                ((b.0 - a.0) * k, (b.1 - a.1) * k)
            }
        };
        let h1 = handle(p0, p2, d01 + d12);
        let h2 = handle(p1, p3, d12 + d23);
        let c1 = (p1.0 + h1.0, p1.1 + h1.1);
        let c2 = (p2.0 - h2.0, p2.1 - h2.1);
        let steps = (d12 / INK_SAMPLE_STEP).ceil().clamp(1.0, 24.0) as usize;
        for s in 1..=steps {
            let t = s as f64 / steps as f64;
            let u = 1.0 - t;
            let bez = |a: f64, b: f64, c: f64, d: f64| {
                u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d
            };
            out.push((bez(p1.0, c1.0, c2.0, p2.0), bez(p1.1, c1.1, c2.1, p2.1)));
        }
    }
    out
}

/// The stored form of one pen stroke: cleaned, smoothed, bounded in size.
pub fn smooth_ink_stroke(points: &[(f64, f64)]) -> Option<Vec<(f64, f64)>> {
    let mut smooth = smooth_polyline(&simplify_ink_stroke(points)?);
    if smooth.len() > INK_MAX_POINTS {
        let stride = smooth.len().div_ceil(INK_MAX_POINTS);
        let last = *smooth.last()?;
        smooth = smooth.iter().step_by(stride).copied().collect();
        if smooth.last() != Some(&last) {
            smooth.push(last);
        }
    }
    Some(smooth)
}

fn polyline_length(points: &[(f64, f64)]) -> f64 {
    points
        .windows(2)
        .map(|pair| {
            let dx = pair[1].0 - pair[0].0;
            let dy = pair[1].1 - pair[0].1;
            (dx * dx + dy * dy).sqrt()
        })
        .sum()
}

fn bounds<'a>(points: impl IntoIterator<Item = &'a (f64, f64)>) -> (f64, f64, f64, f64) {
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for &(x, y) in points {
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    (min_x, max_x, min_y, max_y)
}

fn ink_path(points: &[(f64, f64)]) -> poppler::Path {
    use glib::translate::FromGlibPtrFull;
    // poppler_path_new_from_array stores this pointer and poppler_path_free
    // releases it with g_free. It does not copy the array.
    let bytes = std::mem::size_of::<poppler::ffi::PopplerPoint>() * points.len();
    let ptr = unsafe { glib::ffi::g_malloc(bytes) as *mut poppler::ffi::PopplerPoint };
    assert!(!ptr.is_null(), "g_malloc failed");
    for (index, &(x, y)) in points.iter().enumerate() {
        unsafe {
            std::ptr::write(ptr.add(index), poppler::ffi::PopplerPoint { x, y });
        }
    }
    unsafe {
        FromGlibPtrFull::from_glib_full(poppler::ffi::poppler_path_new_from_array(
            ptr,
            points.len(),
        ))
    }
}

/// poppler-rs 0.26 leaves `AnnotInk::set_ink_list` unimplemented.
/// The call copies point data; the paths must stay alive until it returns.
fn set_ink_list(annot: &poppler::AnnotInk, paths: &[poppler::Path]) {
    use glib::translate::ToGlibPtr;
    let stashes = paths
        .iter()
        .map(|path| path.to_glib_none())
        .collect::<Vec<_>>();
    let mut ptrs = stashes.iter().map(|stash| stash.0).collect::<Vec<_>>();
    unsafe {
        poppler::ffi::poppler_annot_ink_set_ink_list(
            annot.to_glib_none().0,
            ptrs.as_mut_ptr(),
            ptrs.len(),
        );
    }
}

fn poppler_color(rgb: (f64, f64, f64)) -> poppler::Color {
    let channel = |value: f64| (value.clamp(0.0, 1.0) * 65535.0).round() as u16;
    let mut color = poppler::Color::new();
    color.set_red(channel(rgb.0));
    color.set_green(channel(rgb.1));
    color.set_blue(channel(rgb.2));
    color
}

fn rgb_of(color: &poppler::Color) -> (f64, f64, f64) {
    (
        color.red() as f64 / 65535.0,
        color.green() as f64 / 65535.0,
        color.blue() as f64 / 65535.0,
    )
}

fn pdf_rect(x1: f64, y1: f64, x2: f64, y2: f64) -> poppler::Rectangle {
    let mut area = poppler::Rectangle::new();
    area.set_x1(x1);
    area.set_y1(y1);
    area.set_x2(x2);
    area.set_y2(y2);
    area
}

impl OpenDoc {
    fn page_annots(&self, index: i32, kind: poppler::AnnotType) -> Vec<poppler::Annot> {
        use glib::translate::{from_glib_none, ToGlibPtr};
        use poppler::prelude::*;
        let Some(page) = self.page(index) else {
            return Vec::new();
        };
        page.annot_mapping()
            .into_iter()
            .filter_map(|mapping| {
                // The mapping owns an annot reference; retain our own before dropping it.
                let annot: poppler::Annot = unsafe {
                    let ptr: *mut poppler::ffi::PopplerAnnotMapping = mapping.to_glib_none().0;
                    from_glib_none((*ptr).annot)
                };
                (annot.annot_type() == kind).then_some(annot)
            })
            .collect()
    }

    fn all_annots(&self, kind: poppler::AnnotType) -> Vec<(i32, poppler::Annot)> {
        (0..self.n_pages)
            .flat_map(|index| {
                self.page_annots(index, kind)
                    .into_iter()
                    .map(move |annot| (index, annot))
            })
            .collect()
    }

    pub fn free_text_annotations(&self) -> Vec<(i32, poppler::Annot)> {
        self.all_annots(poppler::AnnotType::FreeText)
    }

    pub fn remove_fill_text(&self, index: i32, annot: &poppler::Annot) -> Result<(), String> {
        use poppler::prelude::*;
        if annot.annot_type() != poppler::AnnotType::FreeText || annot.page_index() != index {
            return Err("Text annotation not found on this page".into());
        }
        let page = self.page(index).ok_or("Page not found")?;
        page.remove_annot(annot);
        Ok(())
    }

    #[cfg(test)]
    pub fn add_fill_text(
        &self,
        index: i32,
        rect: crate::annotations::DocumentRect,
        text: &str,
    ) -> Result<(), String> {
        self.add_free_text(
            index,
            &FreeTextSpec {
                rect,
                text: text.to_string(),
                size: FREE_TEXT_SIZE,
                rgb: INK_BLACK,
            },
        )
        .map(|_| ())
    }

    pub fn add_free_text(&self, index: i32, spec: &FreeTextSpec) -> Result<poppler::Annot, String> {
        use glib::object::Cast;
        use poppler::prelude::*;
        let page = self.page(index).ok_or("Page not found")?;
        let (_, height) = page.size();
        let rect = spec.rect;
        let mut area = pdf_rect(
            rect.x,
            height - rect.y - rect.height,
            rect.x + rect.width,
            height - rect.y,
        );
        let annotation = poppler::AnnotFreeText::new(&self.document, &mut area);
        annotation.set_contents(&spec.text);
        annotation.set_border_width(0.0);
        annotation.set_color(None);
        annotation.set_flags(poppler::AnnotFlag::PRINT);
        let mut font = poppler::FontDescription::new("Helvetica");
        unsafe {
            let ptr = glib::translate::ToGlibPtr::<*const poppler::ffi::PopplerFontDescription>::to_glib_none(&font).0
                as *mut poppler::ffi::PopplerFontDescription;
            (*ptr).size_pt = spec.size;
        }
        annotation.set_font_desc(&mut font);
        annotation.set_font_color(&mut poppler_color(spec.rgb));
        page.add_annot(&annotation);
        Ok(annotation.upcast())
    }

    /// Reads back what Lector needs to edit or restore an embedded answer.
    pub fn free_text_spec(&self, index: i32, annot: &poppler::Annot) -> FreeTextSpec {
        use glib::object::Cast;
        use poppler::prelude::*;
        let (_, height) = self.size(index, 0);
        let area = annot.rectangle();
        let (x1, x2) = (area.x1().min(area.x2()), area.x1().max(area.x2()));
        let (y1, y2) = (area.y1().min(area.y2()), area.y1().max(area.y2()));
        let free_text = annot.downcast_ref::<poppler::AnnotFreeText>();
        let size = free_text
            .and_then(|annot| annot.font_desc())
            .map(|font| unsafe {
                let ptr = glib::translate::ToGlibPtr::<
                    *const poppler::ffi::PopplerFontDescription,
                >::to_glib_none(&font)
                .0;
                (*ptr).size_pt
            })
            .filter(|size| size.is_finite() && *size > 0.0)
            .unwrap_or(FREE_TEXT_SIZE);
        let rgb = free_text
            .and_then(|annot| annot.font_color())
            .map(|color| rgb_of(&color))
            .unwrap_or((0.0, 0.0, 0.0));
        FreeTextSpec {
            rect: crate::annotations::DocumentRect {
                x: x1,
                y: height - y2,
                width: x2 - x1,
                height: y2 - y1,
            },
            text: annot.contents().map(|s| s.to_string()).unwrap_or_default(),
            size,
            rgb,
        }
    }

    /// The topmost embedded answer under a top-origin document point.
    pub fn free_text_at(
        &self,
        index: i32,
        x: f64,
        y: f64,
    ) -> Option<(poppler::Annot, FreeTextSpec)> {
        self.page_annots(index, poppler::AnnotType::FreeText)
            .into_iter()
            .rev()
            .map(|annot| {
                let spec = self.free_text_spec(index, &annot);
                (annot, spec)
            })
            .find(|(_, spec)| {
                let r = spec.rect;
                x >= r.x && x <= r.x + r.width && y >= r.y && y <= r.y + r.height
            })
    }

    /// The answer that `spec` describes, for undo after other edits moved
    /// Poppler's objects around.
    pub fn find_free_text(&self, index: i32, spec: &FreeTextSpec) -> Option<poppler::Annot> {
        self.page_annots(index, poppler::AnnotType::FreeText)
            .into_iter()
            .rev()
            .find(|annot| {
                let found = self.free_text_spec(index, annot);
                found.text == spec.text
                    && (found.size - spec.size).abs() < 0.01
                    && (found.rect.x - spec.rect.x).abs() < 0.05
                    && (found.rect.y - spec.rect.y).abs() < 0.05
            })
    }

    pub fn free_text_rects(&self, index: i32) -> Vec<crate::annotations::DocumentRect> {
        self.page_annots(index, poppler::AnnotType::FreeText)
            .iter()
            .map(|annot| self.free_text_spec(index, annot).rect)
            .collect()
    }

    pub fn ink_annotations(&self) -> Vec<(i32, poppler::Annot)> {
        self.all_annots(poppler::AnnotType::Ink)
    }

    /// Strokes of one ink annotation in PDF (bottom-origin) coordinates.
    fn ink_pdf_paths(annot: &poppler::AnnotInk) -> Vec<Vec<(f64, f64)>> {
        annot
            .ink_list()
            .into_iter()
            .map(|path| {
                path.points()
                    .into_iter()
                    .map(|point| (point.x(), point.y()))
                    .collect::<Vec<_>>()
            })
            .filter(|points| !points.is_empty())
            .collect()
    }

    pub fn ink_stroke_count(annot: &poppler::Annot) -> usize {
        use glib::object::Cast;
        annot
            .downcast_ref::<poppler::AnnotInk>()
            .map(|ink| Self::ink_pdf_paths(ink).len())
            .unwrap_or(0)
    }

    /// Ink colour of each stroke returned by [`Self::ink_document_points`].
    pub fn ink_document_strokes(&self, index: i32) -> Vec<(InkStyle, Vec<(f64, f64)>)> {
        use glib::object::Cast;
        use poppler::prelude::*;
        let (_, height) = self.size(index, 0);
        let mut strokes = Vec::new();
        for annot in self.page_annots(index, poppler::AnnotType::Ink) {
            let mut width = INK_STROKE_WIDTH;
            let has_width = unsafe {
                poppler::ffi::poppler_annot_get_border_width(
                    glib::translate::ToGlibPtr::to_glib_none(&annot).0,
                    &mut width,
                )
            };
            let style = InkStyle {
                rgb: annot.color().map(|c| rgb_of(&c)).unwrap_or(INK_BLACK),
                width: if has_width != 0 && width > 0.0 {
                    width
                } else {
                    INK_STROKE_WIDTH
                },
            };
            let Ok(ink) = annot.downcast::<poppler::AnnotInk>() else {
                continue;
            };
            for path in Self::ink_pdf_paths(&ink) {
                strokes.push((
                    style,
                    path.into_iter().map(|(x, y)| (x, height - y)).collect(),
                ));
            }
        }
        strokes
    }

    #[cfg(test)]
    pub fn ink_document_points(&self, index: i32) -> Vec<Vec<(f64, f64)>> {
        self.ink_document_strokes(index)
            .into_iter()
            .map(|(_, points)| points)
            .collect()
    }

    pub fn ink_display_strokes(
        &self,
        index: i32,
        rotation: i32,
    ) -> Vec<(InkStyle, Vec<(f64, f64)>)> {
        let (page_width, page_height) = self.size(index, 0);
        self.ink_document_strokes(index)
            .into_iter()
            .map(|(style, stroke)| {
                let points = stroke
                    .into_iter()
                    .map(|(x, y)| {
                        crate::annotations::document_point_to_display(
                            x,
                            y,
                            page_width,
                            page_height,
                            rotation,
                        )
                    })
                    .collect();
                (style, points)
            })
            .collect()
    }

    pub fn remove_ink(&self, index: i32, annot: &poppler::Annot) -> Result<(), String> {
        use poppler::prelude::*;
        if annot.annot_type() != poppler::AnnotType::Ink || annot.page_index() != index {
            return Err("Drawing not found on this page".into());
        }
        let page = self.page(index).ok_or("Page not found")?;
        page.remove_annot(annot);
        Ok(())
    }

    /// Smooths a top-origin stroke and converts it to clamped PDF coordinates.
    fn ink_pdf_stroke(&self, index: i32, points: &[(f64, f64)]) -> Result<Vec<(f64, f64)>, String> {
        let points = smooth_ink_stroke(points).ok_or("Stroke is too short")?;
        let (width, height) = self.size(index, 0);
        let pdf_points = points
            .iter()
            .map(|(x, y)| (x.clamp(0.0, width), height - y.clamp(0.0, height)))
            .collect::<Vec<_>>();
        if polyline_length(&pdf_points) < INK_MIN_LENGTH {
            return Err("Stroke is too short".into());
        }
        Ok(pdf_points)
    }

    /// Writes all strokes and a Rect that covers them. The Rect must be
    /// updated first: other viewers scale the appearance into it.
    fn set_ink_paths(&self, index: i32, annot: &poppler::AnnotInk, paths: &[Vec<(f64, f64)>]) {
        use poppler::prelude::*;
        let (width, height) = self.size(index, 0);
        let pad = INK_MAX_WIDTH + 1.0;
        let (min_x, max_x, min_y, max_y) = bounds(paths.iter().flatten());
        let mut x1 = (min_x - pad).max(0.0);
        let mut y1 = (min_y - pad).max(0.0);
        let mut x2 = (max_x + pad).min(width);
        let mut y2 = (max_y + pad).min(height);
        if x2 - x1 < 1.0 {
            x2 = (x1 + 1.0).min(width);
            x1 = (x2 - 1.0).max(0.0);
        }
        if y2 - y1 < 1.0 {
            y2 = (y1 + 1.0).min(height);
            y1 = (y2 - 1.0).max(0.0);
        }
        annot.set_rectangle(&mut pdf_rect(x1, y1, x2, y2));
        let paths = paths.iter().map(|path| ink_path(path)).collect::<Vec<_>>();
        set_ink_list(annot, &paths);
    }

    #[cfg(test)]
    pub fn add_ink_stroke(&self, index: i32, points: &[(f64, f64)]) -> Result<(), String> {
        self.add_ink_drawing(index, points, InkStyle::default())
            .map(|_| ())
    }

    /// Starts a new drawing (one ink annotation) with its first stroke.
    pub fn add_ink_drawing(
        &self,
        index: i32,
        points: &[(f64, f64)],
        style: InkStyle,
    ) -> Result<poppler::AnnotInk, String> {
        use poppler::prelude::*;
        let stroke = self.ink_pdf_stroke(index, points)?;
        let page = self.page(index).ok_or("Page not found")?;
        let (min_x, max_x, min_y, max_y) = bounds(&stroke);
        let annotation =
            poppler::AnnotInk::new(&self.document, &pdf_rect(min_x, min_y, max_x, max_y));
        annotation.set_color(Some(&poppler_color(style.rgb)));
        annotation.set_border_width(style.width.clamp(0.25, INK_MAX_WIDTH));
        annotation.set_opacity(1.0);
        annotation.set_flags(poppler::AnnotFlag::PRINT);
        page.add_annot(&annotation);
        self.set_ink_paths(index, &annotation, &[stroke]);
        Ok(annotation)
    }

    /// Adds a stroke to an existing drawing so a signature stays one object.
    pub fn append_ink_stroke(
        &self,
        index: i32,
        annot: &poppler::AnnotInk,
        points: &[(f64, f64)],
    ) -> Result<(), String> {
        let stroke = self.ink_pdf_stroke(index, points)?;
        let mut paths = Self::ink_pdf_paths(annot);
        paths.push(stroke);
        self.set_ink_paths(index, annot, &paths);
        Ok(())
    }

    /// Removes the newest stroke; returns true when the drawing is now gone.
    pub fn pop_ink_stroke(&self, index: i32, annot: &poppler::AnnotInk) -> Result<bool, String> {
        use glib::object::Cast;
        let mut paths = Self::ink_pdf_paths(annot);
        paths.pop();
        if paths.is_empty() {
            self.remove_ink(index, annot.upcast_ref())?;
            return Ok(true);
        }
        self.set_ink_paths(index, annot, &paths);
        Ok(false)
    }

    pub fn save_filled_copy(&self, path: &Path) -> Result<(), String> {
        self.document
            .save(&path_uri(path))
            .map_err(|e| e.to_string())
    }

    pub fn open(path: &Path, password: Option<&str>) -> Result<Self, OpenError> {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let uri = path_uri(&path);
        Self::from_uri_path(uri, path, password)
    }

    pub fn open_uri(uri: &str, password: Option<&str>) -> Result<Self, OpenError> {
        let path = glib::filename_from_uri(uri)
            .ok()
            .map(|(p, _)| p)
            .unwrap_or_else(|| PathBuf::from(uri));
        Self::from_uri_path(uri.to_string(), path, password)
    }

    fn from_uri_path(
        uri: String,
        path: PathBuf,
        password: Option<&str>,
    ) -> Result<Self, OpenError> {
        let document = Document::from_file(&uri, password).map_err(|e| {
            if e.matches(poppler::Error::Encrypted) {
                OpenError::Encrypted
            } else {
                OpenError::Other(e.to_string())
            }
        })?;

        let n_pages = document.n_pages();
        let mut sizes = Vec::with_capacity(n_pages as usize);
        let mut labels = Vec::with_capacity(n_pages as usize);
        for i in 0..n_pages {
            if let Some(page) = document.page(i) {
                let (w, h) = page.size();
                sizes.push(PageSize { w, h });
                let label = page
                    .label()
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| (i + 1).to_string());
                labels.push(label);
            } else {
                sizes.push(PageSize { w: 612.0, h: 792.0 });
                labels.push((i + 1).to_string());
            }
        }

        let outline = collect_outline(&document);
        let title = document
            .title()
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                path.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Document".into())
            });

        Ok(Self {
            path,
            uri,
            n_pages,
            sizes,
            labels,
            outline,
            title,
            author: document.author().map(|s| s.to_string()),
            subject: document.subject().map(|s| s.to_string()),
            producer: document.producer().map(|s| s.to_string()),
            creator: document.creator().map(|s| s.to_string()),
            pdf_version: document.pdf_version_string().map(|s| s.to_string()),
            password: password.map(str::to_string),
            document,
        })
    }

    pub fn page(&self, index: i32) -> Option<Page> {
        self.document.page(index)
    }

    pub fn size(&self, index: i32, rotation: i32) -> (f64, f64) {
        let Some(s) = self.sizes.get(index as usize) else {
            return (612.0, 792.0);
        };
        if rotation % 180 == 0 {
            (s.w, s.h)
        } else {
            (s.h, s.w)
        }
    }

    pub fn find_page(
        &self,
        index: i32,
        query: &str,
        options: SearchOptions,
    ) -> Vec<(i32, poppler::Rectangle)> {
        if query.is_empty() {
            return Vec::new();
        }
        let Some(page) = self.page(index) else {
            return Vec::new();
        };
        page.find_text_with_options(query, options.flags())
            .into_iter()
            .map(|rect| (index, rect))
            .collect()
    }

    pub fn link_at(&self, index: i32, x: f64, y: f64) -> Option<DocumentLink> {
        use glib::translate::ToGlibPtr;

        let page = self.page(index)?;
        let (_, height) = self.size(index, 0);
        let pdf_y = height - y;
        for mapping in page.link_mapping() {
            unsafe {
                let raw_ptr: *mut poppler::ffi::PopplerLinkMapping = mapping.to_glib_none().0;
                let raw = &*raw_ptr;
                let x1 = raw.area.x1.min(raw.area.x2);
                let x2 = raw.area.x1.max(raw.area.x2);
                let y1 = raw.area.y1.min(raw.area.y2);
                let y2 = raw.area.y1.max(raw.area.y2);
                if x < x1 || x > x2 || pdf_y < y1 || pdf_y > y2 || raw.action.is_null() {
                    continue;
                }
                match (*raw.action).type_ {
                    POPPLER_ACTION_GOTO_DEST => {
                        let action = &*(raw.action as *const PopplerActionGotoDest);
                        return dest_page(&self.document, action.dest).map(DocumentLink::GotoPage);
                    }
                    POPPLER_ACTION_URI => {
                        let action = &*(raw.action as *const PopplerActionUri);
                        if !action.uri.is_null() {
                            return Some(DocumentLink::Uri(
                                CStr::from_ptr(action.uri).to_string_lossy().into_owned(),
                            ));
                        }
                    }
                    _ => {}
                }
            }
        }
        None
    }
}

fn collect_outline(doc: &Document) -> Vec<OutlineItem> {
    use glib::translate::{from_glib_full, ToGlibPtr};
    unsafe {
        let raw = poppler::ffi::poppler_index_iter_new(doc.to_glib_none().0);
        if raw.is_null() {
            return Vec::new();
        }
        let mut iter: IndexIter = from_glib_full(raw);
        let mut items = Vec::new();
        collect_iter(doc, &mut iter, 0, &mut items);
        items
    }
}

fn collect_iter(doc: &Document, iter: &mut IndexIter, depth: u32, out: &mut Vec<OutlineItem>) {
    loop {
        let (title, page) = action_info(doc, iter);
        if !title.is_empty() || page.is_some() {
            out.push(OutlineItem {
                title: if title.is_empty() {
                    format!("Page {}", page.unwrap_or(0) + 1)
                } else {
                    title
                },
                page,
                depth,
            });
        }
        if let Some(mut child) = iter.child() {
            collect_iter(doc, &mut child, depth + 1, out);
        }
        if !iter.next() {
            break;
        }
    }
}

fn action_info(doc: &Document, iter: &IndexIter) -> (String, Option<i32>) {
    unsafe {
        let action = poppler::ffi::poppler_index_iter_get_action(iter.as_ptr());
        if action.is_null() {
            return (String::new(), None);
        }
        let any = &*(action as *const PopplerActionAny);
        let title = if any.title.is_null() {
            String::new()
        } else {
            CStr::from_ptr(any.title).to_string_lossy().into_owned()
        };
        let page = if any.type_ == POPPLER_ACTION_GOTO_DEST {
            let goto = &*(action as *const PopplerActionGotoDest);
            dest_page(doc, goto.dest)
        } else {
            None
        };
        poppler::ffi::poppler_action_free(action);
        (title, page)
    }
}

fn dest_page(doc: &Document, dest: *mut PopplerDest) -> Option<i32> {
    if dest.is_null() {
        return None;
    }
    unsafe {
        let d = &*dest;
        if d.page_num > 0 {
            return Some(d.page_num - 1);
        }
        if d.named_dest.is_null() {
            return None;
        }
        let name = CStr::from_ptr(d.named_dest).to_string_lossy();
        let found = doc.find_dest(name.as_ref())?;
        let p = (*found.as_ptr()).page_num;
        if p > 0 {
            Some(p - 1)
        } else {
            None
        }
    }
}

#[cfg(test)]
pub(crate) fn write_test_pdf(name: &str, page_contents: &[&str]) -> PathBuf {
    write_test_pdf_catalog(name, page_contents, "")
}

#[cfg(test)]
pub(crate) fn write_labeled_test_pdf() -> PathBuf {
    write_test_pdf_catalog(
        "labels",
        &[
            "BT /F1 20 Tf 72 700 Td (roman one) Tj ET",
            "BT /F1 20 Tf 72 700 Td (roman two) Tj ET",
            "BT /F1 20 Tf 72 700 Td (arabic one) Tj ET",
        ],
        " /PageLabels << /Nums [ 0 << /S /r >> 2 << /S /D >> ] >>",
    )
}

#[cfg(test)]
fn write_test_pdf_catalog(name: &str, page_contents: &[&str], catalog_extra: &str) -> PathBuf {
    use std::io::Write as _;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_TEST_PDF: AtomicUsize = AtomicUsize::new(0);

    let id = NEXT_TEST_PDF.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("lector-{name}-{}-{id}.pdf", std::process::id()));
    let page_count = page_contents.len();
    let first_page = 3;
    let font = first_page + page_count;
    let first_contents = font + 1;
    let kids = (0..page_count)
        .map(|i| format!("{} 0 R", first_page + i))
        .collect::<Vec<_>>()
        .join(" ");
    let mut objects = vec![
        format!("<< /Type /Catalog /Pages 2 0 R{catalog_extra} >>"),
        format!("<< /Type /Pages /Kids [{kids}] /Count {page_count} >>"),
    ];
    for i in 0..page_count {
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 {font} 0 R >> >> /Contents {} 0 R >>",
            first_contents + i
        ));
    }
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string());
    for contents in page_contents {
        objects.push(format!(
            "<< /Length {} >>\nstream\n{contents}\nendstream",
            contents.len()
        ));
    }

    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0];
    for (i, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        write!(&mut pdf, "{} 0 obj\n{object}\nendobj\n", i + 1).expect("write PDF object");
    }
    let xref = pdf.len();
    write!(&mut pdf, "xref\n0 {}\n0000000000 65535 f \n", offsets.len())
        .expect("write PDF xref header");
    for offset in offsets.iter().skip(1) {
        writeln!(&mut pdf, "{offset:010} 00000 n ").expect("write PDF xref entry");
    }
    write!(
        &mut pdf,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        offsets.len()
    )
    .expect("write PDF trailer");
    std::fs::write(&path, pdf).expect("write test PDF");
    path
}

#[cfg(test)]
pub(crate) fn write_link_test_pdf() -> PathBuf {
    use std::io::Write as _;

    let path = std::env::temp_dir().join(format!(
        "lector-links-{}-{}.pdf",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Annots [5 0 R 6 0 R] >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>",
        "<< /Type /Annot /Subtype /Link /Rect [72 680 180 720] /Border [0 0 0] /A << /S /URI /URI (https://omarchy.org) >> >>",
        "<< /Type /Annot /Subtype /Link /Rect [200 680 300 720] /Border [0 0 0] /Dest [4 0 R /Fit] >>",
    ];
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0];
    for (i, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        write!(&mut pdf, "{} 0 obj\n{object}\nendobj\n", i + 1).unwrap();
    }
    let xref = pdf.len();
    write!(&mut pdf, "xref\n0 {}\n0000000000 65535 f \n", offsets.len()).unwrap();
    for offset in offsets.iter().skip(1) {
        writeln!(&mut pdf, "{offset:010} 00000 n ").unwrap();
    }
    write!(
        &mut pdf,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        offsets.len()
    )
    .unwrap();
    std::fs::write(&path, pdf).unwrap();
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn search_options_honor_case_and_whole_words() {
        let path = write_test_pdf(
            "search-options",
            &[
                "BT /F1 20 Tf 72 700 Td (needle) Tj ET",
                "BT /F1 20 Tf 72 700 Td (Needle) Tj ET",
                "BT /F1 20 Tf 72 700 Td (needles) Tj ET",
            ],
        );
        let doc = OpenDoc::open(&path, None).unwrap();
        let hits = |options: SearchOptions| {
            (0..doc.n_pages)
                .flat_map(|page| doc.find_page(page, "needle", options))
                .map(|(page, _)| page)
                .collect::<Vec<_>>()
        };

        assert_eq!(hits(SearchOptions::default()), vec![0, 1, 2]);
        assert_eq!(
            hits(SearchOptions {
                case_sensitive: true,
                whole_word: false
            }),
            vec![0, 2]
        );
        assert_eq!(
            hits(SearchOptions {
                case_sensitive: false,
                whole_word: true
            }),
            vec![0, 1]
        );
        assert_eq!(
            hits(SearchOptions {
                case_sensitive: true,
                whole_word: true
            }),
            vec![0]
        );

        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn page_links_resolve_external_uris_and_internal_destinations() {
        let path = write_link_test_pdf();
        let doc = OpenDoc::open(&path, None).unwrap();

        assert_eq!(
            doc.link_at(0, 100.0, 90.0),
            Some(DocumentLink::Uri("https://omarchy.org".into()))
        );
        assert_eq!(doc.link_at(0, 240.0, 90.0), Some(DocumentLink::GotoPage(1)));
        assert_eq!(doc.link_at(0, 400.0, 400.0), None);

        std::fs::remove_file(path).unwrap();
    }

    fn sample_outline() -> Vec<OutlineItem> {
        [
            (0, "Chapter 1"),
            (1, "1.1"),
            (2, "1.1.1"),
            (1, "1.2"),
            (0, "Chapter 2"),
            (1, "2.1"),
        ]
        .into_iter()
        .map(|(depth, title)| OutlineItem {
            title: title.into(),
            page: Some(depth as i32),
            depth,
        })
        .collect()
    }

    #[test]
    fn poppler_page_labels_are_loaded_from_the_document() {
        let path = write_labeled_test_pdf();
        let doc = OpenDoc::open(&path, None).unwrap();
        assert_eq!(doc.labels, ["i", "ii", "1"]);
        assert_eq!(page_index_from_input("ii", &doc.labels), Some(1));
        assert_eq!(page_index_from_input("1", &doc.labels), Some(2));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn page_input_prefers_labels_then_one_based_numbers() {
        let labels = ["i", "ii", "1"].map(str::to_string);
        assert_eq!(page_index_from_input("ii", &labels), Some(1));
        assert_eq!(page_index_from_input("1", &labels), Some(2));
        assert_eq!(page_index_from_input("3", &labels), Some(2));
        assert_eq!(page_index_from_input("iv", &labels), None);
        assert_eq!(page_index_from_input("  II ", &labels), Some(1));
        assert_eq!(page_index_from_input("0", &labels), None);
        assert_eq!(display_page_label(&labels, 0), "i");
        assert_eq!(display_page_label(&labels, 2), "1");
        assert_eq!(display_page_label(&labels, 9), "10");
    }

    #[test]
    fn outline_parents_start_collapsed() {
        let items = sample_outline();
        let collapsed = initially_collapsed_outline(&items);
        assert_eq!(visible_outline_indices(&items, &collapsed), vec![0, 4]);

        let mut chapter = collapsed.clone();
        assert!(chapter.remove(&0));
        assert_eq!(visible_outline_indices(&items, &chapter), vec![0, 1, 3, 4]);

        let mut section = chapter;
        assert!(section.remove(&1));
        assert_eq!(
            visible_outline_indices(&items, &section),
            vec![0, 1, 2, 3, 4]
        );
    }

    #[test]
    fn collapsing_a_chapter_hides_its_nested_entries() {
        let items = sample_outline();
        assert!(outline_item_has_children(&items, 0));
        assert!(outline_item_has_children(&items, 1));
        assert!(!outline_item_has_children(&items, 2));
        assert!(outline_item_has_children(&items, 4));
        assert!(!outline_item_has_children(&items, 5));

        let mut collapsed = HashSet::new();
        assert_eq!(
            visible_outline_indices(&items, &collapsed),
            vec![0, 1, 2, 3, 4, 5]
        );

        collapsed.insert(0);
        assert_eq!(visible_outline_indices(&items, &collapsed), vec![0, 4, 5]);

        collapsed.clear();
        collapsed.insert(1);
        assert_eq!(
            visible_outline_indices(&items, &collapsed),
            vec![0, 1, 3, 4, 5]
        );
    }
}
