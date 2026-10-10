use super::*;

#[test]
#[ignore = "requires a GTK display; run with --ignored --test-threads=1"]
fn menu_popover_opens_and_maps_on_activation() {
    init_test_gtk();
    let app = Application::builder()
        .application_id("test.lector.menu")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    ui.window.set_default_size(1100, 688);
    ui.window.present();
    let context = glib::MainContext::default();
    // Wait on real time: an idle window has no pending work to iterate on.
    let run_until = |done: &dyn Fn() -> bool| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while !done() && std::time::Instant::now() < deadline {
            while context.pending() {
                context.iteration(false);
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    };
    run_until(&|| ui.window.is_mapped());
    let menu_btn = ui
        .toolbar
        .last_child()
        .and_then(|widget| widget.downcast::<MenuButton>().ok())
        .expect("toolbar must end with the hamburger menu button");
    let popover = menu_btn.popover().expect("hamburger must own a popover");

    menu_btn.set_active(true);
    run_until(&|| popover.is_mapped() && popover.width() > 120);

    assert!(
        popover.is_mapped(),
        "hamburger popover must map after activation"
    );
    assert!(
        popover.width() > 120 && popover.height() > 120,
        "popover must be visible-sized, got {}x{}",
        popover.width(),
        popover.height()
    );

    ui.hold.borrow_mut().take();
    ui.window.destroy();
}
