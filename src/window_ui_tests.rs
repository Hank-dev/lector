//! Display-backed checks run explicitly; no windows are presented or sessions saved.
use super::*;

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
        ui.prev_btn.upcast_ref::<gtk::Widget>(),
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
    ui.inner.borrow_mut().annotations = Some(AnnotationStore::default());
    ui.inner.borrow_mut().selection = Some(Selection {
        page: 0,
        x1: 70.0,
        y1: 70.0,
        x2: 160.0,
        y2: 110.0,
    });
    ui.update_annotation_actions();
    assert!(ui.highlight_item.is_sensitive());
    assert!(ui.note_item.is_sensitive());
    assert_eq!(ui.inner.borrow().doc.as_ref().unwrap().n_pages, 8);
    ui.follow_link(DocumentLink::GotoPage(3));
    assert_eq!(ui.inner.borrow().page, 3);
    ui.goto_page(0);
    assert!(ui.cont_item.is_active());
    assert!(!ui.dual_item.is_active());
    assert!(!ui.single_item.is_active());
    assert_eq!(
        ui.invert_item.is_active(),
        ui.inner.borrow().pdf_mode == PdfMode::Inverted,
        "inverted menu state must reflect the active preference"
    );
    assert_eq!(
        ui.normal_item.is_active(),
        ui.inner.borrow().pdf_mode == PdfMode::Normal,
        "normal menu state must reflect the active preference"
    );
    assert_eq!(
        ui.omarchy_item.is_active(),
        ui.inner.borrow().pdf_mode == PdfMode::Omarchy,
        "Omarchy menu state must reflect the active preference"
    );
    ui.dual_item.set_active(true);
    assert_eq!(ui.inner.borrow().layout_mode, LayoutMode::Dual);
    ui.omarchy_item.set_active(true);
    assert_eq!(ui.inner.borrow().pdf_mode, PdfMode::Omarchy);
    ui.normal_item.set_active(true);
    assert_eq!(ui.inner.borrow().pdf_mode, PdfMode::Normal);
    ui.invert_item.set_active(true);
    assert_eq!(ui.inner.borrow().pdf_mode, PdfMode::Inverted);
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
    assert_eq!(ui.inner.borrow().layout_mode, LayoutMode::Single);
    ui.toggle_presentation();
    assert!(!ui.inner.borrow().presentation);
    assert_eq!(ui.inner.borrow().layout_mode, LayoutMode::Dual);
    assert_eq!(ui.inner.borrow().zoom, Zoom::Custom(1.25));
    ui.goto_page(6);
    ui.reload();
    assert_eq!(ui.inner.borrow().page, 6);
    assert_eq!(ui.inner.borrow().zoom, Zoom::Custom(1.25));
    ui.search("needle");
    let previous_generation = ui.inner.borrow().search_gen;
    ui.open_path(&short, None);
    assert_ne!(ui.inner.borrow().search_gen, previous_generation);
    assert!(ui.inner.borrow().search_query.is_empty());
    assert_eq!(ui.search_info.text(), "");
    let context = glib::MainContext::default();
    for _ in 0..128 {
        if !context.pending() {
            break;
        }
        context.iteration(false);
    }
    assert!(ui.inner.borrow().search_hits.is_empty());
    ui.search("needle");
    for _ in 0..128 {
        if ui.search_info.text() == "1 of 2" {
            break;
        }
        context.iteration(false);
    }
    assert_eq!(ui.inner.borrow().search_hits.len(), 2);
    assert_eq!(ui.search_info.text(), "1 of 2");
    ui.set_layout(LayoutMode::Continuous);
    assert!(ui.pages_box.first_child().unwrap().is::<DrawingArea>());
    ui.set_layout(LayoutMode::Dual);
    assert!(ui.pages_box.first_child().unwrap().is::<GtkBox>());
    ui.open_path(&long, None);
    assert_eq!(ui.inner.borrow().page, 6);
    assert_eq!(ui.inner.borrow().zoom, Zoom::Custom(1.25));
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

    let page_before = ui.inner.borrow().page;
    let start_scale = ui.inner.borrow().layout.scale;
    let anchor = ui.capture_anchor(Some((300.0, 260.0)));
    let (first_zoom, first_restore) = pinch_zoom_update(start_scale, 1.2, anchor);
    ui.set_zoom_restore(first_zoom, first_restore);
    assert_eq!(
        ui.inner.borrow().page,
        page_before,
        "temporary adjustment signals during pinch reflow must not change pages"
    );
    let (final_zoom, final_restore) = pinch_zoom_update(start_scale, 1.5, anchor);
    ui.set_zoom_restore(final_zoom, final_restore);
    wait_for_view_restore(&ui, &context);
    let final_scale = ui.inner.borrow().layout.scale;
    let (_, expected_y) = restored_scroll(anchor, final_scale);
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
