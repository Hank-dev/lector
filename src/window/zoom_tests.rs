use super::*;

#[test]
#[ignore = "requires a GTK display; run separately on one test thread"]
fn actual_pinch_reuses_raster_until_end() {
    init_test_gtk();
    let app = Application::builder()
        .application_id("org.omarchy.Lector.PinchRasterTest")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    ui.inner.borrow_mut().session = Session::default();
    ui.set_layout(LayoutMode::Single);
    let pdf = crate::doc::write_test_pdf("pinch-raster", &["BT /F1 20 Tf 72 700 Td (pinch) Tj ET"]);
    ui.open_path(&pdf, None);
    ui.set_zoom(Zoom::Actual);
    settle();
    ui.pinch_active.set(true);
    ui.set_zoom(Zoom::Custom(2.0));
    settle();
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
    assert!(
        ui.inner.borrow_mut().raster.get_page(&key).is_some(),
        "preview must remain available"
    );
    assert!(
        !ui.inner.borrow().raster.contains(&key),
        "do not reraster an existing page while fingers are moving"
    );
    ui.finish_pinch();
    settle();
    assert!(
        ui.inner.borrow().raster.contains(&key),
        "gesture end must sharpen to exact size"
    );
    ui.hold.borrow_mut().take();
    std::fs::remove_file(pdf).unwrap();
}

#[test]
fn dual_zoom_anchor_uses_page_under_pointer() {
    let left = crate::view::PageGeom {
        index: 0,
        x: 16.0,
        y: 16.0,
        w: 600.0,
        h: 800.0,
        scale: 1.0,
    };
    let right = crate::view::PageGeom {
        index: 1,
        x: 632.0,
        ..left
    };
    let anchor = anchor_from_content(&[left, right], 932.0, 416.0, 100.0, 100.0);
    assert_eq!(anchor.page, 1);
    assert_eq!(anchor.rel_x, 0.5);
    assert_eq!(anchor.rel_y, 0.5);
}

#[test]
fn dual_zoom_anchor_miss_stays_on_the_nearest_page() {
    let left = crate::view::PageGeom {
        index: 0,
        x: 16.0,
        y: 16.0,
        w: 600.0,
        h: 800.0,
        scale: 1.0,
    };
    let right = crate::view::PageGeom {
        index: 1,
        x: 632.0,
        h: 400.0,
        ..left
    };
    let margin = anchor_from_content(&[left, right], 1300.0, 200.0, 100.0, 100.0);
    assert_eq!(margin.page, 1);
    assert_eq!(margin.rel_x, 1.0);
    let beside = anchor_from_content(&[left, right], 932.0, 500.0, 100.0, 100.0);
    assert_eq!(beside.page, 1);
    assert_eq!(beside.rel_x, 0.5);
    assert_eq!(beside.rel_y, 1.0);
    let gutter = anchor_from_content(&[left, right], 628.0, 200.0, 80.0, 40.0);
    assert_eq!(gutter.page, 1);
    assert_eq!(gutter.rel_x, 0.0);
    let zoomed_right = crate::view::PageGeom {
        w: 1200.0,
        h: 800.0,
        scale: 2.0,
        ..right
    };
    let (x, _) = restored_scroll(
        gutter,
        &[
            crate::view::PageGeom {
                w: 1200.0,
                h: 1600.0,
                scale: 2.0,
                ..left
            },
            zoomed_right,
        ],
    );
    assert!(
        (x - (zoomed_right.x - gutter.view_x)).abs() < 0.5,
        "gutter anchor drifted with scale: {x}"
    );

    let lower = crate::view::PageGeom {
        index: 1,
        y: left.y + left.h + 16.0,
        ..left
    };
    let gap = anchor_from_content(&[left, lower], 316.0, lower.y - 4.0, 10.0, 10.0);
    assert_eq!(gap.page, 1);
    assert_eq!(gap.rel_y, 0.0);
}

#[test]
#[ignore = "requires a GTK display; run separately on one test thread"]
fn actual_pinch_commits_small_final_scale() {
    init_test_gtk();
    let app = Application::builder()
        .application_id("org.omarchy.Lector.PinchFinalTest")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    ui.inner.borrow_mut().session = Session::default();
    let pdf = crate::doc::write_test_pdf("pinch-final", &["pinch"]);
    ui.open_path(&pdf, None);
    ui.set_zoom(Zoom::Actual);
    settle();
    let controllers = ui.scrolled.observe_controllers();
    let pinch = (0..controllers.n_items())
        .find_map(|i| controllers.item(i)?.downcast::<GestureZoom>().ok())
        .unwrap();
    pinch.emit_by_name::<()>("begin", &[&None::<gdk::EventSequence>]);
    pinch.emit_by_name::<()>("scale-changed", &[&1.015_f64]);
    pinch.emit_by_name::<()>("end", &[&None::<gdk::EventSequence>]);
    assert!(
        (ui.inner.borrow().view.layout.scale - 1.015).abs() < 1e-9,
        "pinch end must apply the final scale even below the old 2% threshold"
    );
    assert!(!ui.pinch_active.get());
    settle();
    pinch.emit_by_name::<()>("begin", &[&None::<gdk::EventSequence>]);
    let gen = ui.inner.borrow().layout_gen;
    for step in 1..=100 {
        pinch.emit_by_name::<()>("scale-changed", &[&(1.0 + f64::from(step) * 0.01)]);
    }
    assert_eq!(
        ui.inner.borrow().layout_gen,
        gen,
        "a burst of pinch events must wait for one frame, not reflow per event"
    );
    pinch.emit_by_name::<()>("end", &[&None::<gdk::EventSequence>]);
    assert!((ui.inner.borrow().view.layout.scale - 2.03).abs() < 1e-9);
    assert_eq!(ui.inner.borrow().layout_gen, gen + 1);
    settle();
    assert_eq!(
        ui.inner.borrow().layout_gen,
        gen + 1,
        "stale frame must not reapply"
    );
    ui.hold.borrow_mut().take();
    std::fs::remove_file(pdf).unwrap();
}

#[test]
#[ignore = "requires a GTK display; run separately on one test thread"]
fn single_to_continuous_restores_late_page_after_allocation() {
    init_test_gtk();
    let app = Application::builder()
        .application_id("org.omarchy.Lector.LayoutRestoreTest")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    ui.inner.borrow_mut().session = Session::default();
    ui.set_layout(LayoutMode::Single);
    let pdf = crate::doc::write_test_pdf("layout-restore", &["late page"; 40]);
    ui.window.present();
    ui.open_path(&pdf, None);
    ui.set_zoom(Zoom::Actual);
    ui.goto_page(25);
    settle();
    assert!(ui.scrolled.is_mapped());
    assert_eq!(ui.inner.borrow().view.page, 25);
    let old_upper = ui.scrolled.vadjustment().upper();
    ui.set_layout(LayoutMode::Continuous);
    let target = page_scroll_y(&ui.inner.borrow().view.layout.pages, 25);
    assert!(
        target > old_upper,
        "destination must require the new canvas allocation"
    );
    settle();
    assert!(
        (ui.scrolled.vadjustment().value() - target).abs() < 1.0,
        "layout switch lost the selected page: actual={}, target={target}",
        ui.scrolled.vadjustment().value()
    );
    assert_eq!(ui.inner.borrow().view.page, 25);
    assert!(ui.visible_canvas_pages().contains(&25));
    assert!(ui.page_widgets.borrow().iter().any(|(page, _)| *page == 25));
    ui.window.set_visible(false);
    ui.hold.borrow_mut().take();
    std::fs::remove_file(pdf).unwrap();
}

#[test]
#[ignore = "requires a GTK display; run separately on one test thread"]
fn continuous_goto_reaches_page_beyond_stale_adjustment() {
    init_test_gtk();
    let app = Application::builder()
        .application_id("org.omarchy.Lector.GotoStaleUpperTest")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    ui.inner.borrow_mut().session = Session::default();
    ui.set_layout(LayoutMode::Continuous);
    let pdf = crate::doc::write_test_pdf("goto-stale-upper", &["late page"; 40]);
    ui.window.present();
    ui.open_path(&pdf, None);
    ui.set_zoom(Zoom::Custom(0.2));
    settle();
    assert!(ui.scrolled.is_mapped());
    let old_upper = ui.scrolled.vadjustment().upper();
    ui.set_zoom(Zoom::Actual);
    let target = page_scroll_y(&ui.inner.borrow().view.layout.pages, 30);
    assert!(
        target > old_upper,
        "destination must still be past the pre-zoom adjustment"
    );
    ui.goto_page(30);
    assert!(
        target > ui.scrolled.vadjustment().upper() - ui.scrolled.vadjustment().page_size(),
        "goto must run before the taller canvas is allocated"
    );
    settle();
    assert_eq!(ui.inner.borrow().view.page, 30);
    assert!(
        (ui.scrolled.vadjustment().value() - target).abs() < 1.0,
        "continuous jump stayed on the stale upper: actual={}, target={target}",
        ui.scrolled.vadjustment().value()
    );
    ui.window.set_visible(false);
    ui.hold.borrow_mut().take();
    std::fs::remove_file(pdf).unwrap();
}

#[test]
#[ignore = "requires a GTK display; run separately on one test thread"]
fn mapped_pinch_ticks_before_end_and_discards_finished_targets() {
    init_test_gtk();
    let app = Application::builder()
        .application_id("org.omarchy.Lector.PinchTickTest")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    ui.inner.borrow_mut().session = Session::default();
    ui.set_layout(LayoutMode::Single);
    let pdf = crate::doc::write_test_pdf("pinch-tick", &["pinch"]);
    ui.window.present();
    ui.open_path(&pdf, None);
    ui.set_zoom(Zoom::Actual);
    settle();
    assert!(ui.scrolled.is_mapped());
    let controllers = ui.scrolled.observe_controllers();
    let pinch = (0..controllers.n_items())
        .find_map(|i| controllers.item(i)?.downcast::<GestureZoom>().ok())
        .unwrap();
    pinch.emit_by_name::<()>("begin", &[&None::<gdk::EventSequence>]);
    let gen = ui.inner.borrow().layout_gen;
    for scale in [1.01_f64, 1.02, 1.03] {
        pinch.emit_by_name::<()>("scale-changed", &[&scale]);
    }
    assert_eq!(ui.inner.borrow().layout_gen, gen);
    settle();
    assert!(ui.pinch_active.get(), "no end signal has been delivered");
    assert!((ui.inner.borrow().view.layout.scale - 1.03).abs() < 1e-9);
    assert_eq!(
        ui.inner.borrow().layout_gen,
        gen + 1,
        "one tick applies only the latest target"
    );

    for finish in ["end", "cancel"] {
        // Leave a tick queued, finish synchronously, and start the next gesture
        // before the old tick gets a chance to run on the mapped widget.
        pinch.emit_by_name::<()>("scale-changed", &[&1.2_f64]);
        pinch.emit_by_name::<()>(finish, &[&None::<gdk::EventSequence>]);
        assert!(!ui.pinch_active.get());
        let final_scale = ui.inner.borrow().view.layout.scale;
        let gen = ui.inner.borrow().layout_gen;
        pinch.emit_by_name::<()>("begin", &[&None::<gdk::EventSequence>]);
        settle();
        assert_eq!(
            ui.inner.borrow().layout_gen,
            gen,
            "{finish}: stale tick replayed a finished target"
        );
        assert_eq!(ui.inner.borrow().view.layout.scale, final_scale);
        assert!(ui.pinch_active.get());

        pinch.emit_by_name::<()>("scale-changed", &[&1.05_f64]);
        pinch.emit_by_name::<()>("scale-changed", &[&1.08_f64]);
        settle();
        assert!((ui.inner.borrow().view.layout.scale - final_scale * 1.08).abs() < 1e-9);
        assert_eq!(
            ui.inner.borrow().layout_gen,
            gen + 1,
            "{finish}: next gesture must still tick exactly once"
        );
    }
    pinch.emit_by_name::<()>("end", &[&None::<gdk::EventSequence>]);
    settle();
    ui.window.set_visible(false);
    ui.hold.borrow_mut().take();
    std::fs::remove_file(pdf).unwrap();
}

fn settle() {
    let context = glib::MainContext::default();
    let deadline = Instant::now() + Duration::from_millis(220);
    while Instant::now() < deadline {
        for _ in 0..64 {
            if !context.pending() {
                break;
            }
            context.iteration(false);
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
#[ignore = "requires a GTK display; run separately on one test thread"]
fn zoom_keeps_visible_page_widgets_after_restore() {
    init_test_gtk();
    let app = Application::builder()
        .application_id("org.omarchy.Lector.ZoomVisibilityTest")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    ui.inner.borrow_mut().session = Session::default();
    ui.set_layout(LayoutMode::Continuous);
    let pdf = crate::doc::write_test_pdf(
        "zoom-visibility",
        &["BT /F1 20 Tf 72 700 Td (Visible page) Tj ET"; 40],
    );
    ui.window.present();
    ui.open_path(&pdf, None);
    ui.set_zoom(Zoom::Actual);
    settle();
    ui.goto_page(25);
    settle();
    assert!(ui.scrolled.height() > 100);
    assert_eq!(ui.inner.borrow().view.page, 25);
    ui.zoom_in_btn.emit_clicked();
    settle();
    let visible = ui.visible_canvas_pages();
    let widgets = ui.page_widgets.borrow();
    for page in visible {
        assert!(
            widgets.iter().any(|(index, _)| *index == page),
            "visible page {page} disappeared after zoom; widgets: {:?}",
            widgets.iter().map(|(i, _)| *i).collect::<Vec<_>>()
        );
    }
    drop(widgets);
    ui.pinch_active.set(true);
    let scale = ui.inner.borrow().view.layout.scale;
    assert_eq!(ui.canvas_scroll(true, -1.0), glib::Propagation::Stop);
    assert_eq!(
        ui.inner.borrow().view.layout.scale,
        scale,
        "Ctrl-scroll must not compete with pinch"
    );
    ui.finish_pinch();
    ui.zoom_in_btn.emit_clicked();
    ui.goto_page(3);
    settle();
    assert_eq!(
        ui.inner.borrow().view.page,
        3,
        "old zoom restoration must not undo newer navigation"
    );
    let target = page_scroll_y(&ui.inner.borrow().view.layout.pages, 3);
    assert!((ui.scrolled.vadjustment().value() - target).abs() < 1.0);
    ui.zoom_in_btn.emit_clicked();
    assert_eq!(ui.canvas_scroll(false, 1.0), glib::Propagation::Proceed);
    let pan_target = ui.scrolled.vadjustment().value() + 64.0;
    ui.scrolled.vadjustment().set_value(pan_target);
    settle();
    assert!(
        (ui.scrolled.vadjustment().value() - pan_target).abs() < 1.0,
        "zoom restore must not override a subsequent pan"
    );
    ui.window.set_visible(false);
    ui.hold.borrow_mut().take();
    std::fs::remove_file(pdf).unwrap();
}

#[test]
#[ignore = "requires a GTK display; run separately on one test thread"]
fn zoom_from_centered_page_preserves_document_focus() {
    init_test_gtk();
    let app = Application::builder()
        .application_id("org.omarchy.Lector.ZoomCenterTest")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    ui.inner.borrow_mut().session = Session::default();
    ui.set_layout(LayoutMode::Single);
    let pdf = crate::doc::write_test_pdf("zoom-center", &["center"]);
    ui.window.present();
    ui.open_path(&pdf, None);
    ui.set_zoom(Zoom::Custom(0.25));
    settle();
    let da = ui.page_widgets.borrow()[0].1.clone();
    let bounds = da.compute_bounds(&ui.scrolled).unwrap();
    let focus = (
        (bounds.x() + bounds.width() * 0.5) as f64,
        (bounds.y() + bounds.height() * 0.5) as f64,
    );
    let anchor = ui.capture_anchor(Some(focus));
    assert!((anchor.rel_x - 0.5).abs() < 0.01, "centered X: {anchor:?}");
    assert!((anchor.rel_y - 0.5).abs() < 0.01, "centered Y: {anchor:?}");
    ui.set_zoom_at(Zoom::Custom(4.0), Some(focus));
    settle();
    let bounds = da.compute_bounds(&ui.scrolled).unwrap();
    assert!(
        ((bounds.x() + bounds.width() * 0.5) as f64 - focus.0).abs() < 2.0,
        "focus={focus:?}, bounds={bounds:?}, hadj={:?}, canvas={:?}, layout={:?}",
        (
            ui.scrolled.hadjustment().value(),
            ui.scrolled.hadjustment().upper(),
            ui.scrolled.hadjustment().page_size()
        ),
        ui.pages_box.compute_bounds(&ui.scrolled),
        ui.inner.borrow().view.layout.pages
    );
    assert!(
        ((bounds.y() + bounds.height() * 0.5) as f64 - focus.1).abs() < 2.0,
        "anchor={anchor:?} focus={focus:?} bounds={bounds:?} canvas={:?} vadj={:?}",
        ui.pages_box.compute_bounds(&ui.scrolled),
        (
            ui.scrolled.vadjustment().value(),
            ui.scrolled.vadjustment().upper(),
            ui.scrolled.height()
        )
    );
    ui.window.set_visible(false);
    ui.hold.borrow_mut().take();
    std::fs::remove_file(pdf).unwrap();
}
