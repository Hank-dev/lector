use super::*;

impl Lector {
    pub(crate) fn current_location(&self) -> ViewLocation {
        ViewLocation::new(
            self.inner.borrow().view.page,
            self.scrolled.hadjustment().value(),
            self.scrolled.vadjustment().value(),
        )
    }

    pub(crate) fn navigate_to_page(&self, page: i32) {
        let current = self.current_location();
        let (target_page, target_y) = {
            let st = self.inner.borrow();
            let n = st.view.doc.as_ref().map(|doc| doc.n_pages).unwrap_or(1);
            let target_page = page.clamp(0, (n - 1).max(0));
            (
                target_page,
                page_scroll_y(&st.view.layout.pages, target_page),
            )
        };
        if target_page == current.page {
            self.goto_page(target_page);
            return;
        }
        let target = ViewLocation::new(target_page, 0.0, target_y);
        self.inner.borrow_mut().navigation.visit(current, target);
        self.goto_page(target_page);
        self.update_navigation_buttons();
    }

    pub(crate) fn restore_location(&self, location: ViewLocation) {
        {
            let mut st = self.inner.borrow_mut();
            let n = st.view.doc.as_ref().map(|doc| doc.n_pages).unwrap_or(1);
            st.view.page = location.page.clamp(0, (n - 1).max(0));
            let remember = st.view.doc.as_ref().map(|doc| {
                (
                    doc.path.clone(),
                    st.view.page,
                    st.view.zoom,
                    st.view.rotation,
                )
            });
            if let Some((path, page, zoom, rotation)) = remember {
                remember_document_state(&mut st.session, &path, page, zoom, rotation);
            }
        }
        self.reflow_restore(ViewRestore::Location(location));
        self.update_navigation_buttons();
    }

    pub(crate) fn navigate_back(&self) {
        let current = self.current_location();
        let target = self.inner.borrow_mut().navigation.back(current);
        if let Some(location) = target {
            self.restore_location(location);
        }
    }

    pub(crate) fn navigate_forward(&self) {
        let current = self.current_location();
        let target = self.inner.borrow_mut().navigation.forward(current);
        if let Some(location) = target {
            self.restore_location(location);
        }
    }

    pub(crate) fn update_navigation_buttons(&self) {
        let st = self.inner.borrow();
        self.back_btn.set_sensitive(!st.navigation.back.is_empty());
        self.forward_btn
            .set_sensitive(!st.navigation.forward.is_empty());
    }

    pub(crate) fn goto_page(&self, page: i32) {
        let mode = {
            let mut st = self.inner.borrow_mut();
            let n = st.view.doc.as_ref().map(|d| d.n_pages).unwrap_or(1);
            st.view.page = page.clamp(0, (n - 1).max(0));
            let remember = st.view.doc.as_ref().map(|doc| {
                (
                    doc.path.clone(),
                    st.view.page,
                    st.view.zoom,
                    st.view.rotation,
                )
            });
            if let Some((path, page, zoom, rotation)) = remember {
                remember_document_state(&mut st.session, &path, page, zoom, rotation);
            }
            st.view.layout_mode
        };
        if mode != LayoutMode::Continuous {
            self.reflow_restore(ViewRestore::CurrentPage);
            return;
        }
        self.update_chrome();
        self.inner.borrow_mut().ignore_scroll = true;
        self.scroll_to_current();
        self.inner.borrow_mut().ignore_scroll = false;
        self.sync_visible_page_widgets();
        self.ensure_visible_rasters();
    }

    pub(crate) fn goto_page_rel(&self, delta: i32) {
        let page = self.inner.borrow().view.page + delta;
        self.goto_page(page);
    }

    pub(crate) fn set_zoom(&self, zoom: Zoom) {
        self.set_zoom_at(zoom, None);
    }

    pub(crate) fn set_zoom_at(&self, zoom: Zoom, focus: Option<(f64, f64)>) {
        let anchor = self.capture_anchor(focus);
        self.set_zoom_restore(zoom, ViewRestore::Anchor(anchor));
    }

    pub(crate) fn set_zoom_restore(&self, zoom: Zoom, restore: ViewRestore) {
        self.inner.borrow_mut().view.zoom = zoom;
        self.sync_zoom_dropdown();
        self.reflow_restore(restore);
    }

    pub(crate) fn zoom_by(&self, factor: f64) {
        self.zoom_by_at(factor, None);
    }

    pub(crate) fn zoom_by_at(&self, factor: f64, focus: Option<(f64, f64)>) {
        let current = self.inner.borrow().view.layout.scale;
        self.set_zoom_at(zoom_from_factor(current, factor), focus);
    }

    pub(crate) fn zoom_from_dropdown(&self, idx: u32) {
        if self.inner.borrow().ignore_zoom_dropdown {
            return;
        }
        let zoom = match idx {
            0 | 1 => Zoom::FitPage,
            2 => Zoom::FitWidth,
            3 => Zoom::FitHeight,
            4 => Zoom::Custom(0.50),
            5 => Zoom::Custom(0.70),
            6 => Zoom::Custom(0.85),
            7 => Zoom::Actual,
            8 => Zoom::Custom(1.25),
            9 => Zoom::Custom(1.50),
            10 => Zoom::Custom(1.75),
            11 => Zoom::Custom(2.00),
            12 => Zoom::Custom(3.00),
            13 => Zoom::Custom(4.00),
            14 => Zoom::Custom(8.00),
            15 => Zoom::Custom(16.0),
            _ => return,
        };
        self.inner.borrow_mut().view.zoom = zoom;
        self.reflow();
    }

    pub(crate) fn sync_zoom_dropdown(&self) {
        let chrome = zoom_chrome(self.inner.borrow().view.zoom);
        self.inner.borrow_mut().ignore_zoom_dropdown = true;
        match chrome {
            ZoomChrome::Preset(idx) => {
                self.zoom_drop.set_selected(idx);
                self.remove_custom_zoom_item();
            }
            ZoomChrome::Percent(percent) => {
                let idx = self.set_custom_zoom_item(&format!("{percent}%"));
                self.zoom_drop.set_selected(idx);
            }
        }
        self.inner.borrow_mut().ignore_zoom_dropdown = false;
    }

    pub(crate) fn zoom_string_list(&self) -> Option<gtk::StringList> {
        self.zoom_drop.model().and_downcast::<gtk::StringList>()
    }

    pub(crate) fn set_custom_zoom_item(&self, label: &str) -> u32 {
        let Some(list) = self.zoom_string_list() else {
            return ZOOM_LABELS.len() as u32;
        };
        let slot = ZOOM_LABELS.len() as u32;
        if list.n_items() <= slot {
            list.append(label);
        } else {
            list.splice(slot, 1, &[label]);
        }
        slot
    }

    pub(crate) fn remove_custom_zoom_item(&self) {
        let Some(list) = self.zoom_string_list() else {
            return;
        };
        let slot = ZOOM_LABELS.len() as u32;
        if list.n_items() > slot {
            list.remove(slot);
        }
    }

    pub(crate) fn toggle_fit(&self) {
        let z = self.inner.borrow().view.zoom;
        let next = match z {
            Zoom::FitWidth => Zoom::FitPage,
            Zoom::FitPage => Zoom::Actual,
            _ => Zoom::FitWidth,
        };
        self.set_zoom(next);
    }

    pub(crate) fn set_layout(&self, mode: LayoutMode) {
        let page = {
            let mut st = self.inner.borrow_mut();
            st.view.layout_mode = mode;
            st.ignore_menu_checks = true;
            st.view.page
        };
        self.cont_item.set_active(mode == LayoutMode::Continuous);
        self.dual_item.set_active(mode == LayoutMode::Dual);
        self.cover_item.set_sensitive(mode == LayoutMode::Dual);
        self.single_item.set_active(mode == LayoutMode::Single);
        self.inner.borrow_mut().ignore_menu_checks = false;
        self.reflow();
        self.goto_page(page);
    }

    pub(crate) fn set_dual_cover(&self, cover: bool) {
        let page = {
            let mut st = self.inner.borrow_mut();
            st.view.dual_cover = cover;
            st.ignore_menu_checks = true;
            st.view.page
        };
        self.cover_item.set_active(cover);
        self.inner.borrow_mut().ignore_menu_checks = false;
        self.reflow();
        self.goto_page(page);
    }

    pub(crate) fn set_pdf_mode(&self, mode: PdfMode) {
        {
            let mut st = self.inner.borrow_mut();
            st.view.pdf_mode = mode;
            st.ignore_menu_checks = true;
        }
        self.normal_item.set_active(mode == PdfMode::Normal);
        self.omarchy_item.set_active(mode == PdfMode::Omarchy);
        self.invert_item.set_active(mode == PdfMode::Inverted);
        self.inner.borrow_mut().ignore_menu_checks = false;
        self.inner.borrow_mut().raster.clear();
        self.rebuild_sidebar();
        self.queue_pages_draw();
        self.ensure_visible_rasters();
    }

    pub(crate) fn rotate(&self, deg: i32) {
        let mut st = self.inner.borrow_mut();
        st.view.rotate_by(deg);
        st.raster_gen = st.raster_gen.wrapping_add(1);
        self.raster_pending.borrow_mut().clear();
        drop(st);
        self.reflow();
    }

    pub(crate) fn toggle_fullscreen(&self) {
        if self.window.is_fullscreen() {
            let ended_presentation = {
                let mut st = self.inner.borrow_mut();
                let mut restore = st.pres_restore.take();
                let inner = &mut *st;
                let ended = restore_presentation_state(
                    &mut inner.presentation,
                    &mut restore,
                    &mut inner.view.layout_mode,
                    &mut inner.view.zoom,
                    &mut inner.pane.sidebar,
                );
                inner.pres_restore = restore;
                ended
            };
            self.window.unfullscreen();
            self.toolbar.set_visible(true);
            if ended_presentation {
                self.window.remove_css_class("lector-presentation");
                self.sync_sidebar();
                let layout = self.inner.borrow().view.layout_mode;
                self.set_layout(layout);
                self.sync_zoom_dropdown();
            }
        } else {
            self.window.fullscreen();
        }
    }

    pub(crate) fn toggle_presentation(&self) {
        let going = !self.inner.borrow().presentation;
        if going {
            let restore = {
                let st = self.inner.borrow();
                (st.view.layout_mode, st.view.zoom, st.pane.sidebar)
            };
            {
                let mut st = self.inner.borrow_mut();
                st.presentation = true;
                st.pres_restore = Some(restore);
                st.view.layout_mode = LayoutMode::Single;
                st.view.zoom = Zoom::FitPage;
            }
            self.find_bar.set_reveal_child(false);
            self.sidebar_revealer.set_reveal_child(false);
            self.toolbar.set_visible(false);
            self.window.add_css_class("lector-presentation");
            self.window.fullscreen();
        } else {
            {
                let mut st = self.inner.borrow_mut();
                let mut restore = st.pres_restore.take();
                let inner = &mut *st;
                restore_presentation_state(
                    &mut inner.presentation,
                    &mut restore,
                    &mut inner.view.layout_mode,
                    &mut inner.view.zoom,
                    &mut inner.pane.sidebar,
                );
                inner.pres_restore = restore;
            }
            self.window.remove_css_class("lector-presentation");
            self.toolbar.set_visible(true);
            self.window.unfullscreen();
            self.sync_sidebar();
        }
        let layout = self.inner.borrow().view.layout_mode;
        self.set_layout(layout);
        self.sync_zoom_dropdown();
    }

    pub(crate) fn toggle_sidebar(&self) {
        let on = self.inner.borrow().pane.sidebar == Sidebar::Hidden;
        self.sidebar_btn.set_active(on);
    }

    pub(crate) fn set_sidebar_visible(&self, on: bool) {
        {
            let mut st = self.inner.borrow_mut();
            if on {
                st.pane.sidebar = sidebar_when_revealed(st.pane.sidebar);
            } else {
                st.pane.sidebar = Sidebar::Hidden;
            }
        }
        let side = self.inner.borrow().pane.sidebar;
        self.sidebar_revealer
            .set_reveal_child(side != Sidebar::Hidden);
        match side {
            Sidebar::Thumbs => self.stack.set_visible_child_name("thumbs"),
            Sidebar::Outline => self.stack.set_visible_child_name("outline"),
            Sidebar::Marks => self.stack.set_visible_child_name("marks"),
            Sidebar::Hidden => {}
        }
        if on {
            self.populate_sidebar();
        } else {
            let mut st = self.inner.borrow_mut();
            st.thumb_gen = st.thumb_gen.wrapping_add(1);
        }
    }

    pub(crate) fn sync_sidebar(&self) {
        let on = self.inner.borrow().pane.sidebar != Sidebar::Hidden;
        if self.sidebar_btn.is_active() != on {
            self.sidebar_btn.set_active(on);
        } else {
            self.set_sidebar_visible(on);
        }
    }

    pub(crate) fn rebuild_sidebar(&self) {
        self.sidebar_ready.set(0);
        {
            let mut st = self.inner.borrow_mut();
            st.thumb_gen = st.thumb_gen.wrapping_add(1);
        }
        self.ignore_marks.set(true);
        for list in [&self.thumbs, &self.outline, &self.marks] {
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }
        }
        self.ignore_marks.set(false);
        self.populate_sidebar();
    }

    pub(crate) fn populate_sidebar(&self) {
        let side = self.inner.borrow().pane.sidebar;
        let bit = match side {
            Sidebar::Hidden => return,
            Sidebar::Thumbs => 1,
            Sidebar::Outline => 2,
            Sidebar::Marks => 4,
        };
        if self.sidebar_ready.get() & bit == 0 {
            self.sidebar_ready.set(self.sidebar_ready.get() | bit);
            match side {
                Sidebar::Thumbs => self.rebuild_thumbs(),
                Sidebar::Outline => self.rebuild_outline(),
                Sidebar::Marks => self.rebuild_marks(),
                Sidebar::Hidden => {}
            }
        } else if side == Sidebar::Marks {
            self.sync_marks_selection();
        }
        if side == Sidebar::Thumbs {
            self.schedule_thumbs();
        }
    }

    pub(crate) fn sync_marks_selection(&self) {
        self.ignore_marks.set(true);
        let targeted = self.inner.borrow().pane.targeted_annotation.clone();
        let mut selected = None;
        let mut child = self.marks.first_child();
        while let Some(widget) = child {
            child = widget.next_sibling();
            let Ok(row) = widget.downcast::<gtk::ListBoxRow>() else {
                continue;
            };
            if targeted.as_deref() == Some(row.widget_name().as_str()) {
                selected = Some(row);
            }
        }
        self.marks.select_row(selected.as_ref());
        self.ignore_marks.set(false);
    }

    pub(crate) fn rebuild_thumbs(&self) {
        let st = self.inner.borrow();
        let Some(doc) = st.view.doc.as_ref() else {
            return;
        };
        let n = doc.n_pages;
        let cap = n.min(1000);
        drop(st);
        {
            let mut st = self.inner.borrow_mut();
            st.thumb_gen = st.thumb_gen.wrapping_add(1);
        }
        for i in 0..cap {
            let row = gtk::ListBoxRow::new();
            let col = GtkBox::new(Orientation::Vertical, 4);
            col.set_margin_top(8);
            col.set_margin_bottom(8);
            col.set_margin_start(10);
            col.set_margin_end(10);
            col.set_widget_name("thumb-col");
            let lab = Label::new(Some(&format!("{}", i + 1)));
            lab.add_css_class("dim-label");
            col.append(&lab);
            row.set_child(Some(&col));
            self.thumbs.append(&row);
        }
        if n > cap {
            let row = gtk::ListBoxRow::new();
            let lab = Label::new(Some(&format!("… {n} pages")));
            lab.add_css_class("dim-label");
            lab.set_margin_top(8);
            lab.set_margin_bottom(8);
            row.set_child(Some(&lab));
            row.set_sensitive(false);
            self.thumbs.append(&row);
        }
    }

    pub(crate) fn rebuild_outline(&self) {
        while let Some(child) = self.outline.first_child() {
            self.outline.remove(&child);
        }
        let (items, collapsed) = {
            let st = self.inner.borrow();
            let Some(doc) = st.view.doc.as_ref() else {
                return;
            };
            (doc.outline.clone(), st.pane.outline_collapsed.clone())
        };
        for index in visible_outline_indices(&items, &collapsed) {
            let item = &items[index];
            let row = gtk::ListBoxRow::new();
            row.set_widget_name(&format!("o{index}"));
            let content = GtkBox::new(Orientation::Horizontal, 2);
            content.set_margin_start(6 + item.depth as i32 * 14);
            content.set_margin_end(8);
            content.set_margin_top(2);
            content.set_margin_bottom(2);

            if outline_item_has_children(&items, index) {
                let icon = if collapsed.contains(&index) {
                    "pan-end-symbolic"
                } else {
                    "pan-down-symbolic"
                };
                let toggle = Button::from_icon_name(icon);
                toggle.add_css_class("flat");
                toggle.set_tooltip_text(Some(if collapsed.contains(&index) {
                    "Expand section"
                } else {
                    "Collapse section"
                }));
                toggle.update_property(&[gtk::accessible::Property::Label(
                    if collapsed.contains(&index) {
                        "Expand section"
                    } else {
                        "Collapse section"
                    },
                )]);
                toggle.connect_clicked({
                    let h = Handle(self.me.borrow().clone());
                    move |_| {
                        let Some(ui) = h.get() else { return };
                        let changed = {
                            let mut st = ui.inner.borrow_mut();
                            let Some(items) = st.view.doc.as_ref().map(|doc| doc.outline.clone())
                            else {
                                return;
                            };
                            toggle_outline_collapsed(&items, &mut st.pane.outline_collapsed, index)
                        };
                        if changed {
                            ui.rebuild_outline();
                        }
                    }
                });
                content.append(&toggle);
            } else {
                let spacer = GtkBox::new(Orientation::Horizontal, 0);
                spacer.set_size_request(34, 1);
                content.append(&spacer);
            }

            let label = Label::new(Some(&item.title));
            label.set_xalign(0.0);
            label.set_hexpand(true);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            content.append(&label);
            row.set_child(Some(&content));
            self.outline.append(&row);
        }
    }

    pub(crate) fn rebuild_marks(&self) {
        self.ignore_marks.set(true);
        while let Some(child) = self.marks.first_child() {
            self.marks.remove(&child);
        }
        let (annotations, labels, targeted) = {
            let st = self.inner.borrow();
            let Some(doc) = st.view.doc.as_ref() else {
                self.ignore_marks.set(false);
                return;
            };
            let annotations = st
                .annotations
                .as_ref()
                .map(|store| store.for_document(&doc.path).to_vec())
                .unwrap_or_default();
            (
                annotations,
                doc.labels.clone(),
                st.pane.targeted_annotation.clone(),
            )
        };
        let mut selected = None;
        if annotations.is_empty() {
            let empty = Label::new(Some(marks_empty_copy()));
            empty.add_css_class("dim-label");
            empty.set_wrap(true);
            empty.set_xalign(0.0);
            empty.set_margin_top(12);
            empty.set_margin_start(8);
            empty.set_margin_end(8);
            self.marks.append(&empty);
            self.ignore_marks.set(false);
            return;
        }
        for annotation in annotations {
            let page_label = display_page_label(&labels, annotation.page);
            let kind = match &annotation.kind {
                AnnotationKind::Highlight => "Highlight".to_string(),
                AnnotationKind::Note { text } => {
                    let line = text.lines().next().unwrap_or(text).trim();
                    if line.is_empty() {
                        "Note".into()
                    } else {
                        ellipsize_chars(line, 42)
                    }
                }
            };
            let caption = format!("{page_label}  {kind}");
            let label = Label::new(Some(&caption));
            label.set_xalign(0.0);
            label.set_hexpand(true);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            let row = gtk::ListBoxRow::new();
            row.set_widget_name(&annotation.id);
            row.set_child(Some(&label));
            row.update_property(&[gtk::accessible::Property::Label(&caption)]);
            self.marks.append(&row);
            if targeted.as_deref() == Some(annotation.id.as_str()) {
                selected = Some(row);
            }
        }
        if let Some(row) = selected {
            self.marks.select_row(Some(&row));
        }
        self.ignore_marks.set(false);
    }

    pub(crate) fn activate_mark(&self, id: &str) {
        let annotation = {
            let st = self.inner.borrow();
            st.view.doc.as_ref().and_then(|doc| {
                st.annotations.as_ref().and_then(|store| {
                    store
                        .for_document(&doc.path)
                        .iter()
                        .find(|annotation| annotation.id == id)
                        .cloned()
                })
            })
        };
        let Some(annotation) = annotation else {
            return;
        };
        self.inner.borrow_mut().pane.targeted_annotation = Some(annotation.id.clone());
        self.scroll_to_annotation(&annotation);
        self.update_annotation_actions();
        self.queue_pages_draw();
        self.rebuild_marks();
        if matches!(annotation.kind, AnnotationKind::Note { .. }) {
            self.edit_note(&annotation);
        }
    }

    pub(crate) fn scroll_to_annotation(&self, annotation: &Annotation) {
        let y = {
            let st = self.inner.borrow();
            let Some(doc) = st.view.doc.as_ref() else {
                return;
            };
            let Some(geom) = st.view.layout.page_geom(annotation.page) else {
                return;
            };
            let (page_width, page_height) = doc.size(annotation.page, 0);
            let bounds = document_rect_to_display(
                annotation.rect,
                page_width,
                page_height,
                st.view.rotation,
            );
            geom.y + bounds.y * geom.scale - SEARCH_HIT_MARGIN
        };
        {
            let mut st = self.inner.borrow_mut();
            let n = st.view.doc.as_ref().map(|doc| doc.n_pages).unwrap_or(1);
            st.view.page = annotation.page.clamp(0, (n - 1).max(0));
        }
        self.update_chrome();
        self.inner.borrow_mut().ignore_scroll = true;
        if self.inner.borrow().view.layout_mode != LayoutMode::Continuous {
            self.inner.borrow_mut().ignore_scroll = false;
            self.reflow_restore(ViewRestore::CurrentPage);
            return;
        }
        let v = self.scrolled.vadjustment();
        v.set_value(clamp_adjustment(
            y.max(0.0),
            v.lower(),
            v.upper(),
            v.page_size(),
        ));
        self.inner.borrow_mut().ignore_scroll = false;
        self.sync_visible_page_widgets();
        self.ensure_visible_rasters();
    }

    pub(crate) fn schedule_thumbs(&self) {
        let gen = self.inner.borrow().thumb_gen;
        let h = Handle(self.me.borrow().clone());
        glib::idle_add_local_once(move || {
            if let Some(ui) = h.get() {
                ui.fill_visible_thumbs(gen);
            }
        });
    }

    pub(crate) fn fill_visible_thumbs(&self, gen: u64) {
        if self.inner.borrow().thumb_gen != gen {
            return;
        }
        if self.inner.borrow().pane.sidebar != Sidebar::Thumbs {
            return;
        }
        let v = self.thumbs_sw.vadjustment();
        let top = v.value();
        let bot = top + v.page_size().max(1.0);
        let pdf_mode = self.inner.borrow().view.pdf_mode;
        let mut filled = 0;
        let mut child = self.thumbs.first_child();
        while let Some(widget) = child {
            child = widget.next_sibling();
            let Ok(row) = widget.downcast::<gtk::ListBoxRow>() else {
                continue;
            };
            let Some(bounds) = row.compute_bounds(&self.thumbs) else {
                continue;
            };
            let y = bounds.y() as f64;
            let h = bounds.height() as f64;
            if y + h < top - 80.0 || y > bot + 80.0 {
                continue;
            }
            let Some(col) = row.child() else {
                continue;
            };
            if col
                .first_child()
                .and_then(|w| w.downcast::<gtk::Picture>().ok())
                .is_some()
            {
                continue;
            }
            let page = row.index();
            let dpr = self.page_dpr();
            let surf = {
                let st = self.inner.borrow();
                let Some(doc) = st.view.doc.as_ref() else {
                    return;
                };
                thumbnail(
                    doc,
                    page,
                    THUMB_CSS_WIDTH,
                    dpr,
                    pdf_mode,
                    st.theme.pdf_palette(),
                )
            };
            if let Some(surf) = surf {
                if let Some(tex) = surface_texture(&surf) {
                    let pic = gtk::Picture::for_paintable(&tex);
                    pic.set_can_shrink(true);
                    pic.set_content_fit(gtk::ContentFit::Contain);
                    let (cw, ch) = thumbnail_css_size(
                        surf.width() as f64 / dpr.max(1.0),
                        surf.height() as f64 / dpr.max(1.0),
                        THUMB_CSS_WIDTH,
                    );
                    pic.set_size_request(cw, ch);
                    if let Some(boxw) = col.downcast_ref::<GtkBox>() {
                        boxw.prepend(&pic);
                    }
                    filled += 1;
                    if filled >= 2 {
                        let h = Handle(self.me.borrow().clone());
                        glib::idle_add_local_once(move || {
                            if let Some(ui) = h.get() {
                                ui.fill_visible_thumbs(gen);
                            }
                        });
                        return;
                    }
                }
            }
        }
    }
}
