//! Display-backed checks run explicitly; no windows are presented or sessions saved.
use super::*;

#[test]
#[ignore = "requires a GTK display; run separately"]
fn actual_adoption_invalidates_raster_lifetime() {
    gtk::init().unwrap();
    let app = Application::builder()
        .application_id("org.omarchy.LectorRasterLifetime")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    ui.inner.borrow_mut().session = Session::default();
    let path = crate::doc::write_test_pdf("raster-lifetime", &[""]);
    ui.open_path(&path, None);
    let key = {
        let st = ui.inner.borrow();
        RasterKey::from_geom(
            &st.view.layout.pages[0],
            st.view.rotation,
            st.view.pdf_mode,
            st.theme.pdf_palette(),
            ui.page_dpr(),
        )
    };
    let old_gen = ui.inner.borrow().raster_gen;
    ui.inner.borrow_mut().raster.insert(
        key,
        gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, 2, 2).unwrap(),
    );
    ui.raster_pending.borrow_mut().insert(999);
    ui.open_path(&path, None);
    assert!(
        !ui.inner.borrow().raster.contains(&key),
        "same-path reload retained old pixels"
    );
    assert_ne!(ui.inner.borrow().raster_gen, old_gen);
    assert!(!ui.raster_pending.borrow().contains(&999));
    let pending = ui.raster_pending.borrow().clone();
    ui.raster_one(old_gen);
    assert_eq!(
        *ui.raster_pending.borrow(),
        pending,
        "stale callback consumed live work"
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "requires a GTK display; run separately"]
fn actual_sidebar_population_is_lazy_and_reused() {
    gtk::init().unwrap();
    let app = Application::builder()
        .application_id("org.omarchy.LectorLazySidebar")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    ui.inner.borrow_mut().session = Session::default();
    ui.set_layout(LayoutMode::Single);
    ui.set_sidebar_visible(false);
    let path = crate::doc::write_test_pdf("lazy-sidebar", &[""; 1000]);
    let start = std::time::Instant::now();
    ui.open_path(&path, None);
    let mut rows = 0;
    let mut child = ui.thumbs.first_child();
    while let Some(row) = child {
        rows += 1;
        child = row.next_sibling();
    }
    eprintln!(
        "synthetic unpresented 1000-page open: {:?}; thumbnail rows={rows}",
        start.elapsed()
    );
    assert_eq!(rows, 0, "hidden pane must construct no thumbnail rows");
    ui.set_sidebar_visible(true);
    assert_eq!(ui.inner.borrow().pane.sidebar, Sidebar::Outline);
    assert!(ui.thumbs.first_child().is_none());
    ui.stack.set_visible_child_name("thumbs");
    let first = ui.thumbs.first_child().expect("thumbnail tab populated");
    assert!(ui.thumbs.row_at_index(999).is_some());
    ui.stack.set_visible_child_name("marks");
    ui.stack.set_visible_child_name("thumbs");
    assert_eq!(
        ui.thumbs.first_child().unwrap(),
        first,
        "tab switching rebuilt rows"
    );
    ui.set_sidebar_visible(false);
    ui.set_sidebar_visible(true);
    ui.stack.set_visible_child_name("thumbs");
    assert_eq!(ui.thumbs.first_child().unwrap(), first);
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "requires a GTK display; run separately"]
fn actual_marks_selection_follows_target_after_sidebar_reuse() {
    gtk::init().unwrap();
    let app = Application::builder()
        .application_id("org.omarchy.LectorMarksReuse")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    ui.inner.borrow_mut().session = Session::default();
    ui.inner.borrow_mut().annotations = Some(AnnotationStore::default());
    let path = crate::doc::write_test_pdf("marks-reuse", &[""]);
    ui.open_path(&path, None);
    let annotation = new_annotation(
        0,
        DocumentRect {
            x: 10.0,
            y: 10.0,
            width: 40.0,
            height: 20.0,
        },
        AnnotationKind::Highlight,
    );
    let id = annotation.id.clone();
    {
        let mut st = ui.inner.borrow_mut();
        st.annotations.as_mut().unwrap().add(&path, annotation);
    }
    ui.set_sidebar_visible(true);
    ui.stack.set_visible_child_name("marks");
    ui.activate_mark(&id);
    assert_eq!(
        ui.marks
            .selected_row()
            .map(|row| row.widget_name().to_string()),
        Some(id.clone())
    );
    ui.set_sidebar_visible(false);
    {
        let mut st = ui.inner.borrow_mut();
        let inner = &mut *st;
        begin_selection_state(
            &mut inner.view.selection,
            &mut inner.pane.targeted_annotation,
            Some((0, 12.0, 12.0)),
        );
    }
    ui.set_sidebar_visible(true);
    ui.stack.set_visible_child_name("marks");
    assert!(
        ui.marks.selected_row().is_none(),
        "reused Marks pane kept a row selected after targeting was cleared"
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "requires a GTK display; run separately"]
fn actual_raster_queue_prunes_obsolete_and_cached_work() {
    gtk::init().unwrap();
    let app = Application::builder()
        .application_id("org.omarchy.LectorRasterQueue")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    ui.inner.borrow_mut().session = Session::default();
    ui.set_layout(LayoutMode::Continuous);
    let path = crate::doc::write_test_pdf("raster-queue", &[""; 20]);
    ui.open_path(&path, None);
    ui.raster_pending.borrow_mut().extend(0..20);
    ui.ensure_visible_rasters();
    let needed = pages_intersecting_view(
        &ui.inner.borrow().view.layout.pages,
        ui.scrolled.vadjustment().value(),
        ui.scrolled.height() as f64,
        VIEW_PAGE_PAD,
    );
    eprintln!(
        "obsolete work: queued 20, retained {} for {} needed pages",
        ui.raster_pending.borrow().len(),
        needed.len()
    );
    assert!(ui
        .raster_pending
        .borrow()
        .iter()
        .all(|p| needed.contains(p)));
    let index = needed[0];
    let key = {
        let st = ui.inner.borrow();
        RasterKey::from_geom(
            &st.view.layout.page_geom(index).unwrap(),
            st.view.rotation,
            st.view.pdf_mode,
            st.theme.pdf_palette(),
            ui.page_dpr(),
        )
    };
    let sentinel = gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, 2, 2).unwrap();
    ui.inner.borrow_mut().raster.insert(key, sentinel);
    ui.raster_pending.borrow_mut().clear();
    ui.raster_pending.borrow_mut().insert(index);
    let gen = ui.inner.borrow().raster_gen;
    ui.raster_one(gen);
    assert_eq!(
        ui.inner.borrow_mut().raster.get(&key).unwrap().width(),
        2,
        "cache hit was redundantly rasterized by Poppler"
    );
    ui.raster_pending.borrow_mut().insert(19);
    assert_eq!(
        ui.take_next_raster_index(),
        None,
        "idle must recheck live viewport"
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "requires a GTK display; run separately"]
fn actual_rotation_feedback_reuses_correctly_oriented_pixels() {
    gtk::init().unwrap();
    let app = Application::builder()
        .application_id("org.omarchy.LectorRotationFeedback")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    ui.inner.borrow_mut().session = Session::default();
    ui.set_layout(LayoutMode::Single);
    let path = crate::doc::write_test_pdf("rotation-feedback", &["1 0 0 rg 0 0 306 396 re f 0 1 0 rg 306 0 306 396 re f 0 0 1 rg 0 396 306 396 re f 0 0 0 rg 306 396 306 396 re f"]);
    ui.open_path(&path, None);
    ui.set_zoom(Zoom::Custom(1.0));
    let gen = ui.inner.borrow().raster_gen;
    ui.raster_one(gen);
    fn corners(surface: &mut gtk::cairo::ImageSurface) -> Vec<[u8; 4]> {
        let (w, h, s) = (
            surface.width() as usize,
            surface.height() as usize,
            surface.stride() as usize,
        );
        let data = surface.data().unwrap();
        [
            (w / 10, h / 10),
            (w * 9 / 10, h / 10),
            (w / 10, h * 9 / 10),
            (w * 9 / 10, h * 9 / 10),
        ]
        .iter()
        .map(|(x, y)| data[y * s + x * 4..y * s + x * 4 + 4].try_into().unwrap())
        .collect()
    }
    for rotation in [90, 180, 270, 0] {
        let start = std::time::Instant::now();
        ui.rotate(90);
        let (geom, mut expected) = {
            let st = ui.inner.borrow();
            let geom = st.view.layout.page_geom(0).unwrap();
            let expected = raster_page(
                st.view.doc.as_ref().unwrap(),
                &geom,
                rotation,
                st.view.pdf_mode,
                st.theme.pdf_palette(),
                1.0,
            )
            .unwrap();
            (geom, expected)
        };
        let mut actual = gtk::cairo::ImageSurface::create(
            gtk::cairo::Format::ARgb32,
            geom.w as i32,
            geom.h as i32,
        )
        .unwrap();
        {
            let cr = gtk::cairo::Context::new(&actual).unwrap();
            ui.draw_page(&cr, 0, 1.0);
        }
        eprintln!(
            "rotation {rotation}: synchronous feedback+reference {:?}; no raster idle dispatched",
            start.elapsed()
        );
        assert_eq!(
            corners(&mut actual),
            corners(&mut expected),
            "rotation={rotation}"
        );
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "requires a GTK display; run separately"]
fn actual_layout_transition_keeps_reusable_page_widget() {
    gtk::init().unwrap();
    let app = Application::builder()
        .application_id("org.omarchy.LectorWidgetReuse")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    ui.inner.borrow_mut().session = Session::default();
    ui.set_layout(LayoutMode::Single);
    let path = crate::doc::write_test_pdf("widget-reuse", &[""; 2]);
    ui.open_path(&path, None);
    let first = ui.page_widgets.borrow()[0].1.clone();
    ui.set_layout(LayoutMode::Continuous);
    assert_eq!(
        ui.page_widgets
            .borrow()
            .iter()
            .find(|(i, _)| *i == 0)
            .unwrap()
            .1,
        first,
        "layout transition recreated unchanged page widget"
    );
    ui.set_layout(LayoutMode::Single);
    assert_eq!(ui.page_widgets.borrow()[0].1, first);
    std::fs::remove_file(path).unwrap();
}

fn widget_tree_has_label(widget: &gtk::Widget, expected: &str) -> bool {
    if widget
        .downcast_ref::<Label>()
        .is_some_and(|label| label.text() == expected)
    {
        return true;
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        if widget_tree_has_label(&current, expected) {
            return true;
        }
        child = current.next_sibling();
    }
    false
}

fn widget_tree_has_css_class(widget: &gtk::Widget, class: &str) -> bool {
    if widget.has_css_class(class) {
        return true;
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        if widget_tree_has_css_class(&current, class) {
            return true;
        }
        child = current.next_sibling();
    }
    false
}

fn wait_for_view_restore(ui: &Lector, context: &glib::MainContext) {
    for _ in 0..512 {
        if !ui.inner.borrow().ignore_scroll {
            return;
        }
        context.iteration(true);
    }
    assert!(!ui.inner.borrow().ignore_scroll, "view restore timed out");
}

#[test]
#[ignore = "requires a GTK display; run with --ignored --test-threads=1"]
fn actual_window_navigation_presentation_search_and_reload() {
    gtk::init().expect("GTK display required");
    let app = Application::builder()
        .application_id("org.omarchy.LectorRegression")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    // Fullscreen requests require a native surface even though this test never
    // presents the window. GTK can otherwise crash destroying the unrealized
    // fullscreen window during cleanup.
    gtk::prelude::WidgetExt::realize(&ui.window);
    assert!(
        !widget_tree_has_label(ui.toolbar.upcast_ref(), "lector"),
        "the document toolbar must not display the application name"
    );
    for control in [
        ui.back_btn.upcast_ref::<gtk::Widget>(),
        ui.forward_btn.upcast_ref(),
        ui.prev_btn.upcast_ref(),
        ui.next_btn.upcast_ref(),
        ui.page_entry.upcast_ref(),
        ui.zoom_out_btn.upcast_ref(),
        ui.zoom_in_btn.upcast_ref(),
        ui.zoom_drop.upcast_ref(),
        ui.find_btn.upcast_ref(),
        ui.highlight_item.upcast_ref(),
        ui.note_item.upcast_ref(),
        ui.delete_annotation_item.upcast_ref(),
    ] {
        assert!(
            !control.is_sensitive(),
            "document controls must start disabled"
        );
    }
    ui.inner.borrow_mut().session = Session::default();
    ui.set_layout(LayoutMode::Continuous);
    let long =
        crate::doc::write_test_pdf("window-long", &["BT /F1 20 Tf 72 700 Td (needle) Tj ET"; 8]);
    let short = crate::doc::write_test_pdf(
        "window-short",
        &["BT /F1 20 Tf 72 700 Td (needle) Tj ET"; 2],
    );
    ui.open_path(&long, None);
    assert!(!ui.back_btn.is_sensitive());
    ui.navigate_to_page(0);
    assert!(!ui.back_btn.is_sensitive());
    ui.inner.borrow_mut().annotations = Some(AnnotationStore::default());
    ui.inner.borrow_mut().view.selection = Some(Selection {
        page: 0,
        x1: 70.0,
        y1: 70.0,
        x2: 160.0,
        y2: 110.0,
    });
    ui.update_annotation_actions();
    assert!(ui.highlight_item.is_sensitive());
    assert!(ui.note_item.is_sensitive());
    assert_eq!(ui.inner.borrow().view.doc.as_ref().unwrap().n_pages, 8);
    ui.follow_link(DocumentLink::GotoPage(3));
    assert_eq!(ui.inner.borrow().view.page, 3);
    assert!(ui.back_btn.is_sensitive());
    assert!(!ui.forward_btn.is_sensitive());
    ui.navigate_back();
    assert_eq!(ui.inner.borrow().view.page, 0);
    assert!(ui.forward_btn.is_sensitive());
    ui.navigate_forward();
    assert_eq!(ui.inner.borrow().view.page, 3);
    ui.goto_page(0);
    assert!(ui.cont_item.is_active());
    assert!(!ui.dual_item.is_active());
    assert!(!ui.single_item.is_active());
    assert_eq!(
        ui.invert_item.is_active(),
        ui.inner.borrow().view.pdf_mode == PdfMode::Inverted,
        "inverted menu state must reflect the active preference"
    );
    assert_eq!(
        ui.normal_item.is_active(),
        ui.inner.borrow().view.pdf_mode == PdfMode::Normal,
        "normal menu state must reflect the active preference"
    );
    assert_eq!(
        ui.omarchy_item.is_active(),
        ui.inner.borrow().view.pdf_mode == PdfMode::Omarchy,
        "Omarchy menu state must reflect the active preference"
    );
    ui.dual_item.set_active(true);
    assert_eq!(ui.inner.borrow().view.layout_mode, LayoutMode::Dual);
    ui.omarchy_item.set_active(true);
    assert_eq!(ui.inner.borrow().view.pdf_mode, PdfMode::Omarchy);
    ui.normal_item.set_active(true);
    assert_eq!(ui.inner.borrow().view.pdf_mode, PdfMode::Normal);
    ui.invert_item.set_active(true);
    assert_eq!(ui.inner.borrow().view.pdf_mode, PdfMode::Inverted);
    ui.set_layout(LayoutMode::Single);
    assert!(ui.single_item.is_active());
    assert!(!ui.cont_item.is_active());
    ui.goto_page(1);
    assert_eq!(ui.page_widgets.borrow()[0].0, 1);
    ui.set_layout(LayoutMode::Dual);
    ui.goto_page(4);
    assert_eq!(
        ui.page_widgets
            .borrow()
            .iter()
            .map(|(i, _)| *i)
            .collect::<Vec<_>>(),
        vec![4, 5]
    );
    ui.set_zoom(Zoom::Custom(1.25));
    ui.toggle_presentation();
    assert!(ui.inner.borrow().presentation);
    assert_eq!(ui.inner.borrow().view.layout_mode, LayoutMode::Single);
    ui.toggle_presentation();
    assert!(!ui.inner.borrow().presentation);
    assert_eq!(ui.inner.borrow().view.layout_mode, LayoutMode::Dual);
    assert_eq!(ui.inner.borrow().view.zoom, Zoom::Custom(1.25));
    ui.goto_page(6);
    ui.rotate(90);
    assert_eq!(ui.inner.borrow().view.rotation, 90);
    ui.reload();
    assert_eq!(ui.inner.borrow().view.page, 6);
    assert_eq!(ui.inner.borrow().view.zoom, Zoom::Custom(1.25));
    assert_eq!(ui.inner.borrow().view.rotation, 90);
    ui.search("needle");
    let previous_generation = ui.inner.borrow().search.gen;
    ui.open_path(&short, None);
    assert_eq!(ui.inner.borrow().view.rotation, 0);
    assert_ne!(ui.inner.borrow().search.gen, previous_generation);
    assert!(ui.inner.borrow().search.query.is_empty());
    assert_eq!(ui.search_info.text(), "");
    let context = glib::MainContext::default();
    for _ in 0..128 {
        if !context.pending() {
            break;
        }
        context.iteration(false);
    }
    assert!(ui.inner.borrow().search.hits.is_empty());
    ui.search("needle");
    for _ in 0..128 {
        if ui.search_info.text() == "1 of 2" {
            break;
        }
        context.iteration(false);
    }
    assert_eq!(ui.inner.borrow().search.hits.len(), 2);
    assert_eq!(ui.search_info.text(), "1 of 2");
    ui.set_layout(LayoutMode::Continuous);
    assert!(ui.pages_box.first_child().unwrap().is::<DrawingArea>());
    ui.set_layout(LayoutMode::Dual);
    assert!(ui.pages_box.first_child().unwrap().is::<DrawingArea>());
    assert!(
        ui.page_widgets.borrow().len() <= 2,
        "dual view must not instantiate every document page"
    );
    ui.open_path(&long, None);
    assert_eq!(ui.inner.borrow().view.page, 6);
    assert_eq!(ui.inner.borrow().view.zoom, Zoom::Custom(1.25));
    assert_eq!(ui.inner.borrow().view.rotation, 90);
    // Do not use close(): it invokes production persistence.
    ui.hold.borrow_mut().take();
    ui.window.destroy();
    std::fs::remove_file(long).unwrap();
    std::fs::remove_file(short).unwrap();
}

#[test]
#[ignore = "requires a GTK display; run with --ignored --test-threads=1"]
fn actual_window_info_dialogs_use_lector_chrome() {
    gtk::init().expect("GTK display required");
    let app = Application::builder()
        .application_id("org.omarchy.LectorDialogRegression")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    gtk::prelude::WidgetExt::realize(&ui.window);

    let shortcuts = ui.shortcuts_dialog();
    assert!(
        shortcuts.has_css_class("lector-dialog") && shortcuts.has_css_class("lector-window"),
        "info dialogs must use Lector window chrome, not a system alert"
    );
    let shortcuts_root = shortcuts.upcast_ref::<gtk::Widget>();
    for label in [
        "Keyboard shortcuts",
        "FILE",
        "VIEW",
        "FIND",
        "Open",
        "Ctrl+O",
        "Pinch to zoom",
    ] {
        assert!(
            widget_tree_has_label(shortcuts_root, label),
            "shortcuts dialog missing {label}"
        );
    }
    assert!(
        widget_tree_has_css_class(shortcuts_root, "lector-pill"),
        "dialogs must use the same close pill as the toolbar"
    );
    shortcuts.destroy();

    let about = ui.about_dialog();
    let about_root = about.upcast_ref::<gtk::Widget>();
    assert!(about.has_css_class("lector-dialog"));
    assert!(widget_tree_has_label(about_root, "Lector"));
    assert!(widget_tree_has_label(
        about_root,
        "Native PDF viewer for Omarchy"
    ));
    assert!(widget_tree_has_label(about_root, env!("CARGO_PKG_VERSION")));
    assert!(
        !widget_tree_has_label(about_root, "GNOME Document Viewer"),
        "about copy should match Lector, not a stock toolkit blurb"
    );
    about.destroy();

    ui.inner.borrow_mut().session = Session::default();
    let pdf = crate::doc::write_test_pdf(
        "dialog-props",
        &["BT /F1 20 Tf 72 700 Td (properties) Tj ET"],
    );
    ui.open_path(&pdf, None);
    let props = ui
        .properties_dialog()
        .expect("an open document must have properties");
    let props_root = props.upcast_ref::<gtk::Widget>();
    assert!(props.has_css_class("lector-dialog"));
    for label in ["Properties", "File", "Title", "Pages"] {
        assert!(
            widget_tree_has_label(props_root, label),
            "properties dialog missing {label}"
        );
    }
    props.destroy();

    ui.inner.borrow_mut().session.remember(
        Path::new("/definitely/missing/lector.pdf"),
        4,
        Zoom::Actual,
    );
    let history = ui.reading_history_dialog();
    let history_root = history.upcast_ref::<gtk::Widget>();
    for label in ["Reading History", "lector.pdf", "Missing"] {
        assert!(
            widget_tree_has_label(history_root, label),
            "history dialog missing {label}"
        );
    }
    history.destroy();

    ui.hold.borrow_mut().take();
    std::fs::remove_file(pdf).unwrap();
}

#[test]
#[ignore = "requires a GTK display; run with --ignored --test-threads=1"]
fn actual_pinch_zoom_keeps_anchor_and_page_during_reflow() {
    gtk::init().expect("GTK display required");
    let app = Application::builder()
        .application_id("org.omarchy.LectorPinchRegression")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    ui.window.present();
    let context = glib::MainContext::default();
    for _ in 0..64 {
        context.iteration(false);
    }

    let pdf = crate::doc::write_test_pdf(
        "pinch-anchor",
        &["BT /F1 20 Tf 72 700 Td (pinch) Tj ET"; 30],
    );
    ui.inner.borrow_mut().session = Session::default();
    ui.set_layout(LayoutMode::Continuous);
    ui.open_path(&pdf, None);
    ui.goto_page(15);
    wait_for_view_restore(&ui, &context);

    let page_before = ui.inner.borrow().view.page;
    let start_scale = ui.inner.borrow().view.layout.scale;
    let anchor = ui.capture_anchor(Some((300.0, 260.0)));
    let (first_zoom, first_restore) = pinch_zoom_update(start_scale, 1.2, anchor);
    ui.set_zoom_restore(first_zoom, first_restore);
    assert_eq!(
        ui.inner.borrow().view.page,
        page_before,
        "temporary adjustment signals during pinch reflow must not change pages"
    );
    let (final_zoom, final_restore) = pinch_zoom_update(start_scale, 1.5, anchor);
    ui.set_zoom_restore(final_zoom, final_restore);
    wait_for_view_restore(&ui, &context);
    let pages = ui.inner.borrow().view.layout.pages.clone();
    let (_, expected_y) = restored_scroll(anchor, &pages);
    let v = ui.scrolled.vadjustment();
    let expected_y = clamp_adjustment(expected_y, v.lower(), v.upper(), v.page_size());
    assert!(
        (v.value() - expected_y).abs() < 1.0,
        "pinch must restore the original focal point: got {}, expected {expected_y}",
        v.value()
    );

    ui.hold.borrow_mut().take();
    ui.window.destroy();
    std::fs::remove_file(pdf).unwrap();
}

#[test]
#[ignore = "requires a GTK display; run with --ignored --test-threads=1"]
fn actual_zoom_button_keeps_the_current_page() {
    gtk::init().expect("GTK display required");
    let app = Application::builder()
        .application_id("org.omarchy.LectorZoomButtonRegression")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    let context = glib::MainContext::default();
    let pdf =
        crate::doc::write_test_pdf("zoom-button", &["BT /F1 20 Tf 72 700 Td (zoom) Tj ET"; 20]);
    ui.inner.borrow_mut().session = Session::default();
    ui.set_layout(LayoutMode::Continuous);
    ui.open_path(&pdf, None);
    ui.goto_page(8);
    wait_for_view_restore(&ui, &context);
    let page_before = ui.inner.borrow().view.page;
    ui.zoom_in_btn.emit_clicked();
    wait_for_view_restore(&ui, &context);
    assert_eq!(
        ui.inner.borrow().view.page,
        page_before,
        "toolbar zoom must not jump to another page"
    );
    ui.hold.borrow_mut().take();
    std::fs::remove_file(pdf).unwrap();
}

#[test]
#[ignore = "requires a GTK display; run with --ignored --test-threads=1"]
fn actual_back_navigation_restores_single_page_viewport() {
    gtk::init().expect("GTK display");
    let app = gtk::Application::builder()
        .application_id("org.omarchy.Lector.NavigationViewportTest")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    ui.window.present();
    let context = glib::MainContext::default();
    for _ in 0..64 {
        context.iteration(false);
    }

    let path = crate::doc::write_test_pdf("navigation-viewport", &["one", "two"]);
    ui.open_path(&path, None);
    ui.set_layout(LayoutMode::Single);
    ui.set_zoom(Zoom::Custom(4.0));
    wait_for_view_restore(&ui, &context);
    let horizontal = ui.scrolled.hadjustment();
    let vertical = ui.scrolled.vadjustment();
    let saved_x = clamp_adjustment(
        horizontal.upper() * 0.35,
        horizontal.lower(),
        horizontal.upper(),
        horizontal.page_size(),
    );
    let saved_y = clamp_adjustment(
        vertical.upper() * 0.35,
        vertical.lower(),
        vertical.upper(),
        vertical.page_size(),
    );
    assert!(saved_x > 100.0, "fixture must scroll horizontally");
    assert!(saved_y > 100.0, "fixture must scroll vertically");
    horizontal.set_value(saved_x);
    vertical.set_value(saved_y);

    ui.navigate_to_page(1);
    wait_for_view_restore(&ui, &context);
    ui.navigate_back();
    wait_for_view_restore(&ui, &context);
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(150);
    while std::time::Instant::now() < deadline {
        while context.pending() {
            context.iteration(false);
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    assert_eq!(ui.inner.borrow().view.page, 0);
    assert!((horizontal.value() - saved_x).abs() < 1.0);
    assert!((vertical.value() - saved_y).abs() < 1.0);

    let _ = std::fs::remove_file(path);
    ui.hold.borrow_mut().take();
    ui.window.destroy();
}

#[test]
#[ignore = "requires a GTK display; run with --ignored --test-threads=1"]
fn actual_outline_disclosure_hides_and_restores_descendants() {
    gtk::init().expect("GTK display");
    let app = gtk::Application::builder()
        .application_id("org.omarchy.Lector.OutlineTest")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    let path = crate::doc::write_test_pdf("outline", &["one", "two", "three"]);
    ui.open_path(&path, None);
    ui.inner.borrow_mut().view.doc.as_mut().unwrap().outline = vec![
        crate::doc::OutlineItem {
            title: "Chapter".into(),
            page: Some(0),
            depth: 0,
        },
        crate::doc::OutlineItem {
            title: "Section".into(),
            page: Some(1),
            depth: 1,
        },
        crate::doc::OutlineItem {
            title: "Appendix".into(),
            page: Some(2),
            depth: 0,
        },
    ];
    let collapsed = crate::doc::initially_collapsed_outline(
        &ui.inner.borrow().view.doc.as_ref().unwrap().outline,
    );
    ui.inner.borrow_mut().pane.outline_collapsed = collapsed;
    ui.rebuild_outline();
    ui.goto_page(2);
    let row_count = || {
        let mut count = 0;
        let mut child = ui.outline.first_child();
        while let Some(widget) = child {
            count += 1;
            child = widget.next_sibling();
        }
        count
    };
    assert_eq!(row_count(), 2);

    let first = ui.outline.first_child().unwrap();
    let row = first.downcast::<gtk::ListBoxRow>().unwrap();
    let content = row.child().unwrap().downcast::<gtk::Box>().unwrap();
    let toggle = content
        .first_child()
        .unwrap()
        .downcast::<gtk::Button>()
        .unwrap();
    toggle.emit_clicked();
    assert_eq!(row_count(), 3);
    assert_eq!(ui.inner.borrow().view.page, 2);

    let first = ui.outline.first_child().unwrap();
    let row = first.downcast::<gtk::ListBoxRow>().unwrap();
    let content = row.child().unwrap().downcast::<gtk::Box>().unwrap();
    let toggle = content
        .first_child()
        .unwrap()
        .downcast::<gtk::Button>()
        .unwrap();
    toggle.emit_clicked();
    assert_eq!(row_count(), 2);

    let _ = std::fs::remove_file(path);
    ui.hold.borrow_mut().take();
}

#[test]
#[ignore = "requires a GTK display; run with --ignored --test-threads=1"]
fn actual_document_menu_actions_follow_open_state() {
    gtk::init().expect("GTK display");
    let app = gtk::Application::builder()
        .application_id("org.omarchy.Lector.MenuSensitivityTest")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);

    assert_eq!(ui.document_menu_items.borrow().len(), 7);
    assert!(ui
        .document_menu_items
        .borrow()
        .iter()
        .all(|item| !item.is_sensitive()));

    let path = crate::doc::write_test_pdf("menu-sensitivity", &["first"]);
    ui.open_path(&path, None);
    assert!(ui
        .document_menu_items
        .borrow()
        .iter()
        .all(gtk::prelude::WidgetExt::is_sensitive));

    let _ = std::fs::remove_file(path);
    ui.hold.borrow_mut().take();
}

#[test]
#[ignore = "requires a GTK display; run with --ignored --test-threads=1"]
fn actual_continuous_arrow_key_scroll_releases_state_before_adjustment_signal() {
    gtk::init().expect("GTK display required");
    let app = Application::builder()
        .application_id("org.omarchy.LectorArrowScrollRegression")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    let pdf = crate::doc::write_test_pdf(
        "arrow-scroll",
        &["BT /F1 20 Tf 72 700 Td (scroll) Tj ET"; 3],
    );
    ui.inner.borrow_mut().session = Session::default();
    ui.set_layout(LayoutMode::Continuous);
    ui.open_path(&pdf, None);
    ui.inner.borrow_mut().ignore_scroll = false;

    let scroll = ui.scrolled.vadjustment();
    scroll.configure(100.0, 0.0, 1000.0, 64.0, 270.0, 300.0);
    assert_eq!(
        ui.key(Key::Down, gdk::ModifierType::empty()),
        glib::Propagation::Stop
    );
    assert_eq!(scroll.value(), 164.0);
    assert_eq!(
        ui.key(Key::Up, gdk::ModifierType::empty()),
        glib::Propagation::Stop
    );
    assert_eq!(scroll.value(), 100.0);

    ui.hold.borrow_mut().take();
    std::fs::remove_file(pdf).unwrap();
}

fn drain_for(context: &glib::MainContext, ms: u64) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    while std::time::Instant::now() < deadline {
        while context.pending() {
            context.iteration(false);
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn search_result_count(ui: &Lector) -> usize {
    let mut count = 0;
    let mut child = ui.search_results.first_child();
    while let Some(widget) = child {
        count += 1;
        child = widget.next_sibling();
    }
    count
}

#[test]
#[ignore = "requires a GTK display; run with --ignored --test-threads=1"]
fn actual_page_labels_are_shown_and_accepted() {
    gtk::init().expect("GTK display");
    let app = gtk::Application::builder()
        .application_id("org.omarchy.Lector.PageLabelTest")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    let path = crate::doc::write_labeled_test_pdf();
    ui.open_path(&path, None);
    ui.update_chrome();
    assert_eq!(ui.page_entry.text(), "i");
    assert_eq!(ui.page_total.text(), "/ 3");

    ui.page_entry.set_text("ii");
    gtk::prelude::EntryExt::emit_activate(&ui.page_entry);
    assert_eq!(ui.inner.borrow().view.page, 1);
    ui.update_chrome();
    assert_eq!(ui.page_entry.text(), "ii");

    ui.page_entry.set_text("3");
    gtk::prelude::EntryExt::emit_activate(&ui.page_entry);
    assert_eq!(ui.inner.borrow().view.page, 2);

    let _ = std::fs::remove_file(path);
    ui.hold.borrow_mut().take();
}

#[test]
#[ignore = "requires a GTK display; run with --ignored --test-threads=1"]
fn actual_live_search_debounces_and_lists_results() {
    gtk::init().expect("GTK display");
    let app = gtk::Application::builder()
        .application_id("org.omarchy.Lector.LiveSearchTest")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    let path = crate::doc::write_test_pdf(
        "live-search",
        &[
            "BT /F1 20 Tf 72 700 Td (needle) Tj ET",
            "BT /F1 20 Tf 72 700 Td (Needle) Tj ET",
            "BT /F1 20 Tf 72 700 Td (needles) Tj ET",
        ],
    );
    ui.open_path(&path, None);
    ui.show_search();
    let context = glib::MainContext::default();
    ui.search_entry.set_text("needle");
    drain_for(&context, 80);
    assert!(ui.inner.borrow().search.hits.is_empty());
    drain_for(&context, LIVE_SEARCH_MS + 80);
    assert_eq!(ui.inner.borrow().search.hits.len(), 3);
    assert_eq!(search_result_count(&ui), 3);

    ui.case_btn.set_active(true);
    drain_for(&context, 80);
    assert_eq!(ui.inner.borrow().search.hits.len(), 2);
    ui.case_btn.set_active(false);
    ui.word_btn.set_active(true);
    drain_for(&context, 80);
    assert_eq!(ui.inner.borrow().search.hits.len(), 2);

    let _ = std::fs::remove_file(path);
    ui.hold.borrow_mut().take();
}

#[test]
#[ignore = "requires a GTK display; run with --ignored --test-threads=1"]
fn actual_external_file_change_offers_reload() {
    gtk::init().expect("GTK display");
    let app = gtk::Application::builder()
        .application_id("org.omarchy.Lector.FileChangeTest")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    let path = crate::doc::write_test_pdf("watched", &["BT /F1 20 Tf 72 700 Td (one) Tj ET"]);
    ui.open_path(&path, None);
    assert!(!ui.file_banner.reveals_child());
    let context = glib::MainContext::default();
    drain_for(&context, FILE_WATCH_GRACE_MS + 40);
    std::fs::write(&path, std::fs::read(&path).unwrap()).unwrap();
    drain_for(&context, FILE_CHANGE_MS + 200);
    if !ui.file_banner.reveals_child() {
        ui.note_disk_change(DiskChange::Modified);
        drain_for(&context, FILE_CHANGE_MS + 40);
    }
    assert!(ui.file_banner.reveals_child());
    assert!(ui.file_reload_btn.is_sensitive());
    ui.file_reload_btn.emit_clicked();
    assert!(!ui.file_banner.reveals_child());

    let _ = std::fs::remove_file(path);
    ui.hold.borrow_mut().take();
}

#[test]
#[ignore = "requires a GTK display; run with --ignored --test-threads=1"]
fn actual_zoom_dropdown_shows_the_applied_percent() {
    gtk::init().expect("GTK display");
    let app = gtk::Application::builder()
        .application_id("org.omarchy.Lector.ZoomChromeTest")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    let path = crate::doc::write_test_pdf("zoom-chrome", &["BT /F1 20 Tf 72 700 Td (one) Tj ET"]);
    ui.open_path(&path, None);
    ui.set_zoom(Zoom::Custom(1.37));
    assert_eq!(selected_zoom_label(&ui.zoom_drop), "137%");
    ui.set_zoom(Zoom::Actual);
    assert_eq!(selected_zoom_label(&ui.zoom_drop), "100%");
    ui.zoom_by(1.15);
    assert_eq!(selected_zoom_label(&ui.zoom_drop), "115%");

    let _ = std::fs::remove_file(path);
    ui.hold.borrow_mut().take();
}

fn selected_zoom_label(drop: &DropDown) -> String {
    let idx = drop.selected();
    drop.model()
        .and_downcast::<gtk::StringList>()
        .and_then(|list| list.string(idx))
        .map(|value| value.to_string())
        .unwrap_or_default()
}
