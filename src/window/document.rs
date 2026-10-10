use super::*;

impl Lector {
    pub fn present(&self) {
        self.window.present();
        if let Some(error) = self.inner.borrow().annotation_error.clone() {
            self.alert(
                "Could not load annotations",
                &format!("{error}\n\nThe annotation file will not be modified."),
            );
        }
        if let Some(error) = self.inner.borrow().session_error.clone() {
            self.alert(
                "Could not load reading history",
                &format!(
                    "{error}\n\nThe session file will not be modified. Fix or remove it to save \
                     reading positions again."
                ),
            );
        }
        self.pages_box.grab_focus();
        let h = Handle(self.me.borrow().clone());
        glib::timeout_add_local_once(std::time::Duration::from_millis(50), move || {
            if let Some(ui) = h.get() {
                let w = ui.scrolled.width() as f64;
                let ht = ui.scrolled.height() as f64;
                ui.on_resize(w.max(1.0), ht.max(1.0));
            }
        });
    }

    pub fn open_uri(&self, uri: &str) {
        match OpenDoc::open_uri(uri, None) {
            Ok(doc) => self.adopt_doc(doc),
            Err(OpenError::Encrypted) => {
                if let Ok(path) = glib::filename_from_uri(uri) {
                    self.ask_password(path.0);
                } else {
                    self.alert(
                        "Could not open document",
                        "This file is encrypted and has no local path to unlock.",
                    );
                }
            }
            Err(OpenError::Other(msg)) => self.alert("Could not open document", &msg),
        }
    }

    pub fn open_path(&self, path: &Path, password: Option<&str>) {
        match OpenDoc::open(path, password) {
            Ok(doc) => self.adopt_doc(doc),
            Err(OpenError::Encrypted) => self.ask_password(path.to_path_buf()),
            Err(OpenError::Other(msg)) => self.alert("Could not open document", &msg),
        }
    }

    pub(crate) fn adopt_doc(&self, doc: OpenDoc) {
        // Checked before discarding edits: a refused document keeps them.
        if doc.n_pages <= 0 {
            self.alert("Could not open document", "The file has no pages.");
            return;
        }
        // Typed but uncommitted text counts as an unsaved change.
        self.commit_text_draft();
        if self.pdf_dirty.get() {
            self.confirm_discard(move |ui| ui.adopt_doc(doc));
            return;
        }
        self.cancel_text_placement();
        self.cancel_draw();
        self.reset_edit_history();
        let opened_path = doc.path.clone();
        let restore = {
            let mut st = self.inner.borrow_mut();
            if let Some(old) = st.view.doc.as_ref() {
                let previous = (
                    old.path.clone(),
                    st.view.page,
                    st.view.zoom,
                    st.view.rotation,
                );
                remember_document_state(
                    &mut st.session,
                    &previous.0,
                    previous.1,
                    previous.2,
                    previous.3,
                );
            }
            st.search.invalidate();
            st.session.restore(&doc.path)
        };
        {
            let mut st = self.inner.borrow_mut();
            if let Some(r) = restore {
                st.view.page = r.page.clamp(0, (doc.n_pages - 1).max(0));
                st.view.zoom = session::parse_zoom(&r.zoom, r.custom);
                st.view.rotation = r.rotation;
            } else {
                st.view.page = 0;
                st.view.zoom = Zoom::FitWidth;
                st.view.rotation = 0;
            }
            st.view.selection = None;
            st.pane.targeted_annotation = None;
            st.pane.outline_collapsed = initially_collapsed_outline(&doc.outline);
            // Raster keys have no document identity, including same-path reloads.
            st.raster.clear();
            st.raster_gen = st.raster_gen.wrapping_add(1);
            self.raster_pending.borrow_mut().clear();
            // Keep busy owned by the queued callback until it observes its stale generation.
            st.view.doc = Some(doc);
            st.navigation.clear();
            let page = st.view.page;
            let zoom = st.view.zoom;
            let rotation = st.view.rotation;
            remember_document_state(&mut st.session, &opened_path, page, zoom, rotation);
        }
        self.ignore_search_changed.set(true);
        self.search_entry.set_text("");
        self.ignore_search_changed.set(false);
        self.search_info.set_text("");
        self.clear_search_results();
        let page = self.inner.borrow().view.page;
        self.sync_zoom_dropdown();
        self.rebuild_sidebar();
        self.sync_sidebar();
        self.reflow();
        self.goto_page(page);
        self.update_chrome();
        self.queue_pages_draw();
        self.watch_open_document();
    }

    pub(crate) fn ask_password(&self, path: PathBuf) {
        let pop = gtk::Window::builder()
            .transient_for(&self.window)
            .modal(true)
            .title("Enter password")
            .default_width(360)
            .build();
        let col = GtkBox::new(Orientation::Vertical, 10);
        col.set_margin_top(16);
        col.set_margin_bottom(16);
        col.set_margin_start(16);
        col.set_margin_end(16);
        col.append(&Label::new(Some("This document is encrypted.")));
        let entry = PasswordEntry::new();
        entry.set_show_peek_icon(true);
        col.append(&entry);
        let row = GtkBox::new(Orientation::Horizontal, 8);
        row.set_halign(gtk::Align::End);
        let cancel = Button::with_label("Cancel");
        let ok = Button::with_label("Unlock");
        row.append(&cancel);
        row.append(&ok);
        col.append(&row);
        pop.set_child(Some(&col));
        cancel.connect_clicked({
            let pop = pop.clone();
            move |_| pop.close()
        });
        let h = Handle(self.me.borrow().clone());
        let unlock = {
            let pop = pop.clone();
            let entry = entry.clone();
            let path = path.clone();
            let h = h.clone();
            move || {
                let pw = entry.text().to_string();
                pop.close();
                if let Some(ui) = h.get() {
                    ui.open_path(&path, Some(&pw));
                }
            }
        };
        ok.connect_clicked({
            let unlock = unlock.clone();
            move |_| unlock()
        });
        entry.connect_activate(move |_| unlock());
        pop.present();
        entry.grab_focus();
    }

    pub(crate) fn open_dialog(&self) {
        let dialog = gtk::FileDialog::builder().title("Open Document").build();
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("PDF Documents"));
        filter.add_mime_type("application/pdf");
        filter.add_suffix("pdf");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        dialog.set_filters(Some(&filters));
        let window = self.window.clone();
        let h = Handle(self.me.borrow().clone());
        dialog.open(Some(&window), gio::Cancellable::NONE, move |res| {
            if let Ok(file) = res {
                if let Some(path) = file.path() {
                    if let Some(ui) = h.get() {
                        ui.open_path(&path, None);
                    }
                }
            }
        });
    }

    pub(crate) fn hide_file_banner(&self) {
        self.file_change_gen
            .set(self.file_change_gen.get().wrapping_add(1));
        self.pending_disk_change.set(None);
        self.file_banner.set_reveal_child(false);
    }

    pub(crate) fn show_file_banner(&self, change: DiskChange) {
        match change {
            DiskChange::Modified => {
                self.file_banner_label
                    .set_text("This file has changed on disk.");
                self.file_reload_btn.set_sensitive(true);
                self.file_reload_btn.set_visible(true);
            }
            DiskChange::Removed => {
                self.file_banner_label
                    .set_text("This file was removed from disk.");
                self.file_reload_btn.set_sensitive(false);
                self.file_reload_btn.set_visible(false);
            }
        }
        self.file_banner.set_reveal_child(true);
    }

    pub(crate) fn note_disk_change(&self, change: DiskChange) {
        if self
            .file_watch_ignore_until
            .get()
            .is_some_and(|until| Instant::now() < until)
        {
            return;
        }
        let pending = match (self.pending_disk_change.get(), change) {
            (Some(DiskChange::Removed), _) | (_, DiskChange::Removed) => DiskChange::Removed,
            _ => DiskChange::Modified,
        };
        self.pending_disk_change.set(Some(pending));
        let gen = self.file_change_gen.get().wrapping_add(1);
        self.file_change_gen.set(gen);
        let h = Handle(self.me.borrow().clone());
        glib::timeout_add_local(Duration::from_millis(FILE_CHANGE_MS), move || {
            let Some(ui) = h.get() else {
                return glib::ControlFlow::Break;
            };
            if ui.file_change_gen.get() != gen {
                return glib::ControlFlow::Break;
            }
            if let Some(pending) = ui.pending_disk_change.get() {
                let exists = ui
                    .inner
                    .borrow()
                    .view
                    .doc
                    .as_ref()
                    .is_some_and(|doc| doc.path.is_file());
                if let Some(change) = resolved_disk_change(Some(pending), exists) {
                    ui.show_file_banner(change);
                    if matches!(pending, DiskChange::Removed) {
                        ui.attach_document_monitor(false);
                    }
                }
            }
            glib::ControlFlow::Break
        });
    }

    pub(crate) fn watch_open_document(&self) {
        self.hide_file_banner();
        self.attach_document_monitor(true);
    }

    pub(crate) fn attach_document_monitor(&self, apply_grace: bool) {
        let path = self
            .inner
            .borrow()
            .view
            .doc
            .as_ref()
            .map(|doc| doc.path.clone());
        let Some(path) = path else {
            *self.doc_monitor.borrow_mut() = None;
            return;
        };
        let file = gio::File::for_path(&path);
        let monitor = file
            .monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
            .ok();
        if let Some(monitor) = monitor.as_ref() {
            let h = Handle(self.me.borrow().clone());
            monitor.connect_changed(move |_, _, _, event| {
                if let (Some(ui), Some(change)) = (h.get(), classify_document_event(event)) {
                    ui.note_disk_change(change);
                }
            });
        }
        *self.doc_monitor.borrow_mut() = monitor;
        if apply_grace {
            self.file_watch_ignore_until.set(Some(
                Instant::now() + Duration::from_millis(FILE_WATCH_GRACE_MS),
            ));
        }
    }

    pub(crate) fn link_at_view(&self, x: f64, y: f64) -> Option<DocumentLink> {
        let (page, display_x, display_y) = self.hit_at(x, y)?;
        let st = self.inner.borrow();
        let doc = st.view.doc.as_ref()?;
        let (doc_x, doc_y) =
            display_point_to_document(doc, page, st.view.rotation, display_x, display_y);
        doc.link_at(page, doc_x, doc_y)
    }

    pub(crate) fn activate_link_at(&self, x: f64, y: f64) -> bool {
        if let Some(link) = self.link_at_view(x, y) {
            self.follow_link(link);
            true
        } else {
            false
        }
    }

    pub(crate) fn follow_link(&self, link: DocumentLink) {
        match link {
            DocumentLink::GotoPage(page) => self.navigate_to_page(page),
            DocumentLink::Uri(uri) => {
                if !external_uri_allowed(&uri) {
                    self.alert(
                        "Link blocked",
                        "Only web (HTTP/HTTPS) and email links can be opened.",
                    );
                    return;
                }
                if let Err(error) =
                    gio::AppInfo::launch_default_for_uri(&uri, None::<&gio::AppLaunchContext>)
                {
                    self.alert("Could not open link", &error.to_string());
                }
            }
        }
    }

    /// What a print job draws. A text draft is committed first: while an
    /// answer is being edited it is out of the PDF.
    pub(crate) fn print_source(&self) -> Option<(poppler::Document, i32, i32, Vec<Annotation>)> {
        self.commit_text_draft();
        let st = self.inner.borrow();
        let doc = st.view.doc.as_ref()?;
        let marks = st
            .annotations
            .as_ref()
            .map(|store| store.for_document(&doc.path).to_vec())
            .unwrap_or_default();
        Some((doc.document.clone(), doc.n_pages, st.view.rotation, marks))
    }

    pub(crate) fn print_doc(&self) {
        let Some((document, n, rotation, marks)) = self.print_source() else {
            return;
        };
        let op = gtk::PrintOperation::new();
        op.set_n_pages(n);
        op.connect_draw_page(move |_, ctx, nr| {
            let Some(page) = document.page(nr) else {
                return;
            };
            let cr = ctx.cairo_context();
            let (pw, ph) = page.size();
            let (dw, dh) = print_content_size(pw, ph, rotation);
            let scale = (ctx.width() / dw.max(1.0))
                .min(ctx.height() / dh.max(1.0))
                .max(0.01);
            cr.save().ok();
            apply_print_page_transform(&cr, pw, ph, rotation, ctx.width(), ctx.height());
            page.render_for_printing(&cr);
            cr.restore().ok();
            draw_annotations(
                &cr,
                AnnotationDrawing {
                    annotations: &marks,
                    page: nr,
                    page_width: pw,
                    page_height: ph,
                    rotation,
                    scale,
                    targeted: None,
                },
            );
        });
        match op.run(gtk::PrintOperationAction::PrintDialog, Some(&self.window)) {
            Ok(gtk::PrintOperationResult::Error) => {
                self.alert("Print failed", "The print operation reported an error.");
            }
            Err(e) => self.alert("Print failed", &e.to_string()),
            Ok(_) => {}
        }
    }

    pub(crate) fn save_copy(&self) {
        self.commit_text_draft();
        let st = self.inner.borrow();
        let Some(doc) = st.view.doc.as_ref() else {
            return;
        };
        let document = doc.document.clone();
        let suggest = copy_file_name(&doc.path);
        drop(st);
        let dialog = gtk::FileDialog::builder().title("Save a Copy").build();
        if let Some(name) = suggest {
            dialog.set_initial_name(Some(&name));
        }
        let h = Handle(self.me.borrow().clone());
        dialog.save(
            Some(&self.window),
            gio::Cancellable::NONE,
            move |res| match res {
                Ok(file) => {
                    if let Some(path) = file.path() {
                        if let Some(ui) = h.get() {
                            let same_document = ui
                                .inner
                                .borrow()
                                .view
                                .doc
                                .as_ref()
                                .is_some_and(|doc| doc.document == document);
                            if !same_document {
                                return;
                            }
                            if let Err(error) = ui.save_copy_to(&path) {
                                ui.alert("Could not save a copy", &error);
                            }
                        }
                    }
                }
                Err(e) => {
                    if !e.matches(gtk::DialogError::Cancelled)
                        && !e.matches(gtk::DialogError::Dismissed)
                    {
                        if let Some(ui) = h.get() {
                            ui.alert("Could not save a copy", &e.to_string());
                        }
                    }
                }
            },
        );
    }

    pub(crate) fn show_properties(&self) {
        if let Some(dialog) = self.properties_dialog() {
            dialog.present();
        }
    }

    pub(crate) fn show_reading_history(&self) {
        self.reading_history_dialog().present();
    }

    pub(crate) fn show_shortcuts(&self) {
        self.shortcuts_dialog().present();
    }

    pub(crate) fn show_about(&self) {
        self.about_dialog().present();
    }

    pub(crate) fn themed_dialog(&self, title: &str, body: &impl IsA<gtk::Widget>) -> gtk::Window {
        let win = gtk::Window::builder()
            .transient_for(&self.window)
            .modal(true)
            .title(title)
            .resizable(false)
            .default_width(460)
            .build();
        win.add_css_class("lector-window");
        win.add_css_class("lector-dialog");
        win.set_titlebar(None::<&gtk::Box>);

        let title_label = Label::new(Some(title));
        title_label.add_css_class("lector-dialog-title");
        title_label.set_hexpand(true);
        title_label.set_halign(gtk::Align::Start);
        title_label.set_xalign(0.0);

        let close = icon_pill("window-close-symbolic", "Close");
        let header = GtkBox::new(Orientation::Horizontal, 2);
        header.add_css_class("lector-toolbar");
        header.append(&title_label);
        header.append(&close);

        let body_wrap = GtkBox::new(Orientation::Vertical, 0);
        body_wrap.add_css_class("lector-dialog-body");
        body_wrap.append(body);

        let root = GtkBox::new(Orientation::Vertical, 0);
        root.append(&header);
        root.append(&body_wrap);
        win.set_child(Some(&root));

        let win_close = win.clone();
        close.connect_clicked(move |_| {
            win_close.close();
        });
        let keys = gtk::EventControllerKey::new();
        let win_esc = win.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == Key::Escape {
                win_esc.close();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        win.add_controller(keys);
        win
    }

    pub(crate) fn shortcuts_dialog(&self) -> gtk::Window {
        let left = GtkBox::new(Orientation::Vertical, 2);
        let right = GtkBox::new(Orientation::Vertical, 2);
        left.set_hexpand(true);
        right.set_hexpand(true);
        for section in shortcut_sections() {
            let col = match section.heading {
                "VIEW" | "NAVIGATION" => &right,
                _ => &left,
            };
            col.append(&menu_heading(section.heading));
            for row in section.rows {
                col.append(&shortcut_row(row.action, row.keys));
            }
        }
        let columns = GtkBox::new(Orientation::Horizontal, 28);
        columns.append(&left);
        columns.append(&right);
        let scrolled = ScrolledWindow::new();
        scrolled.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scrolled.set_max_content_height(520);
        scrolled.set_propagate_natural_height(true);
        scrolled.set_child(Some(&columns));
        self.themed_dialog("Keyboard shortcuts", &scrolled)
    }

    pub(crate) fn reading_history_dialog(&self) -> gtk::Window {
        let entries = self.inner.borrow().session.history(20);
        let list = ListBox::new();
        list.add_css_class("lector-history");
        list.set_selection_mode(gtk::SelectionMode::Single);
        if entries.is_empty() {
            let empty = Label::new(Some("No reading history yet"));
            empty.add_css_class("dim-label");
            empty.set_margin_top(24);
            empty.set_margin_bottom(24);
            list.append(&empty);
        } else {
            for entry in &entries {
                let row = gtk::ListBoxRow::new();
                let text = GtkBox::new(Orientation::Vertical, 2);
                text.set_hexpand(true);
                let name = Label::new(Some(&entry.basename));
                name.add_css_class("lector-menu-label");
                name.set_halign(gtk::Align::Start);
                name.set_xalign(0.0);
                let path = Label::new(Some(&entry.path.display().to_string()));
                path.add_css_class("lector-history-path");
                path.set_halign(gtk::Align::Start);
                path.set_xalign(0.0);
                path.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
                text.append(&name);
                text.append(&path);
                let status_text = if entry.exists {
                    format!("Page {}", entry.page)
                } else {
                    "Missing".into()
                };
                let status = Label::new(Some(&status_text));
                status.add_css_class(if entry.exists {
                    "lector-history-page"
                } else {
                    "lector-history-missing"
                });
                let content = GtkBox::new(Orientation::Horizontal, 12);
                content.set_margin_top(7);
                content.set_margin_bottom(7);
                content.set_margin_start(4);
                content.set_margin_end(4);
                content.append(&text);
                content.append(&status);
                row.set_child(Some(&content));
                row.set_sensitive(entry.exists);
                list.append(&row);
            }
        }

        let clear = Button::with_label("Clear History…");
        clear.set_tooltip_text(Some("Requires confirmation; annotations are kept"));
        clear.set_sensitive(!entries.is_empty());
        clear.set_halign(gtk::Align::End);
        let body = GtkBox::new(Orientation::Vertical, 10);
        let scrolled = ScrolledWindow::new();
        scrolled.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scrolled.set_min_content_height(160);
        scrolled.set_max_content_height(520);
        scrolled.set_propagate_natural_height(true);
        scrolled.set_child(Some(&list));
        body.append(&scrolled);
        body.append(&clear);
        let dialog = self.themed_dialog("Reading History", &body);
        dialog.set_default_width(620);

        let paths = Rc::new(
            entries
                .iter()
                .map(|entry| entry.path.clone())
                .collect::<Vec<_>>(),
        );
        list.connect_row_activated({
            let paths = paths.clone();
            let dialog = dialog.clone();
            let h = Handle(self.me.borrow().clone());
            move |_, row| {
                let Some(path) = paths.get(row.index() as usize) else {
                    return;
                };
                dialog.close();
                if let Some(ui) = h.get() {
                    ui.open_path(path, None);
                }
            }
        });
        clear.connect_clicked({
            let dialog = dialog.clone();
            let h = Handle(self.me.borrow().clone());
            move |_| {
                let confirmation = gtk::AlertDialog::builder()
                    .modal(true)
                    .message("Clear reading history?")
                    .detail("This removes remembered documents and page positions. Annotations are not deleted.")
                    .buttons(["Cancel", "Clear History"])
                    .cancel_button(0)
                    .default_button(0)
                    .build();
                let dialog_after = dialog.clone();
                let h_after = h.clone();
                confirmation.choose(Some(&dialog), gio::Cancellable::NONE, move |result| {
                    if result != Ok(1) {
                        return;
                    }
                    let Some(ui) = h_after.get() else { return };
                    let result = {
                        let st = ui.inner.borrow();
                        let mut candidate = st.session.clone();
                        candidate.clear_history_for_open_document(
                            st.view.doc.as_ref().map(|document| document.path.as_path()),
                        );
                        candidate.save().map(|_| candidate)
                    };
                    match result {
                        Ok(session) => {
                            ui.inner.borrow_mut().session = session;
                            dialog_after.close();
                        }
                        Err(error) => ui.alert("Could not clear history", &error),
                    }
                });
            }
        });
        dialog
    }

    pub(crate) fn about_dialog(&self) -> gtk::Window {
        let about = about_copy();
        let col = GtkBox::new(Orientation::Vertical, 6);
        col.set_halign(gtk::Align::Center);
        col.set_valign(gtk::Align::Center);
        let icon = Image::from_icon_name("org.omarchy.Lector");
        icon.set_pixel_size(64);
        let name = Label::new(Some(about.name));
        name.add_css_class("lector-about-name");
        let tagline = Label::new(Some(about.tagline));
        tagline.add_css_class("lector-about-copy");
        let detail = Label::new(Some(about.detail));
        detail.add_css_class("lector-about-copy");
        let version = Label::new(Some(about.version));
        version.add_css_class("lector-about-version");
        col.append(&icon);
        col.append(&name);
        col.append(&tagline);
        col.append(&detail);
        col.append(&version);
        self.themed_dialog("About Lector", &col)
    }

    pub(crate) fn properties_dialog(&self) -> Option<gtk::Window> {
        let st = self.inner.borrow();
        let doc = st.view.doc.as_ref()?;
        let fields = property_fields(doc);
        drop(st);
        let list = GtkBox::new(Orientation::Vertical, 8);
        for (key, value) in fields {
            let row = GtkBox::new(Orientation::Horizontal, 16);
            let label = Label::new(Some(key));
            label.add_css_class("lector-field-label");
            label.set_xalign(0.0);
            label.set_width_chars(9);
            label.set_halign(gtk::Align::Start);
            let val = Label::new(Some(&value));
            val.add_css_class("lector-field-value");
            val.set_hexpand(true);
            val.set_xalign(0.0);
            val.set_halign(gtk::Align::Start);
            val.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            val.set_selectable(true);
            row.append(&label);
            row.append(&val);
            list.append(&row);
        }
        Some(self.themed_dialog("Properties", &list))
    }

    pub(crate) fn reload(&self) {
        let (path, password) = {
            let st = self.inner.borrow();
            match st.view.doc.as_ref() {
                Some(d) => (Some(d.path.clone()), d.password.clone()),
                None => (None, None),
            }
        };
        if let Some(p) = path {
            self.open_path(&p, password.as_deref());
        }
    }

    pub(crate) fn persist(&self) {
        self.record_view_in_session();
        let result = self.inner.borrow_mut().session.save();
        if let Err(error) = result {
            self.alert("Could not save session", &error);
        }
    }

    /// Copies the view into the session. During a presentation that is the
    /// view it returns to, not its temporary single-page, fit-page layout.
    pub(crate) fn record_view_in_session(&self) {
        let mut st = self.inner.borrow_mut();
        let st = &mut *st;
        let (layout, zoom, sidebar) = st.pres_restore.filter(|_| st.presentation).unwrap_or((
            st.view.layout_mode,
            st.view.zoom,
            st.pane.sidebar,
        ));
        if let Some(doc) = st.view.doc.as_ref() {
            remember_document_state(
                &mut st.session,
                &doc.path,
                st.view.page,
                zoom,
                st.view.rotation,
            );
        }
        st.session.invert = st.view.pdf_mode == PdfMode::Inverted;
        st.session.pdf_mode = st.view.pdf_mode.key().into();
        st.session.layout = match layout {
            LayoutMode::Single => "single".into(),
            LayoutMode::Dual => "dual".into(),
            LayoutMode::Continuous => "continuous".into(),
        };
        st.session.dual_cover = st.view.dual_cover;
        st.session.sidebar = match sidebar {
            Sidebar::Hidden => "hidden".into(),
            Sidebar::Thumbs => "thumbs".into(),
            Sidebar::Outline => "outline".into(),
            Sidebar::Marks => "marks".into(),
        };
    }

    pub(crate) fn reload_theme(&self) {
        let theme = Theme::load();
        theme::apply_css(&self.css, &theme);
        {
            let mut st = self.inner.borrow_mut();
            st.theme = theme;
            st.raster.clear();
        }
        self.rebuild_sidebar();
        self.queue_pages_draw();
        self.ensure_visible_rasters();
        self.update_chrome();
    }
}
