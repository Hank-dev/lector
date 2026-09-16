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

impl OpenDoc {
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
