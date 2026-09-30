//! Generated AcroForm fixtures only; no personal documents.
use crate::doc::OpenDoc;

#[test]
fn free_text_copy_reopens_and_renders_without_changing_original() {
    use poppler::prelude::*;
    let path = crate::doc::write_test_pdf(
        "free-text",
        &["BT /F1 20 Tf 72 700 Td (Underlying text) Tj ET"],
    );
    let original = std::fs::read(&path).unwrap();
    let doc = OpenDoc::open(&path, None).unwrap();
    assert!(doc.page(0).unwrap().form_field_mapping().is_empty());
    let pixels = |doc: &OpenDoc| {
        let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 612, 792).unwrap();
        {
            let cr = cairo::Context::new(&surface).unwrap();
            cr.set_source_rgb(1., 1., 1.);
            cr.paint().unwrap();
            doc.page(0).unwrap().render(&cr);
        }
        let bytes = surface.data().unwrap().to_vec();
        bytes
    };
    let before = pixels(&doc);
    doc.add_fill_text(
        0,
        crate::annotations::DocumentRect {
            x: 72.,
            y: 150.,
            width: 228.,
            height: 80.,
        },
        "Answer one\nAnswer two",
    )
    .unwrap();
    let copy = path.with_extension("filled.pdf");
    doc.save_filled_copy(&copy).unwrap();
    let reopened = OpenDoc::open(&copy, None).unwrap();
    assert!(reopened
        .page(0)
        .unwrap()
        .text()
        .unwrap()
        .contains("Underlying text"));
    let mappings = reopened.page(0).unwrap().annot_mapping();
    assert_eq!(mappings.len(), 1);
    use glib::translate::{from_glib_none, ToGlibPtr};
    let annot: poppler::Annot = unsafe {
        let ptr: *mut poppler::ffi::PopplerAnnotMapping = mappings[0].to_glib_none().0;
        from_glib_none((*ptr).annot)
    };
    assert_eq!(annot.contents().as_deref(), Some("Answer one\nAnswer two"));
    assert!(
        annot.color().is_none(),
        "text annotation must have no box color"
    );
    let after = pixels(&reopened);
    let edge_changes = (151..229)
        .flat_map(|y| [298, 299].into_iter().map(move |x| (y * 612 + x) * 4))
        .filter(|i| before[*i..*i + 3] != after[*i..*i + 3])
        .count();
    assert_eq!(
        edge_changes, 0,
        "text box must not render vertical edges: {edge_changes}"
    );
    let changed_in_answer = (150..230)
        .flat_map(|y| (72..300).map(move |x| (y * 612 + x) * 4))
        .filter(|i| before[*i..*i + 3] != after[*i..*i + 3])
        .count();
    assert!(
        changed_in_answer > 100,
        "free text must visibly render at requested top-left coordinates"
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
    std::fs::remove_file(copy).unwrap();
    std::fs::remove_file(path).unwrap();
}

#[test]
fn ink_stroke_roundtrips_renders_and_leaves_the_original_unchanged() {
    use crate::view::display_point_to_document;
    use poppler::prelude::*;
    let path = crate::doc::write_test_pdf("ink", &[""]);
    let original = std::fs::read(&path).unwrap();
    let doc = OpenDoc::open(&path, None).unwrap();
    assert!(doc
        .add_ink_stroke(0, &[(40.0, 40.0), (40.4, 40.1)])
        .is_err());
    assert!(doc.ink_annotations().is_empty());
    for rotation in [0, 90, 180, 270] {
        let display = [(72.0, 100.0), (160.0, 130.0), (250.0, 100.0)];
        let document = display
            .iter()
            .map(|(x, y)| display_point_to_document(&doc, 0, rotation, *x, *y))
            .collect::<Vec<_>>();
        let back = document
            .iter()
            .map(|(x, y)| {
                crate::annotations::document_point_to_display(*x, *y, 612.0, 792.0, rotation)
            })
            .collect::<Vec<_>>();
        for (got, expected) in back.iter().zip(&display) {
            assert!((got.0 - expected.0).abs() < 1e-6 && (got.1 - expected.1).abs() < 1e-6);
        }
        doc.add_ink_stroke(0, &document).unwrap();
        let stored = doc.ink_document_points(0);
        let added = stored.last().unwrap();
        // Smoothing inserts samples but the curve passes through every input point.
        for expected in &document {
            assert!(
                added
                    .iter()
                    .any(|got| (got.0 - expected.0).abs() < 0.05
                        && (got.1 - expected.1).abs() < 0.05),
                "rotation {rotation}: {expected:?} missing from {added:?}"
            );
        }
    }
    doc.add_ink_stroke(0, &[(80.0, 100.0), (240.0, 100.0)])
        .unwrap();
    doc.add_fill_text(
        0,
        crate::annotations::DocumentRect {
            x: 72.,
            y: 400.,
            width: 120.,
            height: 40.,
        },
        "Keep text",
    )
    .unwrap();
    let before_remove = doc.ink_annotations();
    assert_eq!(before_remove.len(), 5);
    doc.remove_ink(before_remove[0].0, &before_remove[0].1)
        .unwrap();
    assert_eq!(doc.ink_annotations().len(), 4);
    assert_eq!(doc.free_text_annotations().len(), 1);
    let copy = path.with_extension("ink.pdf");
    doc.save_filled_copy(&copy).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), original);
    let reopened = OpenDoc::open(&copy, None).unwrap();
    assert_eq!(reopened.ink_annotations().len(), 4);
    assert_eq!(
        reopened.free_text_annotations()[0].1.contents().as_deref(),
        Some("Keep text")
    );
    let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 612, 792).unwrap();
    {
        let cr = cairo::Context::new(&surface).unwrap();
        cr.set_source_rgb(1., 1., 1.);
        cr.paint().unwrap();
        reopened.page(0).unwrap().render(&cr);
    }
    let data = surface.data().unwrap();
    let dark = |y0: usize, y1: usize, x0: usize, x1: usize| {
        (y0..y1)
            .flat_map(|y| (x0..x1).map(move |x| (y * 612 + x) * 4))
            .filter(|i| (data[*i] as u16) + (data[*i + 1] as u16) + (data[*i + 2] as u16) < 500)
            .count()
    };
    let on_line = dark(96, 106, 80, 240);
    let off_line = dark(300, 310, 80, 240);
    assert!(
        on_line > 30 && on_line > off_line,
        "saved ink must render on the drawn line: on={on_line} off={off_line}"
    );
    std::fs::remove_file(copy).unwrap();
    std::fs::remove_file(path).unwrap();
}

#[test]
fn delete_one_added_text_preserves_other_text_and_original() {
    use poppler::prelude::*;
    let path = crate::doc::write_test_pdf("remove-text", &[""]);
    let original = std::fs::read(&path).unwrap();
    let doc = OpenDoc::open(&path, None).unwrap();
    let rect = crate::annotations::DocumentRect {
        x: 72.,
        y: 150.,
        width: 228.,
        height: 80.,
    };
    doc.add_fill_text(0, rect, "Delete me").unwrap();
    doc.add_fill_text(0, rect, "Keep me").unwrap();
    let entries = doc.free_text_annotations();
    assert_eq!(entries.len(), 2);
    doc.remove_fill_text(entries[0].0, &entries[0].1).unwrap();
    let copy = path.with_extension("filled.pdf");
    doc.save_filled_copy(&copy).unwrap();
    let reopened = OpenDoc::open(&copy, None).unwrap();
    let remaining = reopened.free_text_annotations();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].1.contents().as_deref(), Some("Keep me"));
    reopened
        .remove_fill_text(remaining[0].0, &remaining[0].1)
        .unwrap();
    let cleaned = path.with_extension("cleaned.pdf");
    reopened.save_filled_copy(&cleaned).unwrap();
    assert!(OpenDoc::open(&cleaned, None)
        .unwrap()
        .free_text_annotations()
        .is_empty());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    std::fs::remove_file(cleaned).unwrap();
    std::fs::remove_file(copy).unwrap();
    std::fs::remove_file(path).unwrap();
}

#[test]
fn filled_copy_refuses_original_and_roundtrips_acroforms() {
    let path = fixture();
    let original = std::fs::read(&path).unwrap();
    let doc = OpenDoc::open(&path, None).unwrap();
    field(&doc, "Name").text_set_text("Backend answer");
    field(&doc, "Notes").text_set_text("First\nSecond");
    field(&doc, "Agree").button_set_state(true);
    field(&doc, "Color").choice_unselect_all();
    field(&doc, "Color").choice_select_item(1);
    let radios = fields(&doc)
        .into_iter()
        .filter(|f| f.name().as_deref() == Some("Radio"))
        .collect::<Vec<_>>();
    let chosen = radios.iter().find(|f| !f.button_get_state()).unwrap();
    chosen.button_set_state(true);
    let selected_id = chosen.id();
    let copy = path.with_extension("filled.pdf");
    doc.save_filled_copy(&copy).unwrap();
    let reopened = OpenDoc::open(&copy, None).unwrap();
    assert_eq!(
        field(&reopened, "Name").text_get_text().as_deref(),
        Some("Backend answer")
    );
    assert_eq!(
        field(&reopened, "Notes").text_get_text().as_deref(),
        Some("First\nSecond")
    );
    assert!(field(&reopened, "Agree").button_get_state());
    assert!(field(&reopened, "Color").choice_is_item_selected(1));
    assert_eq!(
        fields(&reopened)
            .into_iter()
            .filter(|f| f.name().as_deref() == Some("Radio") && f.button_get_state())
            .map(|f| f.id())
            .collect::<Vec<_>>(),
        vec![selected_id]
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert!(
        doc.save_filled_copy(&path).is_err(),
        "must refuse overwriting the original"
    );
    std::fs::remove_file(copy).unwrap();
    std::fs::remove_file(path).unwrap();
}

pub(crate) fn fixture() -> std::path::PathBuf {
    use std::io::Write;
    let path = std::env::temp_dir().join(format!(
        "lector-forms-{}-{}.pdf",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [5 0 R 6 0 R 7 0 R 8 0 R 9 0 R 10 0 R 13 0 R 14 0 R] /DA (/Helv 12 Tf 0 g) /DR << /Font << /Helv 4 0 R >> >> >> >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Annots [5 0 R 6 0 R 7 0 R 8 0 R 9 0 R 11 0 R 12 0 R 13 0 R 14 0 R] >>",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        "<< /Type /Annot /Subtype /Widget /FT /Tx /T (Name) /Rect [50 700 250 730] /V (Original) /P 3 0 R >>",
        "<< /Type /Annot /Subtype /Widget /FT /Tx /T (Notes) /Ff 4096 /Rect [50 600 250 680] /P 3 0 R >>",
        "<< /Type /Annot /Subtype /Widget /FT /Tx /T (Locked) /Ff 1 /Rect [50 550 250 580] /V (Keep) /P 3 0 R >>",
        "<< /Type /Annot /Subtype /Widget /FT /Btn /T (Agree) /Rect [50 500 70 520] /V /Off /AS /Off /AP << /N << /Off 15 0 R /Yes 16 0 R >> >> /P 3 0 R >>",
        "<< /Type /Annot /Subtype /Widget /FT /Ch /T (Color) /Ff 131072 /Rect [50 450 250 480] /Opt [(Red) (Green) (Blue)] /V (Red) /P 3 0 R >>",
        "<< /FT /Btn /Ff 32768 /T (Radio) /Kids [11 0 R 12 0 R] /V /One >>",
        "<< /Type /Annot /Subtype /Widget /Parent 10 0 R /Rect [50 400 70 420] /AS /One /AP << /N << /Off 15 0 R /One 16 0 R >> >> /P 3 0 R >>",
        "<< /Type /Annot /Subtype /Widget /Parent 10 0 R /Rect [90 400 110 420] /AS /Off /AP << /N << /Off 15 0 R /Two 16 0 R >> >> /P 3 0 R >>",
        "<< /Type /Annot /Subtype /Widget /FT /Ch /T (Many) /Ff 2097152 /Rect [50 250 250 380] /Opt [(Apple) (Pear) (Plum)] /P 3 0 R >>",
        "<< /Type /Annot /Subtype /Widget /FT /Sig /T (Signature) /Rect [50 150 250 200] /P 3 0 R >>",
        "<< /Type /XObject /Subtype /Form /BBox [0 0 20 20] /Length 0 >>\nstream\n\nendstream",
        "<< /Type /XObject /Subtype /Form /BBox [0 0 20 20] /Length 20 >>\nstream\n0 0 20 20 re 0 g f\nendstream",
    ];
    let mut pdf = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0];
    for (i, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        write!(pdf, "{} 0 obj\n{object}\nendobj\n", i + 1).unwrap();
    }
    let xref = pdf.len();
    write!(pdf, "xref\n0 {}\n0000000000 65535 f \n", offsets.len()).unwrap();
    for offset in offsets.iter().skip(1) {
        writeln!(pdf, "{offset:010} 00000 n ").unwrap();
    }
    write!(
        pdf,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        offsets.len()
    )
    .unwrap();
    std::fs::write(&path, pdf).unwrap();
    path
}

pub(crate) fn fields(doc: &OpenDoc) -> Vec<poppler::FormField> {
    use glib::translate::{from_glib_none, ToGlibPtr};
    doc.page(0)
        .unwrap()
        .form_field_mapping()
        .iter()
        .map(|mapping| unsafe {
            let ptr: *mut poppler::ffi::PopplerFormFieldMapping = mapping.to_glib_none().0;
            from_glib_none((*ptr).field)
        })
        .collect()
}

pub(crate) fn field(doc: &OpenDoc, name: &str) -> poppler::FormField {
    fields(doc)
        .into_iter()
        .find(|f| f.name().as_deref() == Some(name))
        .unwrap()
}

/// Dark-pixel bounds and row runs of a surface, in points at `scale`.
fn ink_bounds(surface: &mut cairo::ImageSurface, scale: f64) -> (f64, f64, Vec<(f64, f64)>) {
    let width = surface.width() as usize;
    let stride = surface.stride() as usize;
    let data = surface.data().unwrap();
    let (mut x0, mut x1) = (usize::MAX, 0);
    let mut rows = std::collections::BTreeSet::new();
    for y in 0..data.len() / stride {
        for x in 0..width {
            let i = y * stride + x * 4;
            if (data[i] as u16 + data[i + 1] as u16 + data[i + 2] as u16) < 300 {
                x0 = x0.min(x);
                x1 = x1.max(x);
                rows.insert(y);
            }
        }
    }
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for row in rows {
        match runs.last_mut() {
            Some(run) if run.1 + 1 == row => run.1 = row,
            _ => runs.push((row, row)),
        }
    }
    (
        x0 as f64 / scale,
        x1 as f64 / scale,
        runs.iter()
            .map(|(a, b)| (*a as f64 / scale, *b as f64 / scale))
            .collect(),
    )
}

#[test]
fn text_preview_matches_what_poppler_commits() {
    let path = crate::doc::write_test_pdf("preview-match", &[""]);
    let doc = OpenDoc::open(&path, None).unwrap();
    let scale = 4.0;
    for (size, text) in [(12.0, "HHHH HHHH\nHHHH"), (20.0, "HH\nHHH HH\nH")] {
        let rect = crate::view::free_text_rect(100.0, 100.0, text, size, 612.0, 792.0);
        let annot = doc
            .add_free_text(
                0,
                &crate::doc::FreeTextSpec {
                    rect,
                    text: text.into(),
                    size,
                    rgb: (0.0, 0.0, 0.0),
                },
            )
            .unwrap();
        let mut committed =
            cairo::ImageSurface::create(cairo::Format::ARgb32, 612 * 4, 792 * 4).unwrap();
        {
            let cr = cairo::Context::new(&committed).unwrap();
            cr.set_source_rgb(1., 1., 1.);
            cr.paint().unwrap();
            cr.scale(scale, scale);
            doc.page(0).unwrap().render(&cr);
        }
        doc.remove_fill_text(0, &annot).unwrap();
        let mut preview =
            cairo::ImageSurface::create(cairo::Format::ARgb32, 612 * 4, 792 * 4).unwrap();
        {
            let cr = cairo::Context::new(&preview).unwrap();
            cr.set_source_rgb(1., 1., 1.);
            cr.paint().unwrap();
            crate::view::paint_free_text_draft(
                &cr,
                crate::view::TextPreview {
                    rect,
                    text,
                    cursor: 0,
                    size,
                    rgb: (0.0, 0.0, 0.0),
                    // White chrome: compare only the glyphs.
                    accent: (1.0, 1.0, 1.0),
                    page_width: 612.0,
                    page_height: 792.0,
                    rotation: 0,
                    scale,
                },
            );
        }
        let (cx0, cx1, crows) = ink_bounds(&mut committed, scale);
        let (px0, px1, prows) = ink_bounds(&mut preview, scale);
        assert_eq!(
            crows.len(),
            text.split('\n').count(),
            "size {size}: Poppler must not wrap the committed box: {crows:?}"
        );
        assert_eq!(
            crows.len(),
            prows.len(),
            "size {size}: {crows:?} vs {prows:?}"
        );
        for (c, p) in crows.iter().zip(&prows) {
            assert!(
                (c.0 - p.0).abs() < 0.75 && (c.1 - p.1).abs() < 0.75,
                "size {size}: line moved on commit: {crows:?} vs {prows:?}"
            );
        }
        assert!((cx0 - px0).abs() < 0.75, "size {size}: left {cx0} vs {px0}");
        assert!((cx1 - px1).abs() < 1.5, "size {size}: right {cx1} vs {px1}");
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn free_text_style_roundtrips_and_is_found_by_point() {
    let path = crate::doc::write_test_pdf("text-style", &[""]);
    let doc = OpenDoc::open(&path, None).unwrap();
    let spec = crate::doc::FreeTextSpec {
        rect: crate::view::free_text_rect(90.0, 300.0, "Blue answer", 18.0, 612.0, 792.0),
        text: "Blue answer".into(),
        size: 18.0,
        rgb: (0.09, 0.26, 0.66),
    };
    doc.add_free_text(0, &spec).unwrap();
    let copy = path.with_extension("styled.pdf");
    doc.save_filled_copy(&copy).unwrap();
    let reopened = OpenDoc::open(&copy, None).unwrap();
    let (annot, found) = reopened
        .free_text_at(0, 95.0, 310.0)
        .expect("point inside the box must hit it");
    assert_eq!(found.text, spec.text);
    assert!((found.size - 18.0).abs() < 0.01, "size {}", found.size);
    for (got, want) in [
        (found.rgb.0, spec.rgb.0),
        (found.rgb.1, spec.rgb.1),
        (found.rgb.2, spec.rgb.2),
    ] {
        assert!(
            (got - want).abs() < 0.01,
            "{:?} vs {:?}",
            found.rgb,
            spec.rgb
        );
    }
    assert!((found.rect.x - spec.rect.x).abs() < 0.01);
    assert!((found.rect.y - spec.rect.y).abs() < 0.01);
    assert!(reopened.free_text_at(0, 95.0, 200.0).is_none());
    reopened.remove_fill_text(0, &annot).unwrap();
    assert!(reopened.free_text_annotations().is_empty());
    std::fs::remove_file(copy).unwrap();
    std::fs::remove_file(path).unwrap();
}

#[test]
fn strokes_group_into_one_drawing_and_pop_back_out() {
    use poppler::prelude::*;
    let path = crate::doc::write_test_pdf("ink-group", &[""]);
    let doc = OpenDoc::open(&path, None).unwrap();
    let style = crate::doc::InkStyle {
        rgb: (0.78, 0.12, 0.12),
        width: 2.8,
    };
    let drawing = doc
        .add_ink_drawing(0, &[(80.0, 100.0), (120.0, 110.0), (160.0, 100.0)], style)
        .unwrap();
    doc.append_ink_stroke(0, &drawing, &[(300.0, 400.0), (340.0, 420.0)])
        .unwrap();
    doc.append_ink_stroke(0, &drawing, &[(90.0, 600.0), (95.0, 640.0)])
        .unwrap();
    assert_eq!(doc.ink_annotations().len(), 1);
    let strokes = doc.ink_document_strokes(0);
    assert_eq!(strokes.len(), 3);
    assert!(strokes.iter().all(|(s, _)| (s.width - 2.8).abs() < 0.01));
    // The Rect must cover every stroke, or other viewers squash the drawing.
    let area = drawing.rectangle();
    for (_, points) in &strokes {
        for (x, y) in points {
            let pdf_y = 792.0 - y;
            assert!(
                *x >= area.x1() && *x <= area.x2() && pdf_y >= area.y1() && pdf_y <= area.y2(),
                "({x}, {pdf_y}) outside {area:?}"
            );
        }
    }
    let copy = path.with_extension("grouped.pdf");
    doc.save_filled_copy(&copy).unwrap();
    let reopened = OpenDoc::open(&copy, None).unwrap();
    assert_eq!(reopened.ink_annotations().len(), 1);
    let saved = reopened.ink_document_strokes(0);
    assert_eq!(saved.len(), 3);
    let (red, _, _) = saved[0].0.rgb;
    assert!((red - 0.78).abs() < 0.01);
    assert!(!doc.pop_ink_stroke(0, &drawing).unwrap());
    assert_eq!(doc.ink_document_strokes(0).len(), 2);
    assert!(!doc.pop_ink_stroke(0, &drawing).unwrap());
    assert!(
        doc.pop_ink_stroke(0, &drawing).unwrap(),
        "last pop removes the drawing"
    );
    assert!(doc.ink_annotations().is_empty());
    std::fs::remove_file(copy).unwrap();
    std::fs::remove_file(path).unwrap();
}

#[test]
fn smoothing_passes_through_input_and_rounds_corners() {
    let input = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (20.0, 10.0)];
    let smooth = crate::doc::smooth_polyline(&input);
    assert!(smooth.len() > input.len() * 4);
    for point in input {
        assert!(
            smooth
                .iter()
                .any(|p| (p.0 - point.0).abs() < 1e-9 && (p.1 - point.1).abs() < 1e-9),
            "curve must pass through {point:?}"
        );
    }
    // A sampled Catmull-Rom leaves the sharp corner at (10, 0) smoothly:
    // no two consecutive segments turn by 90 degrees.
    let max_turn = smooth
        .windows(3)
        .map(|w| {
            let a = (w[1].0 - w[0].0, w[1].1 - w[0].1);
            let b = (w[2].0 - w[1].0, w[2].1 - w[1].1);
            let cos = (a.0 * b.0 + a.1 * b.1) / ((a.0.hypot(a.1)) * (b.0.hypot(b.1))).max(1e-12);
            cos.clamp(-1.0, 1.0).acos().to_degrees()
        })
        .fold(0.0_f64, f64::max);
    assert!(max_turn < 60.0, "corner stays sharp: {max_turn}°");
    assert!(crate::doc::smooth_ink_stroke(&[(1.0, 1.0), (1.2, 1.1)]).is_none());
    let long = (0..10_000)
        .map(|i| (i as f64 * 0.7, (i as f64 * 0.05).sin() * 20.0))
        .collect::<Vec<_>>();
    assert!(crate::doc::smooth_ink_stroke(&long).unwrap().len() <= 2001);
}
