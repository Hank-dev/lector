use super::*;

impl Lector {
    pub(crate) fn drag_begin(&self, x: f64, y: f64) {
        if self.draw_mode.get() {
            self.begin_ink_stroke(x, y);
            return;
        }
        if self.text_placement.get() {
            self.place_text_caret(x, y);
            return;
        }
        let hit = self.hit_at(x, y);
        {
            let st = &mut *self.inner.borrow_mut();
            begin_selection_state(
                &mut st.view.selection,
                &mut st.pane.targeted_annotation,
                hit,
            );
        }
        self.queue_pages_draw();
        self.update_annotation_actions();
    }

    pub(crate) fn drag_update(&self, x: f64, y: f64) {
        if self.draw_mode.get() {
            self.extend_ink_stroke(x, y);
            return;
        }
        if self.text_placement.get() {
            self.drag_text(x, y);
            return;
        }
        {
            let hit = self.hit_at(x, y);
            let st = &mut *self.inner.borrow_mut();
            let Some(sel) = st.view.selection.as_mut() else {
                return;
            };
            if let Some((page, px, py)) = hit {
                if page == sel.page {
                    sel.x2 = px;
                    sel.y2 = py;
                }
            }
        }
        self.queue_pages_draw();
        self.update_annotation_actions();
    }

    pub(crate) fn copy_selection(&self) {
        let st = self.inner.borrow();
        let (Some(doc), Some(sel)) = (st.view.doc.as_ref(), st.view.selection) else {
            return;
        };
        if let Some(text) = selected_text(doc, st.view.rotation, sel) {
            if let Some(display) = gdk::Display::default() {
                display.clipboard().set_text(&text);
            }
        }
    }

    pub(crate) fn selected_annotation_region(&self) -> Option<(PathBuf, i32, DocumentRect)> {
        let (path, page, rects) = self.selected_annotation_rects()?;
        Some((path, page, union_rects(&rects)?))
    }

    pub(crate) fn selected_annotation_rects(&self) -> Option<(PathBuf, i32, Vec<DocumentRect>)> {
        let st = self.inner.borrow();
        let doc = st.view.doc.as_ref()?;
        let selection = st.view.selection?;
        selected_text(doc, st.view.rotation, selection)?;
        let rects = selection_highlight_rects(doc, st.view.rotation, selection);
        if rects.is_empty() {
            return None;
        }
        Some((doc.path.clone(), selection.page, rects))
    }

    pub(crate) fn update_annotation_actions(&self) {
        let can_create = self.selected_annotation_region().is_some()
            && self.inner.borrow().annotations.is_some();
        self.highlight_item.set_sensitive(can_create);
        self.note_item.set_sensitive(can_create);
        self.delete_annotation_item
            .set_sensitive(self.annotation_to_delete().is_some());
    }

    pub(crate) fn annotation_to_delete(&self) -> Option<String> {
        let selected = self.selected_annotation_region();
        let st = self.inner.borrow();
        let doc = st.view.doc.as_ref()?;
        let annotations = st.annotations.as_ref()?.for_document(&doc.path);
        if let Some(id) = st.pane.targeted_annotation.as_ref() {
            if annotations.iter().any(|annotation| &annotation.id == id) {
                return Some(id.clone());
            }
        }
        let (_, page, rect) = selected?;
        annotations
            .iter()
            .rev()
            .find(|annotation| {
                annotation.page == page
                    && annotation
                        .fragments()
                        .any(|fragment| document_rects_intersect(fragment, rect))
            })
            .map(|annotation| annotation.id.clone())
    }

    pub(crate) fn annotation_store_error(&self) -> String {
        self.inner
            .borrow()
            .annotation_error
            .clone()
            .unwrap_or_else(|| "The annotation store is unavailable.".into())
    }

    pub(crate) fn create_annotation(&self, kind: AnnotationKind) {
        let Some((path, page, rects)) = self.selected_annotation_rects() else {
            return;
        };
        let annotation = match &kind {
            AnnotationKind::Note { .. } => {
                let Some(rect) = union_rects(&rects) else {
                    return;
                };
                new_annotation(page, rect, kind)
            }
            AnnotationKind::Highlight => {
                let Some(annotation) = new_annotation_from_rects(page, rects, kind) else {
                    return;
                };
                annotation
            }
        };
        let result = {
            let st = self.inner.borrow();
            st.annotations.as_ref().map(|store| {
                let mut candidate = store.clone();
                candidate.add(&path, annotation.clone());
                candidate.save().map(|_| candidate)
            })
        };
        match result {
            Some(Ok(candidate)) => {
                let mut st = self.inner.borrow_mut();
                st.annotations = Some(candidate);
                st.pane.targeted_annotation = Some(annotation.id);
                drop(st);
                self.queue_pages_draw();
                self.update_annotation_actions();
                self.rebuild_marks();
            }
            Some(Err(error)) => self.alert("Could not save annotation", &error),
            None => self.alert("Annotations unavailable", &self.annotation_store_error()),
        }
    }

    pub(crate) fn add_highlight(&self) {
        self.create_annotation(AnnotationKind::Highlight);
    }

    pub(crate) fn add_note(&self) {
        if self.selected_annotation_region().is_none() {
            return;
        }
        if self.inner.borrow().annotations.is_none() {
            self.alert("Annotations unavailable", &self.annotation_store_error());
            return;
        }
        let text = gtk::TextView::new();
        text.set_wrap_mode(gtk::WrapMode::WordChar);
        text.set_size_request(420, 130);
        text.set_tooltip_text(Some("Text attached to the selected PDF region"));
        let save = Button::with_label("Add Note");
        save.add_css_class("suggested-action");
        save.set_sensitive(false);
        let cancel = Button::with_label("Cancel");
        let actions = GtkBox::new(Orientation::Horizontal, 8);
        actions.set_halign(gtk::Align::End);
        actions.append(&cancel);
        actions.append(&save);
        let body = GtkBox::new(Orientation::Vertical, 10);
        body.append(&text);
        body.append(&actions);
        let dialog = self.themed_dialog("Add Note", &body);
        text.buffer().connect_changed({
            let save = save.clone();
            move |buffer| {
                let value = buffer.text(&buffer.start_iter(), &buffer.end_iter(), true);
                save.set_sensitive(!value.trim().is_empty());
            }
        });
        cancel.connect_clicked({
            let dialog = dialog.clone();
            move |_| dialog.close()
        });
        save.connect_clicked({
            let dialog = dialog.clone();
            let text = text.clone();
            let h = Handle(self.me.borrow().clone());
            move |_| {
                let buffer = text.buffer();
                let value = buffer
                    .text(&buffer.start_iter(), &buffer.end_iter(), true)
                    .trim()
                    .to_string();
                if value.is_empty() {
                    return;
                }
                dialog.close();
                if let Some(ui) = h.get() {
                    ui.create_annotation(AnnotationKind::Note { text: value });
                }
            }
        });
        dialog.present();
        text.grab_focus();
    }

    pub(crate) fn delete_targeted_annotation(&self) {
        let Some(id) = self.annotation_to_delete() else {
            return;
        };
        let path = {
            let st = self.inner.borrow();
            let Some(doc) = st.view.doc.as_ref() else {
                return;
            };
            doc.path.clone()
        };
        let result = {
            let st = self.inner.borrow();
            st.annotations.as_ref().map(|store| {
                let mut candidate = store.clone();
                if !candidate.remove(&path, &id) {
                    return Ok(candidate);
                }
                candidate.save().map(|_| candidate)
            })
        };
        match result {
            Some(Ok(candidate)) => {
                let mut st = self.inner.borrow_mut();
                st.annotations = Some(candidate);
                if st.pane.targeted_annotation.as_deref() == Some(id.as_str()) {
                    st.pane.targeted_annotation = None;
                }
                drop(st);
                self.queue_pages_draw();
                self.update_annotation_actions();
                self.rebuild_marks();
            }
            Some(Err(error)) => self.alert("Could not delete annotation", &error),
            None => self.alert("Annotations unavailable", &self.annotation_store_error()),
        }
    }

    pub(crate) fn annotation_at_view(
        &self,
        x: f64,
        y: f64,
        marker_only: bool,
    ) -> Option<Annotation> {
        let (page, display_x, display_y) = self.hit_at(x, y)?;
        let st = self.inner.borrow();
        let doc = st.view.doc.as_ref()?;
        let (page_width, page_height) = doc.size(page, 0);
        let marker_radius = 11.0 / st.view.layout.scale.max(0.01);
        st.annotations
            .as_ref()?
            .for_document(&doc.path)
            .iter()
            .rev()
            .filter(|annotation| annotation.page == page)
            .find(|annotation| {
                annotation.fragments().any(|fragment| {
                    let bounds = document_rect_to_display(
                        fragment,
                        page_width,
                        page_height,
                        st.view.rotation,
                    );
                    if matches!(annotation.kind, AnnotationKind::Note { .. }) {
                        let dx = display_x - (bounds.x + bounds.width);
                        let dy = display_y - bounds.y;
                        if dx * dx + dy * dy <= marker_radius * marker_radius {
                            return true;
                        }
                    }
                    !marker_only && document_rect_contains(bounds, display_x, display_y)
                })
            })
            .cloned()
    }

    /// Primary-button release on the pages. `first_was_link` remembers whether
    /// the first click of a double-click followed a link.
    pub(crate) fn click_released(&self, n: i32, x: f64, y: f64, first_was_link: &Cell<bool>) {
        // Edit tools own every click, double-clicks included.
        if self.draw_mode.get() || self.text_placement.get() {
            first_was_link.set(false);
            return;
        }
        if n == 1 {
            let selecting = self
                .inner
                .borrow()
                .view
                .selection
                .is_some_and(|s| (s.x2 - s.x1).abs() > 3.0 || (s.y2 - s.y1).abs() > 3.0);
            if selecting {
                first_was_link.set(false);
                return;
            }
            if self.activate_note_at(x, y) {
                first_was_link.set(false);
            } else {
                let link = self.activate_link_at(x, y);
                if !link {
                    self.target_annotation_at(x, y);
                }
                first_was_link.set(link);
            }
        } else {
            let has_link = self.link_at_view(x, y).is_some();
            if should_toggle_fit(n, has_link, first_was_link.get()) {
                self.toggle_fit();
            }
            first_was_link.set(false);
        }
    }

    pub(crate) fn activate_note_at(&self, x: f64, y: f64) -> bool {
        let Some(annotation) = self.annotation_at_view(x, y, true) else {
            return false;
        };
        if !matches!(annotation.kind, AnnotationKind::Note { .. }) {
            return false;
        }
        self.inner.borrow_mut().pane.targeted_annotation = Some(annotation.id.clone());
        self.queue_pages_draw();
        self.update_annotation_actions();
        self.edit_note(&annotation);
        true
    }

    pub(crate) fn edit_note(&self, annotation: &Annotation) {
        let AnnotationKind::Note { text } = &annotation.kind else {
            return;
        };
        let text_view = gtk::TextView::new();
        text_view.set_wrap_mode(gtk::WrapMode::WordChar);
        text_view.set_size_request(420, 130);
        text_view.set_tooltip_text(Some("Text attached to the selected PDF region"));
        text_view.buffer().set_text(text);
        let save = Button::with_label("Save");
        save.add_css_class("suggested-action");
        save.set_sensitive(!text.trim().is_empty());
        let cancel = Button::with_label("Cancel");
        let actions = GtkBox::new(Orientation::Horizontal, 8);
        actions.set_halign(gtk::Align::End);
        actions.append(&cancel);
        actions.append(&save);
        let body = GtkBox::new(Orientation::Vertical, 10);
        body.append(&text_view);
        body.append(&actions);
        let dialog = self.themed_dialog("Edit Note", &body);
        text_view.buffer().connect_changed({
            let save = save.clone();
            move |buffer| {
                let value = buffer.text(&buffer.start_iter(), &buffer.end_iter(), true);
                save.set_sensitive(!value.trim().is_empty());
            }
        });
        cancel.connect_clicked({
            let dialog = dialog.clone();
            move |_| dialog.close()
        });
        save.connect_clicked({
            let dialog = dialog.clone();
            let text_view = text_view.clone();
            let annotation = annotation.clone();
            let h = Handle(self.me.borrow().clone());
            move |_| {
                let buffer = text_view.buffer();
                let value = buffer
                    .text(&buffer.start_iter(), &buffer.end_iter(), true)
                    .trim()
                    .to_string();
                if value.is_empty() {
                    return;
                }
                dialog.close();
                if let Some(ui) = h.get() {
                    let mut updated = annotation.clone();
                    updated.kind = AnnotationKind::Note { text: value };
                    ui.upsert_annotation(updated);
                }
            }
        });
        dialog.present();
        text_view.grab_focus();
    }

    pub(crate) fn upsert_annotation(&self, annotation: Annotation) {
        let path = {
            let st = self.inner.borrow();
            let Some(doc) = st.view.doc.as_ref() else {
                return;
            };
            doc.path.clone()
        };
        let result = {
            let st = self.inner.borrow();
            st.annotations.as_ref().map(|store| {
                let mut candidate = store.clone();
                candidate.add(&path, annotation.clone());
                candidate.save().map(|_| candidate)
            })
        };
        match result {
            Some(Ok(candidate)) => {
                let mut st = self.inner.borrow_mut();
                st.annotations = Some(candidate);
                st.pane.targeted_annotation = Some(annotation.id);
                drop(st);
                self.queue_pages_draw();
                self.update_annotation_actions();
                self.rebuild_marks();
            }
            Some(Err(error)) => self.alert("Could not save annotation", &error),
            None => self.alert("Annotations unavailable", &self.annotation_store_error()),
        }
    }

    pub(crate) fn target_annotation_at(&self, x: f64, y: f64) -> bool {
        let replacement = self
            .annotation_at_view(x, y, false)
            .map(|annotation| annotation.id);
        let found = replacement.is_some();
        if replace_annotation_target(
            &mut self.inner.borrow_mut().pane.targeted_annotation,
            replacement,
        ) {
            self.queue_pages_draw();
        }
        self.update_annotation_actions();
        found
    }
}
