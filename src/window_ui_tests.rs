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
    ui.inner.borrow_mut().session = Session::default();
    let long =
        crate::doc::write_test_pdf("window-long", &["BT /F1 20 Tf 72 700 Td (needle) Tj ET"; 8]);
    let short = crate::doc::write_test_pdf(
        "window-short",
        &["BT /F1 20 Tf 72 700 Td (needle) Tj ET"; 2],
    );
    ui.open_path(&long, None);
    assert_eq!(ui.inner.borrow().doc.as_ref().unwrap().n_pages, 8);
    ui.set_layout(LayoutMode::Single);
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
