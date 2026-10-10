//! Display-backed checks run explicitly on Broadway (see `init_test_gtk`);
//! no sessions are saved.
use super::*;

fn form_widgets(root: &gtk::Widget) -> Vec<gtk::Widget> {
    let mut result = vec![root.clone()];
    let mut child = root.first_child();
    while let Some(widget) = child {
        result.extend(form_widgets(&widget));
        child = widget.next_sibling();
    }
    result
}

#[test]
#[ignore = "requires a GTK display; run separately"]
fn actual_added_text_manager_deletes_selected_entry() {
    init_test_gtk();
    let app = Application::builder()
        .application_id("org.omarchy.LectorRemoveTextTest")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    ui.inner.borrow_mut().session = Session::default();
    let path = crate::doc::write_test_pdf("remove-text-ui", &[""]);
    ui.open_path(&path, None);
    let rect = crate::annotations::DocumentRect {
        x: 72.,
        y: 150.,
        width: 228.,
        height: 80.,
    };
    {
        let st = ui.inner.borrow();
        let doc = st.view.doc.as_ref().unwrap();
        doc.add_fill_text(0, rect, "Remove this").unwrap();
        doc.add_fill_text(0, rect, "Leave this").unwrap();
    }
    let manager = form_widgets(ui.window.upcast_ref())
        .into_iter()
        .filter_map(|w| w.downcast::<Button>().ok())
        .find(|b| widget_tree_has_label(b.upcast_ref(), "Added Text & Drawings…"))
        .expect("manager must be reachable from the menu");
    manager.emit_clicked();
    let dialog = gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Window>().ok())
        .find(|w| w.title().as_deref() == Some("Added Text & Drawings"))
        .expect("manager menu action must open its dialog");
    let buttons = form_widgets(dialog.upcast_ref())
        .into_iter()
        .filter_map(|w| w.downcast::<Button>().ok())
        .collect::<Vec<_>>();
    let remove = buttons
        .iter()
        .find(|b| b.tooltip_text().as_deref() == Some("Remove this"))
        .expect("each text entry should have its own delete action");
    remove.emit_clicked();
    assert!(ui.pdf_dirty.get());
    let copy = path.with_extension("filled.pdf");
    ui.save_copy_to(&copy).unwrap();
    let reopened = OpenDoc::open(&copy, None).unwrap();
    let entries = reopened.free_text_annotations();
    assert_eq!(entries.len(), 1);
    use poppler::prelude::*;
    assert_eq!(entries[0].1.contents().as_deref(), Some("Leave this"));
    dialog.destroy();
    ui.hold.borrow_mut().take();
    std::fs::remove_file(copy).unwrap();
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "requires a GTK display; run separately"]
fn actual_form_keyboard_shortcuts() {
    init_test_gtk();
    let app = Application::builder()
        .application_id("org.omarchy.LectorFormShortcutTest")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    ui.inner.borrow_mut().session = Session::default();
    let path = crate::form_tests::fixture();
    ui.open_path(&path, None);
    let mods = gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK;
    for key in [Key::t, Key::T] {
        assert_eq!(ui.key(key, mods), glib::Propagation::Stop);
        assert_eq!(
            ui.pages_box.cursor().and_then(|c| c.name()).as_deref(),
            Some("crosshair")
        );
        ui.key(Key::Escape, gdk::ModifierType::empty());
    }
    for key in [Key::f, Key::F] {
        assert_eq!(ui.key(key, mods), glib::Propagation::Stop);
        let dialog = gtk::Window::list_toplevels()
            .into_iter()
            .filter_map(|w| w.downcast::<gtk::Window>().ok())
            .find(|w| w.title().as_deref() == Some("Fill Form"))
            .expect("Ctrl+Shift+F must open Fill Form, not Find");
        dialog.destroy();
    }
    for label in ["Ctrl+Shift+T", "Ctrl+Shift+F"] {
        assert!(widget_tree_has_label(ui.window.upcast_ref(), label));
        let shortcuts = ui.shortcuts_dialog();
        assert!(widget_tree_has_label(shortcuts.upcast_ref(), label));
        shortcuts.destroy();
    }
    ui.hold.borrow_mut().take();
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "requires a GTK display; run separately"]
fn actual_blank_page_drag_opens_text_editor() {
    init_test_gtk();
    let app = Application::builder()
        .application_id("org.omarchy.LectorTextPlacementTest")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    ui.inner.borrow_mut().session = Session::default();
    ui.set_layout(LayoutMode::Single);
    let path = crate::doc::write_test_pdf("blank-drag", &[""]);
    ui.open_path(&path, None);
    ui.window.present();
    drain_for(&glib::MainContext::default(), 300);
    let button = form_widgets(ui.window.upcast_ref())
        .into_iter()
        .filter_map(|w| w.downcast::<Button>().ok())
        .find(|b| widget_tree_has_label(b.upcast_ref(), "Add Text…"))
        .unwrap();
    button.emit_clicked();
    assert_eq!(
        ui.pages_box.cursor().and_then(|c| c.name()).as_deref(),
        Some("crosshair"),
        "Add Text must arm visible placement without requiring an invisible prior selection"
    );
    let da = ui.page_widgets.borrow()[0].1.clone();
    let bounds = da.compute_bounds(&ui.pages_box).unwrap();
    let scale = ui.inner.borrow().view.layout.page_geom(0).unwrap().scale;
    let (x, y) = (
        bounds.x() as f64 + 72. * scale,
        bounds.y() as f64 + 150. * scale,
    );
    ui.drag_begin(x, y);
    assert!(
        gtk::Window::list_toplevels().into_iter().all(|w| {
            w.downcast::<gtk::Window>()
                .ok()
                .and_then(|w| w.title())
                .as_deref()
                != Some("Add Text")
        }),
        "a click must place the caret on the page, not open a text box"
    );
    assert_eq!(
        ui.pages_box.cursor().and_then(|c| c.name()).as_deref(),
        Some("text")
    );
    assert_eq!(
        ui.key(Key::Escape, gdk::ModifierType::empty()),
        glib::Propagation::Stop
    );
    assert!(ui.pages_box.cursor().is_none());
    assert!(ui.text_draft.borrow().is_none());
    ui.hold.borrow_mut().take();
    ui.window.set_visible(false);
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "requires a GTK display; run separately"]
fn actual_free_text_selection_editor() {
    init_test_gtk();
    let app = Application::builder()
        .application_id("org.omarchy.LectorFreeTextTest")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    ui.inner.borrow_mut().session = Session::default();
    ui.set_layout(LayoutMode::Single);
    let path = crate::doc::write_test_pdf("fill-ui", &[""]);
    ui.open_path(&path, None);
    ui.window.present();
    drain_for(&glib::MainContext::default(), 300);
    let caret = 12.0 * crate::view::FREE_TEXT_CARET_CENTER;
    let mut tops = Vec::new();
    for rotation in [0, 90, 180, 270] {
        ui.inner.borrow_mut().view.rotation = rotation;
        ui.reflow();
        // Page widgets report stale bounds until the reflow is allocated.
        drain_for(&glib::MainContext::default(), 600);
        // Separate lines: a click on earlier text would edit it instead.
        let click_y = 150. + rotation as f64;
        tops.push(792. - (click_y - caret));
        let (dx, dy) =
            crate::annotations::document_point_to_display(72., click_y, 612., 792., rotation);
        let da = ui.page_widgets.borrow()[0].1.clone();
        let bounds = da.compute_bounds(&ui.pages_box).unwrap();
        let scale = ui.inner.borrow().view.layout.page_geom(0).unwrap().scale;
        let button = form_widgets(ui.window.upcast_ref())
            .into_iter()
            .filter_map(|w| w.downcast::<Button>().ok())
            .find(|b| widget_tree_has_label(b.upcast_ref(), "Add Text…"))
            .expect("Add Text menu action missing");
        let key = {
            let st = ui.inner.borrow();
            RasterKey::from_geom(
                &st.view.layout.page_geom(0).unwrap(),
                st.view.rotation,
                st.view.pdf_mode,
                st.theme.pdf_palette(),
                ui.page_dpr(),
            )
        };
        assert!(ui.inner.borrow().raster.contains(&key));
        button.emit_clicked();
        ui.drag_begin(
            bounds.x() as f64 + dx * scale,
            bounds.y() as f64 + dy * scale,
        );
        assert_eq!(
            ui.key(Key::H, gdk::ModifierType::empty()),
            glib::Propagation::Stop
        );
        assert_eq!(
            ui.key(Key::i, gdk::ModifierType::empty()),
            glib::Propagation::Stop
        );
        assert_eq!(
            ui.key(Key::Return, gdk::ModifierType::empty()),
            glib::Propagation::Stop
        );
        assert_eq!(
            ui.inner
                .borrow()
                .view
                .doc
                .as_ref()
                .unwrap()
                .page(0)
                .unwrap()
                .annot_mapping()
                .len(),
            (rotation / 90 + 1) as usize
        );
        use glib::translate::ToGlibPtr;
        for mapping in ui
            .inner
            .borrow()
            .view
            .doc
            .as_ref()
            .unwrap()
            .page(0)
            .unwrap()
            .annot_mapping()
        {
            let raw: *mut poppler::ffi::PopplerAnnotMapping = mapping.to_glib_none().0;
            let area = unsafe { &(*raw).area };
            assert!(
                (area.x1 - 72.).abs() < 0.5,
                "rotation {rotation} text must start at the clicked document x, got {}",
                area.x1
            );
            assert!(
                tops.iter().any(|top| (area.y2 - top).abs() < 0.5),
                "rotation {rotation}: the caret must be centred on the click, got top {}",
                area.y2
            );
            assert!(area.x2 > area.x1 + 4.0);
            assert!(area.y2 > area.y1 + 4.0);
        }
        assert!(ui.window.title().unwrap().contains("Unsaved"));
        assert!(
            !ui.inner.borrow().raster.contains(&key),
            "rotation {rotation}: the edited page must re-render"
        );
    }
    let other = crate::doc::write_test_pdf("other-fill", &[""]);
    ui.open_path(&other, None);
    assert_eq!(ui.inner.borrow().view.doc.as_ref().unwrap().path, path);
    ui.reload();
    assert_eq!(
        ui.inner
            .borrow()
            .view
            .doc
            .as_ref()
            .unwrap()
            .page(0)
            .unwrap()
            .annot_mapping()
            .len(),
        4
    );
    assert!(ui.window.emit_by_name::<bool>("close-request", &[]));
    for dialog in gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Window>().ok())
        .filter(|w| w.title().as_deref() == Some("Unsaved PDF changes"))
    {
        dialog.destroy();
    }
    std::fs::remove_file(other).unwrap();
    ui.hold.borrow_mut().take();
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "requires a GTK display; run separately"]
fn actual_form_text_editor() {
    init_test_gtk();
    let app = Application::builder()
        .application_id("org.omarchy.LectorFormTest")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    ui.inner.borrow_mut().session = Session::default();
    ui.set_layout(LayoutMode::Single);
    let path = crate::form_tests::fixture();
    ui.open_path(&path, None);
    let button = form_widgets(ui.window.upcast_ref())
        .into_iter()
        .filter_map(|w| w.downcast::<Button>().ok())
        .find(|b| widget_tree_has_label(b.upcast_ref(), "Fill Form…"))
        .expect("Fill Form menu action missing");
    button.emit_clicked();
    let dialog = gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Window>().ok())
        .find(|w| w.title().as_deref() == Some("Fill Form"))
        .expect("form dialog");
    let entry = form_widgets(dialog.upcast_ref())
        .into_iter()
        .filter_map(|w| w.downcast::<Entry>().ok())
        .find(|e| e.widget_name() == "Name")
        .expect("name editor");
    entry.set_text("Filled answer");
    assert_eq!(
        crate::form_tests::field(ui.inner.borrow().view.doc.as_ref().unwrap(), "Name")
            .text_get_text()
            .as_deref(),
        Some("Filled answer")
    );
    let widgets = form_widgets(dialog.upcast_ref());
    let notes = widgets
        .iter()
        .filter_map(|w| w.downcast_ref::<gtk::TextView>())
        .find(|w| w.widget_name() == "Notes")
        .expect("multiline editor missing");
    notes.buffer().set_text("Line one\nLine two");
    let locked = widgets
        .iter()
        .filter_map(|w| w.downcast_ref::<Entry>())
        .find(|w| w.widget_name() == "Locked")
        .unwrap();
    assert!(!locked.is_sensitive());
    locked.set_text("must not change");
    let agree = widgets
        .iter()
        .filter_map(|w| w.downcast_ref::<CheckButton>())
        .find(|w| w.widget_name() == "Agree")
        .expect("checkbox editor");
    agree.set_active(true);
    let color = widgets
        .iter()
        .filter_map(|w| w.downcast_ref::<DropDown>())
        .find(|w| w.widget_name() == "Color")
        .expect("choice editor");
    color.set_selected(2);
    let many = widgets
        .iter()
        .filter_map(|w| w.downcast_ref::<CheckButton>())
        .filter(|w| w.widget_name() == "Many")
        .collect::<Vec<_>>();
    assert_eq!(many.len(), 3);
    many[0].set_active(true);
    many[2].set_active(true);
    let radios = widgets
        .iter()
        .filter_map(|w| w.downcast_ref::<CheckButton>())
        .filter(|w| w.widget_name() == "Radio")
        .collect::<Vec<_>>();
    assert_eq!(radios.len(), 2);
    radios[1].set_active(true);
    assert!(!radios[0].is_active());
    assert!(ui.pdf_dirty.get());
    assert!(ui.save_copy_to(&path.join("missing/output.pdf")).is_err());
    assert!(ui.pdf_dirty.get(), "failed save must retain dirty state");
    let copy = path.with_extension("filled.pdf");
    ui.save_copy_to(&copy).unwrap();
    assert!(!ui.pdf_dirty.get());
    let reopened = OpenDoc::open(&copy, None).unwrap();
    assert_eq!(
        crate::form_tests::field(&reopened, "Name")
            .text_get_text()
            .as_deref(),
        Some("Filled answer")
    );
    assert_eq!(
        crate::form_tests::field(&reopened, "Notes")
            .text_get_text()
            .as_deref(),
        Some("Line one\nLine two")
    );
    assert_eq!(
        crate::form_tests::field(&reopened, "Locked")
            .text_get_text()
            .as_deref(),
        Some("Keep")
    );
    assert!(crate::form_tests::field(&reopened, "Agree").button_get_state());
    assert!(crate::form_tests::field(&reopened, "Color").choice_is_item_selected(2));
    let selected = crate::form_tests::field(&reopened, "Many");
    assert!(selected.choice_is_item_selected(0));
    assert!(!selected.choice_is_item_selected(1));
    assert!(selected.choice_is_item_selected(2));
    let original = OpenDoc::open(&path, None).unwrap();
    assert_eq!(
        crate::form_tests::field(&original, "Name")
            .text_get_text()
            .as_deref(),
        Some("Original")
    );
    dialog.destroy();
    ui.hold.borrow_mut().take();
    std::fs::remove_file(copy).unwrap();
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "requires a GTK display; run separately"]
fn actual_adoption_invalidates_raster_lifetime() {
    init_test_gtk();
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
    init_test_gtk();
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
    init_test_gtk();
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
    init_test_gtk();
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
    init_test_gtk();
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
            ui.draw_page(&cr, 0, 1.0, geom.w as i32, geom.h as i32);
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
    init_test_gtk();
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
    init_test_gtk();
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
    assert!(
        ui.window.has_css_class("lector-presentation"),
        "presentation letterbox uses the canvas dark class"
    );
    assert_eq!(ui.inner.borrow().view.layout_mode, LayoutMode::Single);
    ui.toggle_presentation();
    assert!(!ui.inner.borrow().presentation);
    assert!(!ui.window.has_css_class("lector-presentation"));
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
    init_test_gtk();
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
    init_test_gtk();
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
    init_test_gtk();
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
    init_test_gtk();
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
    init_test_gtk();
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
    init_test_gtk();
    let app = gtk::Application::builder()
        .application_id("org.omarchy.Lector.MenuSensitivityTest")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);

    assert_eq!(ui.document_menu_items.borrow().len(), 11);
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
    init_test_gtk();
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

#[test]
#[ignore = "requires a GTK display; run separately"]
fn actual_draw_stroke_commits_and_can_be_removed() {
    init_test_gtk();
    let app = Application::builder()
        .application_id("org.omarchy.LectorDrawTest")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    ui.inner.borrow_mut().session = Session::default();
    ui.set_layout(LayoutMode::Single);
    let path = crate::doc::write_test_pdf("draw-ui", &[""]);
    let original = std::fs::read(&path).unwrap();
    ui.open_path(&path, None);
    {
        let mut st = ui.inner.borrow_mut();
        st.view.pdf_mode = PdfMode::Normal;
        st.raster.clear();
    }
    ui.window.present();
    drain_for(&glib::MainContext::default(), 300);
    let button = form_widgets(ui.window.upcast_ref())
        .into_iter()
        .filter_map(|w| w.downcast::<Button>().ok())
        .find(|b| widget_tree_has_label(b.upcast_ref(), "Draw…"))
        .expect("Draw menu action missing");
    button.emit_clicked();
    assert!(ui.draw_mode.get());
    assert_eq!(
        ui.pages_box.cursor().and_then(|c| c.name()).as_deref(),
        Some("crosshair")
    );
    assert!(ui.edit_bar.revealer.reveals_child());
    assert_eq!(ui.edit_bar.title.text().as_str(), "Draw");
    let da = ui.page_widgets.borrow()[0].1.clone();
    let bounds = da.compute_bounds(&ui.pages_box).unwrap();
    let scale = ui.inner.borrow().view.layout.page_geom(0).unwrap().scale;
    let (x, y) = (
        bounds.x() as f64 + 80. * scale,
        bounds.y() as f64 + 200. * scale,
    );
    ui.drag_begin(x, y);
    ui.drag_update(x + 160. * scale, y + 30. * scale);
    let mut surface =
        gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, da.width(), da.height())
            .unwrap();
    {
        let cr = gtk::cairo::Context::new(&surface).unwrap();
        cr.set_source_rgb(1., 1., 1.);
        cr.paint().unwrap();
        ui.draw_page(&cr, 0, 1., da.width(), da.height());
    }
    let stride = surface.stride() as usize;
    let data = surface.data().unwrap();
    let paper = data[8] as i32 + data[9] as i32 + data[10] as i32;
    let x0 = (80.0 * scale).round() as usize;
    let x1 = (240.0 * scale).round() as usize;
    let y0 = (196.0 * scale).round() as usize;
    let y1 = (236.0 * scale).round() as usize;
    let mut darkest = i32::MAX;
    for py in y0..y1 {
        for px in x0..x1 {
            let i = py * stride + px * 4;
            if i + 2 < data.len() {
                darkest = darkest.min(data[i] as i32 + data[i + 1] as i32 + data[i + 2] as i32);
            }
        }
    }
    assert!(
        paper - darkest > 80,
        "in-progress stroke must darken the page: darkest={darkest} paper={paper} scale={scale}"
    );
    drop(data);
    let controllers = ui.pages_box.observe_controllers();
    let drag = (0..controllers.n_items())
        .filter_map(|i| controllers.item(i).and_downcast::<GestureDrag>())
        .next()
        .unwrap();
    drag.emit_by_name::<()>("drag-end", &[&(160. * scale), &(30. * scale)]);
    assert_eq!(
        ui.inner
            .borrow()
            .view
            .doc
            .as_ref()
            .unwrap()
            .ink_annotations()
            .len(),
        1
    );
    assert!(
        ui.draw_mode.get(),
        "drawing stays armed for the next stroke"
    );
    assert!(ui.pdf_dirty.get());
    ui.drag_begin(x, y + 40. * scale);
    ui.drag_update(x + 80. * scale, y + 40. * scale);
    drag.emit_by_name::<()>("drag-end", &[&(80. * scale), &0.0_f64]);
    {
        let st = ui.inner.borrow();
        let doc = st.view.doc.as_ref().unwrap();
        assert_eq!(
            doc.ink_annotations().len(),
            1,
            "strokes on one page form one drawing"
        );
        assert_eq!(doc.ink_document_strokes(0).len(), 2);
    }
    let copy = path.with_extension("drawn.pdf");
    ui.save_copy_to(&copy).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), original);
    let reopened = OpenDoc::open(&copy, None).unwrap();
    assert_eq!(reopened.ink_annotations().len(), 1);
    assert_eq!(reopened.ink_document_strokes(0).len(), 2);
    let manager = form_widgets(ui.window.upcast_ref())
        .into_iter()
        .filter_map(|w| w.downcast::<Button>().ok())
        .find(|b| widget_tree_has_label(b.upcast_ref(), "Added Text & Drawings…"))
        .unwrap();
    manager.emit_clicked();
    let dialog = gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Window>().ok())
        .find(|w| w.title().as_deref() == Some("Added Text & Drawings"))
        .expect("the manager must open from the menu");
    assert!(widget_tree_has_label(
        dialog.upcast_ref(),
        "Drawing — 2 strokes"
    ));
    form_widgets(dialog.upcast_ref())
        .into_iter()
        .filter_map(|w| w.downcast::<Button>().ok())
        .find(|b| b.label().as_deref() == Some("Remove"))
        .unwrap()
        .emit_clicked();
    assert_eq!(
        ui.inner
            .borrow()
            .view
            .doc
            .as_ref()
            .unwrap()
            .ink_annotations()
            .len(),
        0
    );
    dialog.destroy();
    assert_eq!(
        ui.key(Key::Escape, gdk::ModifierType::empty()),
        glib::Propagation::Stop
    );
    assert!(!ui.draw_mode.get());
    assert!(ui.pages_box.cursor().is_none());
    ui.hold.borrow_mut().take();
    ui.window.set_visible(false);
    std::fs::remove_file(copy).unwrap();
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "requires a GTK display; run separately"]
fn actual_add_text_types_on_the_page_and_enter_commits() {
    init_test_gtk();
    let app = Application::builder()
        .application_id("org.omarchy.LectorInlineTextTest")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    ui.inner.borrow_mut().session = Session::default();
    ui.set_layout(LayoutMode::Single);
    let path = crate::doc::write_test_pdf("inline-text", &[""]);
    let original = std::fs::read(&path).unwrap();
    ui.open_path(&path, None);
    {
        let mut st = ui.inner.borrow_mut();
        st.view.pdf_mode = PdfMode::Normal;
        st.raster.clear();
    }
    ui.window.present();
    drain_for(&glib::MainContext::default(), 300);
    let button = form_widgets(ui.window.upcast_ref())
        .into_iter()
        .filter_map(|w| w.downcast::<Button>().ok())
        .find(|b| widget_tree_has_label(b.upcast_ref(), "Add Text…"))
        .expect("Add Text menu action missing");
    button.emit_clicked();
    let da = ui.page_widgets.borrow()[0].1.clone();
    let bounds = da.compute_bounds(&ui.pages_box).unwrap();
    let scale = ui.inner.borrow().view.layout.page_geom(0).unwrap().scale;
    let (x, y) = (
        bounds.x() as f64 + 72. * scale,
        bounds.y() as f64 + 150. * scale,
    );
    ui.drag_begin(x, y);
    let controllers = ui.pages_box.observe_controllers();
    let drag = (0..controllers.n_items())
        .filter_map(|i| controllers.item(i).and_downcast::<GestureDrag>())
        .next()
        .unwrap();
    drag.emit_by_name::<()>("drag-end", &[&0.0_f64, &0.0_f64]);
    assert!(
        gtk::Window::list_toplevels().into_iter().all(|w| {
            w.downcast::<gtk::Window>()
                .ok()
                .and_then(|w| w.title())
                .as_deref()
                != Some("Add Text")
        }),
        "typing must happen on the page, not in an Add Text dialog"
    );
    for key in [Key::H, Key::i] {
        assert_eq!(
            ui.key(key, gdk::ModifierType::empty()),
            glib::Propagation::Stop,
            "letter keys must type into the page"
        );
    }
    assert_eq!(
        ui.key(Key::space, gdk::ModifierType::empty()),
        glib::Propagation::Stop
    );
    assert_eq!(
        ui.inner.borrow().view.page,
        0,
        "space must insert a space, not turn the page"
    );
    assert_eq!(
        ui.key(Key::BackSpace, gdk::ModifierType::empty()),
        glib::Propagation::Stop
    );
    let mut surface =
        gtk::cairo::ImageSurface::create(gtk::cairo::Format::ARgb32, da.width(), da.height())
            .unwrap();
    {
        let cr = gtk::cairo::Context::new(&surface).unwrap();
        ui.draw_page(&cr, 0, 1., da.width(), da.height());
    }
    let stride = surface.stride() as usize;
    let data = surface.data().unwrap();
    let paper = data[8] as i32 + data[9] as i32 + data[10] as i32;
    let x0 = (72.0 * scale).round() as usize;
    let x1 = (110.0 * scale).round() as usize;
    let y0 = (150.0 * scale).round() as usize;
    let y1 = (168.0 * scale).round() as usize;
    let mut darkest = i32::MAX;
    for py in y0..y1 {
        for px in x0..x1 {
            let i = py * stride + px * 4;
            if i + 2 < data.len() {
                darkest = darkest.min(data[i] as i32 + data[i + 1] as i32 + data[i + 2] as i32);
            }
        }
    }
    assert!(
        paper - darkest > 80,
        "typed text must appear on the page before Enter: darkest={darkest} paper={paper}"
    );
    drop(data);
    assert_eq!(
        ui.key(Key::Return, gdk::ModifierType::empty()),
        glib::Propagation::Stop
    );
    use poppler::prelude::AnnotExt;
    let contents = ui
        .inner
        .borrow()
        .view
        .doc
        .as_ref()
        .unwrap()
        .free_text_annotations()
        .into_iter()
        .map(|(_, annot)| annot.contents().unwrap_or_default())
        .collect::<Vec<_>>();
    assert_eq!(contents, vec!["Hi".to_string()]);
    let copy = path.with_extension("filled.pdf");
    ui.save_copy_to(&copy).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(
        OpenDoc::open(&copy, None)
            .unwrap()
            .free_text_annotations()
            .len(),
        1
    );
    ui.hold.borrow_mut().take();
    ui.window.set_visible(false);
    std::fs::remove_file(path).unwrap();
    std::fs::remove_file(copy).unwrap();
}

fn markup_window(id: &str, name: &str) -> (Rc<Lector>, std::path::PathBuf) {
    init_test_gtk();
    let app = Application::builder()
        .application_id(id)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    ui.inner.borrow_mut().session = Session::default();
    ui.set_layout(LayoutMode::Single);
    let path = crate::doc::write_test_pdf(name, &[""]);
    ui.open_path(&path, None);
    ui.inner.borrow_mut().view.pdf_mode = PdfMode::Normal;
    ui.window.present();
    drain_for(&glib::MainContext::default(), 300);
    (ui, path)
}

/// Widget point of a document point on page 0 (rotation 0).
fn page_point(ui: &Lector, x: f64, y: f64) -> (f64, f64) {
    let da = ui.page_widgets.borrow()[0].1.clone();
    let bounds = da.compute_bounds(&ui.pages_box).unwrap();
    let scale = ui.inner.borrow().view.layout.page_geom(0).unwrap().scale;
    (bounds.x() as f64 + x * scale, bounds.y() as f64 + y * scale)
}

fn press(ui: &Lector, x: f64, y: f64) {
    let (x, y) = page_point(ui, x, y);
    ui.drag_begin(x, y);
    ui.release_text();
}

fn type_keys(ui: &Lector, keys: &[Key]) {
    for key in keys {
        assert_eq!(
            ui.key(*key, gdk::ModifierType::empty()),
            glib::Propagation::Stop
        );
    }
}

fn texts(ui: &Lector) -> Vec<String> {
    use poppler::prelude::AnnotExt;
    ui.inner
        .borrow()
        .view
        .doc
        .as_ref()
        .unwrap()
        .free_text_annotations()
        .into_iter()
        .map(|(_, annot)| annot.contents().unwrap_or_default().to_string())
        .collect()
}

fn draft_text(ui: &Lector) -> Option<String> {
    ui.text_draft.borrow().as_ref().map(|d| d.text.clone())
}

#[test]
#[ignore = "requires a GTK display; run separately"]
fn actual_text_tool_edits_commits_moves_and_undoes() {
    let (ui, path) = markup_window("org.omarchy.LectorTextToolTest", "text-tool");
    let ctrl = gdk::ModifierType::CONTROL_MASK;
    ui.add_text_dialog();
    assert!(ui.edit_bar.revealer.reveals_child());
    press(&ui, 100., 200.);
    assert!(
        ui.keys.im_context().is_some(),
        "typing must go through the IM"
    );
    type_keys(&ui, &[Key::H, Key::e, Key::l, Key::o, Key::Left, Key::l]);
    assert_eq!(draft_text(&ui).as_deref(), Some("Hello"));
    ui.im.emit_by_name::<()>("commit", &[&"ø"]);
    assert_eq!(
        draft_text(&ui).as_deref(),
        Some("Helløo"),
        "IM commits insert at the caret"
    );
    type_keys(&ui, &[Key::BackSpace, Key::End]);
    ui.pages_box.clipboard().set_text(" world");
    assert_eq!(ui.key(Key::v, ctrl), glib::Propagation::Stop);
    drain_for(&glib::MainContext::default(), 200);
    assert_eq!(draft_text(&ui).as_deref(), Some("Hello world"));
    type_keys(&ui, &[Key::BackSpace; 6]);

    // Clicking elsewhere keeps the text instead of dropping it.
    press(&ui, 100., 400.);
    assert_eq!(texts(&ui), vec!["Hello"]);
    assert_eq!(draft_text(&ui).as_deref(), Some(""));
    assert_eq!(
        ui.key(Key::Escape, gdk::ModifierType::empty()),
        glib::Propagation::Stop
    );
    assert!(!ui.text_placement.get());
    assert!(ui.keys.im_context().is_none());
    assert_eq!(texts(&ui), vec!["Hello"]);

    // Clicking added text reopens it for editing.
    ui.add_text_dialog();
    press(&ui, 104., 200.);
    assert_eq!(draft_text(&ui).as_deref(), Some("Hello"));
    assert!(
        texts(&ui).is_empty(),
        "the original is out of the PDF while edited"
    );
    type_keys(&ui, &[Key::End, Key::exclam]);
    ui.key(Key::Escape, gdk::ModifierType::empty());
    assert_eq!(texts(&ui), vec!["Hello!"]);
    assert_eq!(ui.key(Key::z, ctrl), glib::Propagation::Stop);
    assert_eq!(
        texts(&ui),
        vec!["Hello"],
        "undo restores the text before the edit"
    );
    ui.key(Key::z, ctrl);
    assert!(texts(&ui).is_empty(), "undo removes added text");

    // Dragging a box moves it; the moved text is placed on release.
    ui.add_text_dialog();
    press(&ui, 100., 300.);
    type_keys(&ui, &[Key::M, Key::o, Key::v, Key::e, Key::Return]);
    let (x, y) = page_point(&ui, 102., 300.);
    ui.drag_begin(x, y);
    let (x2, y2) = page_point(&ui, 202., 320.);
    ui.drag_update(x2, y2);
    ui.release_text();
    assert!(ui.text_draft.borrow().is_none());
    {
        let st = ui.inner.borrow();
        let doc = st.view.doc.as_ref().unwrap();
        let (_, spec) = doc
            .free_text_at(0, 205., 320.)
            .expect("text moved by the drag");
        assert_eq!(spec.text, "Move");
        assert!((spec.rect.x - 200.).abs() < 1.0, "x {}", spec.rect.x);
    }
    ui.finish_edit_mode();

    // Merely opening saved text for editing is not an unsaved change.
    let copy = path.with_extension("tool.pdf");
    ui.save_copy_to(&copy).unwrap();
    ui.open_path(&copy, None);
    assert!(!ui.pdf_dirty.get());
    ui.add_text_dialog();
    press(&ui, 205., 320.);
    assert_eq!(draft_text(&ui).as_deref(), Some("Move"));
    ui.key(Key::Escape, gdk::ModifierType::empty());
    assert_eq!(texts(&ui), vec!["Move"]);
    assert!(
        !ui.pdf_dirty.get(),
        "an unchanged edit must not mark the PDF unsaved"
    );

    ui.hold.borrow_mut().take();
    ui.window.set_visible(false);
    std::fs::remove_file(copy).unwrap();
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "requires a GTK display; run separately"]
fn actual_draw_tool_groups_by_pen_and_undoes_strokes() {
    let (ui, path) = markup_window("org.omarchy.LectorDrawToolTest", "draw-tool");
    let ctrl = gdk::ModifierType::CONTROL_MASK;
    let stroke = |ui: &Lector, y: f64| {
        let (x0, y0) = page_point(ui, 80., y);
        let (x1, y1) = page_point(ui, 200., y + 20.);
        ui.drag_begin(x0, y0);
        ui.drag_update(x1, y1);
        ui.finish_ink_stroke();
    };
    let counts = |ui: &Lector| {
        let st = ui.inner.borrow();
        let doc = st.view.doc.as_ref().unwrap();
        (
            doc.ink_annotations().len(),
            doc.ink_document_strokes(0).len(),
        )
    };
    ui.toggle_draw();
    stroke(&ui, 100.);
    stroke(&ui, 150.);
    assert_eq!(counts(&ui), (1, 2));
    let red = ui
        .edit_bar
        .colors
        .iter()
        .find(|(color, _)| *color == crate::window::edit_bar::PenColor::Red)
        .map(|(_, button)| button.clone())
        .unwrap();
    red.set_active(true);
    stroke(&ui, 200.);
    assert_eq!(counts(&ui), (2, 3), "a new pen starts a new drawing");
    let red_strokes = ui
        .inner
        .borrow()
        .view
        .doc
        .as_ref()
        .unwrap()
        .ink_document_strokes(0)
        .into_iter()
        .filter(|(style, _)| style.rgb.0 > 0.5)
        .count();
    assert_eq!(red_strokes, 1);
    assert_eq!(ui.key(Key::z, ctrl), glib::Propagation::Stop);
    assert_eq!(counts(&ui), (1, 2));
    ui.key(Key::z, ctrl);
    assert_eq!(counts(&ui), (1, 1));
    ui.key(Key::z, ctrl);
    assert_eq!(counts(&ui), (0, 0));
    assert!(!ui.edit_bar.undo.is_sensitive());
    ui.key(Key::Escape, gdk::ModifierType::empty());
    assert!(!ui.draw_mode.get());
    assert!(!ui.edit_bar.revealer.reveals_child());
    ui.hold.borrow_mut().take();
    ui.window.set_visible(false);
    std::fs::remove_file(path).unwrap();
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
    init_test_gtk();
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
    init_test_gtk();
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
    init_test_gtk();
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
    init_test_gtk();
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

#[test]
#[ignore = "requires a GTK display; run separately"]
fn actual_text_draft_survives_redo_double_click_and_print() {
    let (ui, path) = markup_window("org.omarchy.LectorDraftKeysTest", "draft-keys");
    let ctrl = gdk::ModifierType::CONTROL_MASK;
    ui.add_text_dialog();
    press(&ui, 100., 200.);
    type_keys(&ui, &[Key::H, Key::i]);
    assert_eq!(
        ui.key(Key::Z, ctrl | gdk::ModifierType::SHIFT_MASK),
        glib::Propagation::Stop
    );
    assert_eq!(
        draft_text(&ui).as_deref(),
        Some("Hi"),
        "Ctrl+Shift+Z (redo elsewhere) must not drop the typing"
    );
    let zoom = ui.inner.borrow().view.zoom;
    let (x, y) = page_point(&ui, 100., 200.);
    let first_was_link = Cell::new(false);
    ui.click_released(1, x, y, &first_was_link);
    ui.click_released(2, x, y, &first_was_link);
    assert_eq!(
        ui.inner.borrow().view.zoom,
        zoom,
        "a double-click while placing text must not switch the zoom"
    );
    assert!(ui.print_source().is_some());
    assert!(ui.text_draft.borrow().is_none());
    assert_eq!(texts(&ui), vec!["Hi"], "printing includes a new draft");
    // Re-opening an answer takes it out of the PDF until it is committed.
    press(&ui, 102., 202.);
    assert_eq!(draft_text(&ui).as_deref(), Some("Hi"));
    assert!(texts(&ui).is_empty());
    assert!(ui.print_source().is_some());
    assert_eq!(
        texts(&ui),
        vec!["Hi"],
        "printing includes an answer being edited"
    );
    ui.hold.borrow_mut().take();
    ui.window.set_visible(false);
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "requires a GTK display; run separately"]
fn actual_refused_document_keeps_unsaved_edits() {
    let (ui, path) = markup_window("org.omarchy.LectorRefusedDocTest", "refused-doc");
    ui.add_text_dialog();
    press(&ui, 100., 200.);
    type_keys(&ui, &[Key::H, Key::i]);
    ui.cancel_text_placement();
    assert!(ui.pdf_dirty.get());
    let empty = crate::doc::write_test_pdf("no-pages", &[]);
    let refused = OpenDoc::open(&empty, None).expect("Poppler opens a page-less PDF");
    assert_eq!(refused.n_pages, 0);
    ui.adopt_doc(refused);
    let toplevels = gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Window>().ok())
        .collect::<Vec<_>>();
    assert!(
        !toplevels
            .iter()
            .any(|w| w.title().as_deref() == Some("Unsaved PDF changes")),
        "a document that cannot be shown must not offer to discard edits"
    );
    assert!(ui.pdf_dirty.get(), "the edits stay marked unsaved");
    assert_eq!(
        ui.inner.borrow().view.doc.as_ref().map(|d| d.path.clone()),
        Some(path.canonicalize().unwrap())
    );
    for window in toplevels {
        if window != ui.window.clone().upcast::<gtk::Window>() {
            window.destroy();
        }
    }
    ui.hold.borrow_mut().take();
    ui.window.set_visible(false);
    std::fs::remove_file(empty).unwrap();
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "requires a GTK display; run separately"]
fn actual_shifted_shortcuts_reload_and_search_back() {
    let (ui, path) = markup_window("org.omarchy.LectorShiftedKeysTest", "shifted-keys");
    let mods = gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK;
    {
        let mut st = ui.inner.borrow_mut();
        st.search.hits = vec![(0, poppler::Rectangle::new()); 3];
        st.search.idx = Some(0);
    }
    assert_eq!(ui.key(Key::G, mods), glib::Propagation::Stop);
    assert_eq!(
        ui.inner.borrow().search.idx,
        Some(2),
        "Ctrl+Shift+G steps back"
    );
    let before = ui
        .inner
        .borrow()
        .view
        .doc
        .as_ref()
        .unwrap()
        .document
        .clone();
    assert_eq!(ui.key(Key::R, mods), glib::Propagation::Stop);
    let st = ui.inner.borrow();
    assert_ne!(
        st.view.doc.as_ref().unwrap().document,
        before,
        "Ctrl+Shift+R reloads"
    );
    assert_eq!(st.view.rotation, 0, "Ctrl+Shift+R does not rotate");
    drop(st);
    ui.hold.borrow_mut().take();
    ui.window.set_visible(false);
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "requires a GTK display; run separately"]
fn actual_edit_rerenders_only_its_page_and_thumbnail() {
    let (ui, path) = markup_window("org.omarchy.LectorEditRasterTest", "edit-raster");
    let context = glib::MainContext::default();
    ui.set_sidebar_visible(true);
    ui.stack.set_visible_child_name("thumbs");
    drain_for(&context, 300);
    let picture = |ui: &Lector| {
        ui.thumbs
            .row_at_index(0)
            .and_then(|row| row.child())
            .and_then(|col| col.first_child())
            .filter(|child| child.is::<gtk::Picture>())
    };
    let row = ui.thumbs.row_at_index(0).expect("thumbnail row");
    let old_picture = picture(&ui).expect("thumbnail rendered");
    let key = {
        let st = ui.inner.borrow();
        RasterKey::from_geom(
            &st.view.layout.page_geom(0).unwrap(),
            st.view.rotation,
            st.view.pdf_mode,
            st.theme.pdf_palette(),
            ui.page_dpr(),
        )
    };
    assert!(ui.inner.borrow().raster.contains(&key));
    ui.inner
        .borrow()
        .view
        .doc
        .as_ref()
        .unwrap()
        .add_ink_stroke(0, &[(100., 100.), (200., 150.)])
        .unwrap();
    ui.pdf_changed(0);
    {
        let mut st = ui.inner.borrow_mut();
        assert!(!st.raster.contains(&key), "the edited page re-renders");
        assert!(
            st.raster.get_page(&key).is_some(),
            "its old pixels stay on screen meanwhile"
        );
    }
    drain_for(&context, 300);
    assert!(ui.inner.borrow().raster.contains(&key));
    assert_eq!(
        ui.thumbs.row_at_index(0).unwrap(),
        row,
        "an edit must not rebuild the thumbnail list"
    );
    let new_picture = picture(&ui).expect("thumbnail re-rendered");
    assert_ne!(new_picture, old_picture);
    ui.hold.borrow_mut().take();
    ui.window.set_visible(false);
    std::fs::remove_file(path).unwrap();
}

fn search_window(id: &str, name: &str, pages: &[&str]) -> (Rc<Lector>, std::path::PathBuf) {
    init_test_gtk();
    let app = Application::builder()
        .application_id(id)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    ui.inner.borrow_mut().session = Session::default();
    let path = crate::doc::write_test_pdf(name, pages);
    ui.open_path(&path, None);
    (ui, path)
}

#[test]
#[ignore = "requires a GTK display; run separately"]
fn actual_search_results_grow_without_rebuilding() {
    let page = "BT /F1 20 Tf 72 700 Td (needle) Tj 0 -40 Td (needle) Tj ET";
    let (ui, path) = search_window(
        "org.omarchy.LectorSearchGrowTest",
        "search-grow",
        &[page; 40],
    );
    let context = glib::MainContext::default();
    ui.search("needle");
    let first = ui
        .search_results
        .first_child()
        .expect("the first page's hits are listed at once");
    drain_for(&context, 1500);
    assert_eq!(ui.inner.borrow().search.hits.len(), 80);
    assert_eq!(search_result_count(&ui), 80);
    assert_eq!(
        ui.search_results.first_child().unwrap(),
        first,
        "rows are appended as pages are searched, not rebuilt"
    );
    assert_eq!(
        ui.inner.borrow().search.snippets.len(),
        40,
        "each page's text is read once"
    );
    ui.search_step(1);
    assert_eq!(
        ui.search_results.selected_row().map(|row| row.index()),
        Some(1)
    );
    ui.search("zzz");
    drain_for(&context, 300);
    assert_eq!(search_result_count(&ui), 0, "a new search drops old rows");
    ui.hold.borrow_mut().take();
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "requires a GTK display; run separately"]
fn actual_single_page_search_jump_shows_the_hit() {
    let (ui, path) = search_window(
        "org.omarchy.LectorSearchJumpTest",
        "search-jump",
        &[
            "BT /F1 20 Tf 72 700 Td (start) Tj ET",
            "BT /F1 20 Tf 480 40 Td (needle) Tj ET",
        ],
    );
    let context = glib::MainContext::default();
    ui.set_layout(LayoutMode::Single);
    // Zoom before the first allocation: Broadway without a client never
    // re-allocates, and both pages share one size.
    ui.set_zoom(Zoom::Custom(3.0));
    ui.window.present();
    drain_for(&context, 300);
    ui.search("needle");
    drain_for(&context, 500);
    let st = ui.inner.borrow();
    assert_eq!(st.view.page, 1);
    let (_, rect) = st.search.hits[0];
    let geom = st.view.layout.page_geom(1).expect("hit page laid out");
    let local = crate::view::PageGeom {
        x: 0.0,
        y: 0.0,
        ..geom
    };
    let (x, y, w, h) =
        crate::view::pdf_rect_to_view(&local, st.view.doc.as_ref().unwrap(), 0, &rect);
    let (left, top) = (geom.x + x, geom.y + y);
    let hadj = ui.scrolled.hadjustment();
    let vadj = ui.scrolled.vadjustment();
    assert!(
        vadj.page_size() < geom.h && hadj.page_size() < geom.w,
        "fixture must scroll both ways"
    );
    assert!(
        top >= vadj.value() && top + h <= vadj.value() + vadj.page_size(),
        "hit rows {top}..{} outside view {}..{} (upper {})",
        top + h,
        vadj.value(),
        vadj.value() + vadj.page_size(),
        vadj.upper()
    );
    assert!(
        left >= hadj.value() && left + w <= hadj.value() + hadj.page_size(),
        "hit columns {left}..{} outside view {}..{}",
        left + w,
        hadj.value(),
        hadj.value() + hadj.page_size()
    );
    drop(st);
    ui.hold.borrow_mut().take();
    ui.window.set_visible(false);
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "requires a GTK display; run separately"]
fn actual_viewport_resize_reflows_without_frame_polling() {
    let (ui, path) = markup_window("org.omarchy.LectorResizeTest", "resize");
    let context = glib::MainContext::default();
    ui.set_zoom(Zoom::FitWidth);
    drain_for(&context, 300);
    let before = ui.inner.borrow().view.view_w;
    // Broadway without a client runs no frames, so allocate a narrower
    // window directly; the viewport then reports its new page size.
    ui.window
        .allocate(before as i32 - 200, ui.window.height(), -1, None);
    // The next layout pass restores the real surface size, so record the
    // narrowest width the layout saw. Wait long enough for a resize that a
    // running view restore deferred.
    let mut after = before;
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1200);
    while after >= before - 100.0 && std::time::Instant::now() < deadline {
        while context.pending() {
            context.iteration(false);
        }
        after = after.min(ui.inner.borrow().view.view_w);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(
        after < before - 100.0,
        "a narrower viewport must reflow: {before} -> {after}"
    );
    // A resize that arrives while a view restore runs is taken afterwards.
    ui.inner.borrow_mut().ignore_scroll = true;
    assert!(!ui.on_resize(before, 600.0), "deferred during a restore");
    assert_eq!(ui.inner.borrow().view.view_w, after);
    ui.inner.borrow_mut().ignore_scroll = false;
    assert!(ui.on_resize(before, 600.0), "taken once the restore ends");
    ui.hold.borrow_mut().take();
    ui.window.set_visible(false);
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "requires a GTK display; run separately"]
fn actual_unreadable_session_is_reported_and_kept() {
    let dir = std::env::temp_dir().join(format!("lector-config-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("lector")).unwrap();
    let file = dir.join("lector").join("session.toml");
    std::fs::write(&file, "sidebar = 3\n").unwrap();
    // One test per process, so pointing the config here affects only this test.
    std::env::set_var("XDG_CONFIG_HOME", &dir);
    init_test_gtk();
    let app = Application::builder()
        .application_id("org.omarchy.LectorBadSessionTest")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    assert!(
        ui.inner
            .borrow()
            .session_error
            .as_deref()
            .is_some_and(|error| error.contains("session.toml")),
        "the unreadable file must be reported when the window opens"
    );
    ui.persist();
    assert_eq!(std::fs::read(&file).unwrap(), b"sidebar = 3\n");
    for window in gtk::Window::list_toplevels() {
        if let Ok(window) = window.downcast::<gtk::Window>() {
            window.destroy();
        }
    }
    ui.hold.borrow_mut().take();
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
#[ignore = "requires a GTK display; run separately"]
fn actual_closing_a_presentation_keeps_the_reading_view() {
    let (ui, path) = search_window(
        "org.omarchy.LectorPresentationCloseTest",
        "pres-close",
        &[""; 3],
    );
    ui.set_layout(LayoutMode::Continuous);
    ui.set_zoom(Zoom::Custom(1.5));
    ui.toggle_presentation();
    assert!(ui.inner.borrow().presentation);
    ui.record_view_in_session();
    {
        let st = ui.inner.borrow();
        assert_eq!(st.session.layout, "continuous");
        let doc_path = st.view.doc.as_ref().unwrap().path.clone();
        let saved = st.session.restore(&doc_path).expect("document remembered");
        assert_eq!(
            session::parse_zoom(&saved.zoom, saved.custom),
            Zoom::Custom(1.5)
        );
    }
    ui.toggle_presentation();
    ui.hold.borrow_mut().take();
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "requires a GTK display; run separately"]
fn actual_rotation_redraws_thumbnails() {
    let (ui, path) = markup_window("org.omarchy.LectorThumbRotationTest", "thumb-rotation");
    let context = glib::MainContext::default();
    ui.set_sidebar_visible(true);
    ui.stack.set_visible_child_name("thumbs");
    drain_for(&context, 300);
    let picture = |ui: &Lector| {
        ui.thumbs
            .row_at_index(0)
            .and_then(|row| row.child())
            .and_then(|col| col.first_child())
            .and_then(|child| child.downcast::<gtk::Picture>().ok())
    };
    let size = |picture: &gtk::Picture| {
        let paintable = picture.paintable().expect("thumbnail texture");
        (paintable.intrinsic_width(), paintable.intrinsic_height())
    };
    let (w, h) = size(&picture(&ui).expect("thumbnail rendered"));
    assert!(w < h, "portrait page");
    ui.rotate(90);
    drain_for(&context, 300);
    let (w, h) = size(&picture(&ui).expect("thumbnail rendered again"));
    assert!(w > h, "the thumbnail turns with the page: {w}x{h}");
    ui.hold.borrow_mut().take();
    ui.window.set_visible(false);
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "requires a GTK display; run separately"]
fn actual_small_zoom_keeps_every_nearby_page_rendered() {
    let (ui, path) = search_window("org.omarchy.LectorSmallZoomTest", "small-zoom", &[""; 60]);
    let context = glib::MainContext::default();
    ui.set_layout(LayoutMode::Continuous);
    // Zoom before the first allocation: Broadway without a client never
    // re-allocates.
    ui.set_zoom(Zoom::Custom(0.1));
    ui.window.present();
    drain_for(&context, 500);
    // Mid-document, pages within the prefetch margin on both sides count.
    ui.scrolled.vadjustment().set_value(2000.0);
    drain_for(&context, 1500);
    let view_top = ui.scrolled.vadjustment().value();
    let view_h = ui.scrolled.height() as f64;
    let dpr = ui.page_dpr();
    let mut st = ui.inner.borrow_mut();
    let near = pages_intersecting_view(&st.view.layout.pages, view_top, view_h, VIEW_PAGE_PAD);
    assert!(
        near.len() > RASTER_CACHE_PAGES,
        "fixture must need more pages than the base cache: {}",
        near.len()
    );
    let st = &mut *st;
    let missing = near
        .iter()
        .filter(|index| {
            let geom = st.view.layout.page_geom(**index).unwrap();
            let key = RasterKey::from_geom(
                &geom,
                st.view.rotation,
                st.view.pdf_mode,
                st.theme.pdf_palette(),
                dpr,
            );
            !st.raster.contains(&key)
        })
        .count();
    assert_eq!(missing, 0, "pages near the view were evicted");
    ui.hold.borrow_mut().take();
    ui.window.set_visible(false);
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore = "requires a GTK display; run separately"]
fn actual_dragged_selection_updates_annotation_items_when_the_menu_opens() {
    let (ui, path) = search_window(
        "org.omarchy.LectorDragSelectTest",
        "drag-select",
        &["BT /F1 20 Tf 72 700 Td (needle in text) Tj ET"],
    );
    let context = glib::MainContext::default();
    ui.inner.borrow_mut().annotations = Some(AnnotationStore::default());
    ui.window.present();
    drain_for(&context, 300);
    let (x1, y1) = page_point(&ui, 70., 75.);
    let (x2, y2) = page_point(&ui, 220., 100.);
    ui.drag_begin(x1, y1);
    ui.drag_update(x2, y2);
    assert!(
        !ui.highlight_item.is_sensitive(),
        "menu rows are not recomputed on every pointer motion"
    );
    let popover = form_widgets(ui.window.upcast_ref())
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::MenuButton>().ok())
        .find_map(|button| button.popover())
        .expect("main menu");
    popover.popup();
    drain_for(&context, 100);
    assert!(
        ui.highlight_item.is_sensitive(),
        "refreshed when the menu opens"
    );
    assert!(ui.note_item.is_sensitive());
    popover.popdown();
    ui.hold.borrow_mut().take();
    ui.window.set_visible(false);
    std::fs::remove_file(path).unwrap();
}
