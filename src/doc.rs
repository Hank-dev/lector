use std::ffi::CStr;
use std::path::{Path, PathBuf};

use poppler::ffi::{
    PopplerActionAny, PopplerActionGotoDest, PopplerDest, POPPLER_ACTION_GOTO_DEST,
};
use poppler::{Document, IndexIter, Page};

use crate::theme::path_uri;

#[derive(Clone, Debug)]
pub struct OutlineItem {
    pub title: String,
    pub page: Option<i32>,
    pub depth: u32,
}

#[derive(Clone, Debug)]
pub struct PageSize {
    pub w: f64,
    pub h: f64,
}

pub struct OpenDoc {
    pub path: PathBuf,
    #[allow(dead_code)]
    pub uri: String,
    pub document: Document,
    pub n_pages: i32,
    pub sizes: Vec<PageSize>,
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
        for i in 0..n_pages {
            if let Some(page) = document.page(i) {
                let (w, h) = page.size();
                sizes.push(PageSize { w, h });
            } else {
                sizes.push(PageSize { w: 612.0, h: 792.0 });
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

    pub fn find_page(&self, index: i32, query: &str) -> Vec<(i32, poppler::Rectangle)> {
        if query.is_empty() {
            return Vec::new();
        }
        let Some(page) = self.page(index) else {
            return Vec::new();
        };
        page.find_text(query)
            .into_iter()
            .map(|rect| (index, rect))
            .collect()
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
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
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
