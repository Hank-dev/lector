mod annotations;
mod doc;
#[cfg(test)]
mod form_tests;
mod persist;
mod session;
mod theme;
mod view;
mod window;

use gtk::prelude::*;
use gtk::{gio, glib, Application};

use window::Lector;

const APP_ID: &str = "org.omarchy.Lector";

fn main() -> glib::ExitCode {
    let app = Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::HANDLES_OPEN | gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(|app| {
        if app.windows().is_empty() {
            let ui = Lector::new(app);
            ui.present();
        }
    });

    app.connect_open(|app, files, _hint| {
        let mut opened = false;
        for file in files {
            let ui = Lector::new(app);
            if let Some(path) = file.path() {
                ui.open_path(&path, None);
            } else {
                ui.open_uri(&file.uri());
            }
            ui.present();
            opened = true;
        }
        if !opened && app.windows().is_empty() {
            let ui = Lector::new(app);
            ui.present();
        }
    });

    app.run()
}
