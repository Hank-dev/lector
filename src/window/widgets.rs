use gtk::prelude::*;
use gtk::Box as GtkBox;
use gtk::{
    gdk, glib, Button, CheckButton, Image, Label, Orientation, Popover, Separator, ToggleButton,
};

use super::{Handle, Lector};
use crate::annotations::DocumentRect;

pub(crate) fn document_rect_contains(rect: DocumentRect, x: f64, y: f64) -> bool {
    x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height
}

pub(crate) fn document_rects_intersect(a: DocumentRect, b: DocumentRect) -> bool {
    a.x <= b.x + b.width && a.x + a.width >= b.x && a.y <= b.y + b.height && a.y + a.height >= b.y
}

pub(crate) fn should_toggle_fit(click_count: i32, has_link: bool, first_was_link: bool) -> bool {
    click_count == 2 && !has_link && !first_was_link
}

pub(crate) fn external_uri_allowed(uri: &str) -> bool {
    if uri.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return false;
    }
    let Some((scheme, _)) = uri.split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    if !chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        || !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        return false;
    }
    matches!(
        scheme.to_ascii_lowercase().as_str(),
        "http" | "https" | "mailto"
    )
}

pub(crate) fn icon_pill(icon: &str, tip: &str) -> Button {
    let b = Button::from_icon_name(icon);
    b.set_tooltip_text(Some(tip));
    b.add_css_class("lector-pill");
    b
}

pub(crate) fn toggle_pill(icon: &str, tip: &str) -> ToggleButton {
    let b = ToggleButton::new();
    b.set_icon_name(icon);
    b.set_tooltip_text(Some(tip));
    b.add_css_class("lector-pill");
    b
}

pub(crate) fn menu_sep() -> Separator {
    let s = Separator::new(Orientation::Horizontal);
    s.add_css_class("lector-menu-sep");
    s
}

pub(crate) fn menu_heading(text: &str) -> Label {
    let l = Label::new(Some(text));
    l.add_css_class("lector-menu-heading");
    l.set_halign(gtk::Align::Start);
    l.set_xalign(0.0);
    l
}

pub(crate) fn shortcut_row(action: &str, keys: &str) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 12);
    row.add_css_class("lector-shortcut-row");
    let name = Label::new(Some(action));
    name.add_css_class("lector-menu-label");
    name.set_halign(gtk::Align::Start);
    name.set_hexpand(true);
    name.set_xalign(0.0);
    let key = Label::new(Some(keys));
    key.add_css_class("lector-menu-key");
    key.set_halign(gtk::Align::End);
    row.append(&name);
    row.append(&key);
    row
}

pub(crate) fn menu_check_row(label: &str, shortcut: &str) -> CheckButton {
    let item = CheckButton::new();
    item.update_property(&[gtk::accessible::Property::Label(label)]);
    item.add_css_class("lector-menu-item");
    item.set_halign(gtk::Align::Fill);
    item.set_hexpand(true);

    let row = GtkBox::new(Orientation::Horizontal, 10);
    let name = Label::new(Some(label));
    name.add_css_class("lector-menu-label");
    name.set_halign(gtk::Align::Start);
    name.set_hexpand(true);
    name.set_xalign(0.0);
    row.append(&name);
    if !shortcut.is_empty() {
        let key = Label::new(Some(shortcut));
        key.add_css_class("lector-menu-key");
        key.set_halign(gtk::Align::End);
        row.append(&key);
    }
    item.set_child(Some(&row));
    item
}

pub(crate) fn bare_menu_row(icon: &str, label: &str, shortcut: &str) -> Button {
    let b = Button::new();
    b.add_css_class("lector-menu-item");
    b.add_css_class("flat");
    b.set_halign(gtk::Align::Fill);
    b.set_hexpand(true);
    b.set_has_frame(false);
    b.set_tooltip_text(Some(label));

    let row = GtkBox::new(Orientation::Horizontal, 10);
    let ico = Image::from_icon_name(icon);
    ico.add_css_class("lector-menu-icon");
    ico.set_pixel_size(16);
    let name = Label::new(Some(label));
    name.add_css_class("lector-menu-label");
    name.set_halign(gtk::Align::Start);
    name.set_hexpand(true);
    name.set_xalign(0.0);
    row.append(&ico);
    row.append(&name);
    if !shortcut.is_empty() {
        let key = Label::new(Some(shortcut));
        key.add_css_class("lector-menu-key");
        key.set_halign(gtk::Align::End);
        row.append(&key);
    }
    b.set_child(Some(&row));
    b
}

pub(crate) fn connect_menu_row(button: &Button, pop: &Popover, h: &Handle, f: fn(&Lector)) {
    let pop = pop.clone();
    let h = h.clone();
    button.connect_clicked(move |_| {
        pop.popdown();
        if let Some(ui) = h.get() {
            f(&ui);
        }
    });
}

pub(crate) fn menu_row(
    icon: &str,
    label: &str,
    shortcut: &str,
    pop: &Popover,
    h: &Handle,
    f: fn(&Lector),
) -> Button {
    let b = bare_menu_row(icon, label, shortcut);
    connect_menu_row(&b, pop, h, f);
    b
}

pub(crate) type MenuItem = (&'static str, &'static str, &'static str, fn(&Lector));

pub(crate) fn append_menu_section(
    col: &GtkBox,
    heading: &str,
    items: &[MenuItem],
    pop: &Popover,
    h: &Handle,
) {
    col.append(&menu_heading(heading));
    for (icon, label, shortcut, f) in items {
        col.append(&menu_row(icon, label, shortcut, pop, h, *f));
    }
}

pub(crate) fn menu_open(ui: &Lector) {
    ui.open_dialog();
}
pub(crate) fn menu_save(ui: &Lector) {
    ui.save_copy();
}
pub(crate) fn menu_print(ui: &Lector) {
    ui.print_doc();
}
pub(crate) fn menu_sidebar(ui: &Lector) {
    ui.toggle_sidebar();
}
pub(crate) fn menu_rot_l(ui: &Lector) {
    ui.rotate(-90);
}
pub(crate) fn menu_rot_r(ui: &Lector) {
    ui.rotate(90);
}
pub(crate) fn menu_fs(ui: &Lector) {
    ui.toggle_fullscreen();
}
pub(crate) fn menu_pres(ui: &Lector) {
    ui.toggle_presentation();
}
pub(crate) fn menu_reload(ui: &Lector) {
    ui.reload();
}
pub(crate) fn menu_highlight(ui: &Lector) {
    ui.add_highlight();
}
pub(crate) fn menu_note(ui: &Lector) {
    ui.add_note();
}
pub(crate) fn menu_delete_annotation(ui: &Lector) {
    ui.delete_targeted_annotation();
}
pub(crate) fn menu_history(ui: &Lector) {
    ui.show_reading_history();
}
pub(crate) fn menu_props(ui: &Lector) {
    ui.show_properties();
}
pub(crate) fn menu_keys(ui: &Lector) {
    ui.show_shortcuts();
}
pub(crate) fn menu_about(ui: &Lector) {
    ui.show_about();
}

pub(crate) fn surface_texture(surf: &cairo::ImageSurface) -> Option<gdk::MemoryTexture> {
    surf.flush();
    let w = surf.width();
    let h = surf.height();
    let stride = surf.stride() as usize;
    let mut bytes = None;
    surf.with_data(|data| bytes = Some(glib::Bytes::from(data)))
        .ok()?;
    let bytes = bytes?;
    Some(gdk::MemoryTexture::new(
        w,
        h,
        gdk::MemoryFormat::B8g8r8a8Premultiplied,
        &bytes,
        stride,
    ))
}
