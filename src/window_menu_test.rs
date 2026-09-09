use super::*;

#[test]
#[ignore = "requires a GTK display; run with --ignored --test-threads=1"]
fn menu_popover_opens_and_maps_on_activation() {
    gtk::init().expect("GTK display required");
    let app = Application::builder()
        .application_id("test.lector.menu")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(gio::Cancellable::NONE).unwrap();
    let ui = Lector::new(&app);
    ui.window.set_default_size(1100, 688);
    ui.window.present();
    let context = glib::MainContext::default();
    for _ in 0..30 {
        context.iteration(false);
    }
    let menu_btn = ui
        .toolbar
        .last_child()
        .and_then(|widget| widget.downcast::<MenuButton>().ok())
        .expect("toolbar must end with the hamburger menu button");
    let popover = menu_btn.popover().expect("hamburger must own a popover");

    menu_btn.set_active(true);
    for _ in 0..30 {
        context.iteration(false);
    }

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
