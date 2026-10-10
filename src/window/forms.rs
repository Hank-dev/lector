use super::*;
use crate::doc::FreeTextSpec;
use poppler::FormFieldType;

const MAX_DRAFT_CHARS: usize = 2000;

/// Text being typed on the page. The origin is the box's top-left corner in
/// unrotated, top-origin document coordinates.
pub(crate) struct TextDraft {
    pub(crate) page: i32,
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) text: String,
    /// Byte offset of the caret, always on a char boundary.
    pub(crate) cursor: usize,
    pub(crate) size: f64,
    pub(crate) rgb: (f64, f64, f64),
    /// The embedded answer this draft is editing; it is out of the PDF meanwhile.
    pub(crate) replaced: Option<FreeTextSpec>,
    pub(crate) was_dirty: bool,
}

impl TextDraft {
    pub(crate) fn rect(&self, doc: &OpenDoc) -> DocumentRect {
        let (width, height) = doc.size(self.page, 0);
        crate::view::free_text_rect(self.x, self.y, &self.text, self.size, width, height)
    }

    fn unchanged(&self) -> bool {
        self.replaced.as_ref().is_some_and(|old| {
            old.text == self.text
                && old.size == self.size
                && old.rgb == self.rgb
                && (old.rect.x - self.x).abs() < 0.01
                && (old.rect.y - self.y).abs() < 0.01
        })
    }

    fn line_start(&self, at: usize) -> usize {
        self.text[..at].rfind('\n').map(|i| i + 1).unwrap_or(0)
    }

    fn line_end(&self, at: usize) -> usize {
        self.text[at..]
            .find('\n')
            .map(|i| at + i)
            .unwrap_or(self.text.len())
    }

    fn prev_boundary(&self, at: usize) -> usize {
        self.text[..at]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0)
    }

    fn next_boundary(&self, at: usize) -> usize {
        self.text[at..]
            .chars()
            .next()
            .map(|c| at + c.len_utf8())
            .unwrap_or(at)
    }

    fn prev_word(&self, at: usize) -> usize {
        let head = self.text[..at].trim_end_matches(|c: char| c.is_whitespace() && c != '\n');
        let head = if head.len() == at && head.ends_with('\n') {
            &head[..head.len() - 1]
        } else {
            head
        };
        head.rfind(|c: char| c.is_whitespace())
            .map(|i| i + head[i..].chars().next().map_or(1, char::len_utf8))
            .unwrap_or(0)
    }

    fn next_word(&self, at: usize) -> usize {
        let tail = &self.text[at..];
        let skip_space = tail.len() - tail.trim_start().len();
        let rest = &tail[skip_space..];
        at + skip_space + rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len())
    }

    /// Moves the caret one line up or down, keeping its column in chars.
    fn vertical(&self, at: usize, down: bool) -> usize {
        let start = self.line_start(at);
        let column = self.text[start..at].chars().count();
        let target = if down {
            let end = self.line_end(at);
            if end == self.text.len() {
                return self.text.len();
            }
            end + 1
        } else {
            if start == 0 {
                return 0;
            }
            self.line_start(start - 1)
        };
        let end = self.line_end(target);
        self.text[target..end]
            .char_indices()
            .nth(column)
            .map(|(i, _)| target + i)
            .unwrap_or(end)
    }

    pub(crate) fn insert(&mut self, text: &str) {
        let room = MAX_DRAFT_CHARS.saturating_sub(self.text.chars().count());
        let clean = text
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\t', " ")
            .chars()
            .filter(|c| *c == '\n' || !c.is_control())
            .take(room)
            .collect::<String>();
        self.text.insert_str(self.cursor, &clean);
        self.cursor += clean.len();
    }

    fn delete_range(&mut self, from: usize, to: usize) {
        let (from, to) = (from.min(to), from.max(to));
        self.text.replace_range(from..to, "");
        self.cursor = from;
    }
}

/// A press in Add Text mode that may become a move of the draft box.
pub(crate) struct TextPress {
    page: i32,
    widget: (f64, f64),
    start: (f64, f64),
    origin: (f64, f64),
    moving: bool,
}

impl Lector {
    pub(crate) fn cancel_text_placement(&self) {
        self.commit_text_draft();
        self.text_press.borrow_mut().take();
        let was_placing = self.text_placement.replace(false);
        if !was_placing {
            return;
        }
        if !self.draw_mode.get() {
            self.pages_box.set_cursor_from_name(None);
        }
        self.queue_pages_draw();
        self.update_chrome();
    }

    fn doc_point(&self, x: f64, y: f64) -> Option<(i32, f64, f64)> {
        let (page, px, py) = self.hit_at(x, y)?;
        let st = self.inner.borrow();
        let doc = st.view.doc.as_ref()?;
        let (dx, dy) = display_point_to_document(doc, page, st.view.rotation, px, py);
        Some((page, dx, dy))
    }

    /// Press in Add Text mode: move the open draft, edit added text under the
    /// pointer, or start a new draft with its caret centred on the click.
    pub(crate) fn place_text_caret(&self, x: f64, y: f64) {
        let Some((page, dx, dy)) = self.doc_point(x, y) else {
            self.commit_text_draft();
            return;
        };
        let on_draft = {
            let st = self.inner.borrow();
            let draft = self.text_draft.borrow();
            match (st.view.doc.as_ref(), draft.as_ref()) {
                (Some(doc), Some(draft)) if draft.page == page => {
                    let r = draft.rect(doc);
                    let pad = 4.0;
                    (dx >= r.x - pad
                        && dx <= r.x + r.width + pad
                        && dy >= r.y - pad
                        && dy <= r.y + r.height + pad)
                        .then_some((r.x, r.y))
                }
                _ => None,
            }
        };
        let origin = match on_draft {
            Some(origin) => origin,
            None => {
                self.commit_text_draft();
                let hit = self
                    .inner
                    .borrow()
                    .view
                    .doc
                    .as_ref()
                    .and_then(|doc| doc.free_text_at(page, dx, dy));
                match hit {
                    Some((annot, spec)) => {
                        let origin = (spec.rect.x, spec.rect.y);
                        if !self.edit_free_text(page, &annot, spec) {
                            return;
                        }
                        origin
                    }
                    None => {
                        let pen = self.pen.get();
                        let origin = (dx, dy - pen.text_size * crate::view::FREE_TEXT_CARET_CENTER);
                        *self.text_draft.borrow_mut() = Some(TextDraft {
                            page,
                            x: origin.0,
                            y: origin.1,
                            text: String::new(),
                            cursor: 0,
                            size: pen.text_size,
                            rgb: pen.color.rgb(),
                            replaced: None,
                            was_dirty: self.pdf_dirty.get(),
                        });
                        origin
                    }
                }
            }
        };
        *self.text_press.borrow_mut() = Some(TextPress {
            page,
            widget: (x, y),
            start: (dx, dy),
            origin,
            moving: false,
        });
        self.start_text_input();
        self.pages_box.set_cursor_from_name(Some("text"));
        self.pages_box.grab_focus();
        self.queue_pages_draw();
        self.update_chrome();
    }

    /// Takes an embedded answer out of the PDF and opens it as a draft.
    fn edit_free_text(&self, page: i32, annot: &poppler::Annot, spec: FreeTextSpec) -> bool {
        let was_dirty = self.pdf_dirty.get();
        let removed = self
            .inner
            .borrow()
            .view
            .doc
            .as_ref()
            .map(|doc| doc.remove_fill_text(page, annot));
        match removed {
            Some(Ok(())) => {}
            Some(Err(error)) => {
                self.alert("Could not edit text", &error);
                return false;
            }
            None => return false,
        }
        *self.text_draft.borrow_mut() = Some(TextDraft {
            page,
            x: spec.rect.x,
            y: spec.rect.y,
            cursor: spec.text.len(),
            text: spec.text.clone(),
            size: spec.size,
            rgb: spec.rgb,
            replaced: Some(spec),
            was_dirty,
        });
        self.pdf_changed(page);
        true
    }

    pub(crate) fn drag_text(&self, x: f64, y: f64) {
        let mut press = self.text_press.borrow_mut();
        let Some(active) = press.as_mut() else {
            return;
        };
        if !active.moving {
            let (wx, wy) = active.widget;
            if (x - wx).powi(2) + (y - wy).powi(2) < 16.0 {
                return;
            }
            active.moving = true;
            self.pages_box.set_cursor_from_name(Some("grabbing"));
        }
        let Some((page, dx, dy)) = self.doc_point(x, y) else {
            return;
        };
        if page != active.page {
            return;
        }
        let origin = (
            active.origin.0 + dx - active.start.0,
            active.origin.1 + dy - active.start.1,
        );
        drop(press);
        if let Some(draft) = self.text_draft.borrow_mut().as_mut() {
            draft.x = origin.0;
            draft.y = origin.1;
        }
        self.queue_pages_draw();
    }

    /// A moved box with text is placed at once; a click keeps typing.
    pub(crate) fn release_text(&self) {
        let Some(press) = self.text_press.borrow_mut().take() else {
            return;
        };
        if !press.moving {
            return;
        }
        let has_text = self
            .text_draft
            .borrow()
            .as_ref()
            .is_some_and(|draft| !draft.text.trim().is_empty());
        if has_text {
            self.commit_text_draft();
        }
        if self.text_placement.get() {
            self.pages_box
                .set_cursor_from_name(Some(if self.text_draft.borrow().is_some() {
                    "text"
                } else {
                    "crosshair"
                }));
        }
    }

    fn start_text_input(&self) {
        self.keys.set_im_context(Some(&self.im));
        self.im.focus_in();
        self.im.reset();
    }

    fn stop_text_input(&self) {
        self.im.reset();
        self.im.focus_out();
        self.keys.set_im_context(None::<&gtk::IMContext>);
    }

    /// Writes the draft into the PDF. Empty drafts are dropped; an emptied
    /// edit deletes the original. Safe to call when there is no draft.
    pub(crate) fn commit_text_draft(&self) {
        let Some(draft) = self.text_draft.borrow_mut().take() else {
            return;
        };
        self.stop_text_input();
        if self.text_placement.get() {
            self.pages_box.set_cursor_from_name(Some("crosshair"));
        }
        if draft.text.trim().is_empty() {
            if let Some(old) = draft.replaced {
                self.push_undo(EditUndo::Text {
                    page: draft.page,
                    added: None,
                    replaced: Some(old),
                });
                self.pdf_changed(draft.page);
            } else {
                self.queue_pages_draw();
                self.update_chrome();
            }
            return;
        }
        let unchanged = draft.unchanged();
        let result = {
            let st = self.inner.borrow();
            let Some(doc) = st.view.doc.as_ref() else {
                return;
            };
            let spec = match (&draft.replaced, unchanged) {
                (Some(old), true) => old.clone(),
                _ => FreeTextSpec {
                    rect: draft.rect(doc),
                    text: draft.text.clone(),
                    size: draft.size,
                    rgb: draft.rgb,
                },
            };
            doc.add_free_text(draft.page, &spec).map(|_| spec)
        };
        match result {
            Ok(spec) => {
                if !unchanged {
                    self.push_undo(EditUndo::Text {
                        page: draft.page,
                        added: Some(spec),
                        replaced: draft.replaced,
                    });
                }
                self.pdf_changed(draft.page);
                if unchanged {
                    self.pdf_dirty.set(draft.was_dirty);
                    self.update_chrome();
                }
            }
            Err(error) => {
                self.queue_pages_draw();
                self.alert("Could not add text", &error);
            }
        }
    }

    /// Drops the draft; an edited answer goes back exactly as it was.
    pub(crate) fn discard_text_draft(&self) {
        let Some(draft) = self.text_draft.borrow_mut().take() else {
            return;
        };
        self.stop_text_input();
        self.text_press.borrow_mut().take();
        if self.text_placement.get() {
            self.pages_box.set_cursor_from_name(Some("crosshair"));
        }
        if let Some(old) = draft.replaced {
            let result = self
                .inner
                .borrow()
                .view
                .doc
                .as_ref()
                .map(|doc| doc.add_free_text(draft.page, &old));
            if let Some(Err(error)) = result {
                self.alert("Could not restore text", &error);
            }
            self.pdf_changed(draft.page);
            self.pdf_dirty.set(draft.was_dirty);
        }
        self.queue_pages_draw();
        self.update_chrome();
    }

    pub(crate) fn handle_inline_text_key(
        &self,
        key: Key,
        mods: gdk::ModifierType,
    ) -> Option<glib::Propagation> {
        if self.text_draft.borrow().is_none() {
            return None;
        }
        let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
        let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);
        let alt = mods.contains(gdk::ModifierType::ALT_MASK);
        if key == Key::Escape {
            // Handled by the caller: Esc keeps the text and leaves the tool.
            return None;
        }
        if ctrl {
            match key {
                Key::v | Key::V if !shift => self.paste_into_draft(),
                Key::z | Key::Z if !shift => self.discard_text_draft(),
                // There is no redo; swallow it rather than run undo.
                Key::z | Key::Z => {}
                Key::BackSpace => {
                    self.edit_draft(|d| d.delete_range(d.prev_word(d.cursor), d.cursor))
                }
                Key::Delete | Key::KP_Delete => {
                    self.edit_draft(|d| d.delete_range(d.cursor, d.next_word(d.cursor)))
                }
                Key::Left | Key::KP_Left => self.edit_draft(|d| d.cursor = d.prev_word(d.cursor)),
                Key::Right | Key::KP_Right => self.edit_draft(|d| d.cursor = d.next_word(d.cursor)),
                Key::Home | Key::KP_Home => self.edit_draft(|d| d.cursor = 0),
                Key::End | Key::KP_End => self.edit_draft(|d| d.cursor = d.text.len()),
                Key::s | Key::S => {
                    // Save a Copy must include what is being typed.
                    self.commit_text_draft();
                    return None;
                }
                // Other shortcuts (zoom, find, open…) still work while typing.
                _ => return None,
            }
            return Some(glib::Propagation::Stop);
        }
        match key {
            Key::Return | Key::KP_Enter | Key::ISO_Enter => {
                if shift {
                    self.edit_draft(|d| d.insert("\n"));
                } else {
                    self.commit_text_draft();
                }
            }
            Key::BackSpace => self.edit_draft(|d| {
                let from = d.prev_boundary(d.cursor);
                d.delete_range(from, d.cursor);
            }),
            Key::Delete | Key::KP_Delete => self.edit_draft(|d| {
                let to = d.next_boundary(d.cursor);
                d.delete_range(d.cursor, to);
            }),
            Key::Left | Key::KP_Left => self.edit_draft(|d| d.cursor = d.prev_boundary(d.cursor)),
            Key::Right | Key::KP_Right => self.edit_draft(|d| d.cursor = d.next_boundary(d.cursor)),
            Key::Up | Key::KP_Up => self.edit_draft(|d| d.cursor = d.vertical(d.cursor, false)),
            Key::Down | Key::KP_Down => self.edit_draft(|d| d.cursor = d.vertical(d.cursor, true)),
            Key::Home | Key::KP_Home => self.edit_draft(|d| d.cursor = d.line_start(d.cursor)),
            Key::End | Key::KP_End => self.edit_draft(|d| d.cursor = d.line_end(d.cursor)),
            _ if !alt => {
                // Real key events reach the IM context first; this covers
                // direct calls and keys the IM passes through.
                if let Some(ch) = key.to_unicode().filter(|c| !c.is_control()) {
                    self.insert_inline_text(&ch.to_string());
                }
            }
            _ => return None,
        }
        Some(glib::Propagation::Stop)
    }

    fn edit_draft(&self, edit: impl FnOnce(&mut TextDraft)) {
        if let Some(draft) = self.text_draft.borrow_mut().as_mut() {
            edit(draft);
        }
        self.queue_pages_draw();
        self.update_chrome();
    }

    pub(crate) fn insert_inline_text(&self, text: &str) {
        self.edit_draft(|draft| draft.insert(text));
    }

    fn paste_into_draft(&self) {
        let h = Handle(self.me.borrow().clone());
        self.pages_box
            .clipboard()
            .read_text_async(gio::Cancellable::NONE, move |result| {
                if let (Some(ui), Ok(Some(text))) = (h.get(), result) {
                    ui.insert_inline_text(&text);
                }
            });
    }

    /// The PDF changed on `page`: marks it unsaved and re-renders that page
    /// and its thumbnail. Its old pixels stay on screen meanwhile.
    pub(crate) fn pdf_changed(&self, page: i32) {
        self.pdf_dirty.set(true);
        {
            let mut st = self.inner.borrow_mut();
            st.raster.invalidate_page(page);
            st.view.selection = None;
        }
        self.refresh_thumb(page);
        self.queue_pages_draw();
        self.ensure_visible_rasters();
        self.update_chrome();
    }

    pub(crate) fn confirm_discard(&self, action: impl FnOnce(&Lector) + 'static) {
        let body = GtkBox::new(Orientation::Vertical, 12);
        let label = Label::new(Some("Your PDF changes have not been saved.\nCancel and use Save a Copy (Ctrl+S), or discard them."));
        label.set_wrap(true);
        body.append(&label);
        let cancel = Button::with_label("Cancel");
        let discard = Button::with_label("Discard Changes");
        body.append(&cancel);
        body.append(&discard);
        let dialog = self.themed_dialog("Unsaved PDF changes", &body);
        let weak = dialog.downgrade();
        cancel.connect_clicked(move |_| {
            if let Some(d) = weak.upgrade() {
                d.close();
            }
        });
        let weak = dialog.downgrade();
        let h = Handle(self.me.borrow().clone());
        let action = RefCell::new(Some(action));
        discard.connect_clicked(move |_| {
            if let Some(d) = weak.upgrade() {
                d.close();
            }
            if let (Some(ui), Some(action)) = (h.get(), action.borrow_mut().take()) {
                ui.pdf_dirty.set(false);
                action(&ui);
            }
        });
        dialog.present();
    }

    pub(crate) fn save_copy_to(&self, path: &Path) -> Result<(), String> {
        self.commit_text_draft();
        {
            let st = self.inner.borrow();
            let doc = st.view.doc.as_ref().ok_or("No document")?;
            doc.save_filled_copy(path)?;
        }
        self.pdf_dirty.set(false);
        self.update_chrome();
        Ok(())
    }

    pub(crate) fn add_text_dialog(&self) {
        self.cancel_draw();
        if self.inner.borrow().view.doc.is_none() {
            return;
        }
        self.commit_text_draft();
        self.text_placement.set(true);
        self.inner.borrow_mut().view.selection = None;
        self.pages_box.set_cursor_from_name(Some("crosshair"));
        self.pages_box.grab_focus();
        self.queue_pages_draw();
        self.update_chrome();
    }

    pub(crate) fn forms_dialog(&self) -> Option<gtk::Window> {
        use glib::translate::{from_glib_none, ToGlibPtr};
        let st = self.inner.borrow();
        let doc = st.view.doc.as_ref()?;
        let mut fields = Vec::new();
        for index in 0..doc.n_pages {
            if let Some(page) = doc.page(index) {
                for mapping in page.form_field_mapping() {
                    // The mapping owns a field reference; retain our own before dropping it.
                    let field: poppler::FormField = unsafe {
                        let ptr: *mut poppler::ffi::PopplerFormFieldMapping =
                            mapping.to_glib_none().0;
                        from_glib_none((*ptr).field)
                    };
                    fields.push((index, field));
                }
            }
        }
        drop(st);
        let body = GtkBox::new(Orientation::Vertical, 10);
        let guidance = Label::new(Some("Changes apply immediately. Close this dialog, then Save a Copy (Ctrl+S).\nXFA, signatures and scripted calculations are not supported."));
        guidance.set_wrap(true);
        body.append(&guidance);
        let mut radios: HashMap<String, CheckButton> = HashMap::new();
        for (page, field) in fields {
            let name = field
                .name()
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("Field {}", field.id()));
            body.append(&Label::new(Some(&format!("Page {} — {name}", page + 1))));
            let h = Handle(self.me.borrow().clone());
            let writable = !field.is_read_only();
            match field.field_type() {
                FormFieldType::Text
                    if field.text_get_text_type() == poppler::FormTextType::Multiline =>
                {
                    let text = gtk::TextView::new();
                    text.set_widget_name(&name);
                    text.set_wrap_mode(gtk::WrapMode::WordChar);
                    text.set_size_request(-1, 100);
                    text.set_sensitive(writable);
                    let buffer = text.buffer();
                    buffer.set_text(field.text_get_text().as_deref().unwrap_or(""));
                    buffer.connect_changed(move |buffer| {
                        if !writable {
                            return;
                        }
                        let value = buffer.text(&buffer.start_iter(), &buffer.end_iter(), true);
                        field.text_set_text(&value);
                        if let Some(ui) = h.get() {
                            ui.pdf_changed(page);
                        }
                    });
                    body.append(&text);
                }
                FormFieldType::Text => {
                    let entry = Entry::new();
                    entry.set_widget_name(&name);
                    entry.set_text(field.text_get_text().as_deref().unwrap_or(""));
                    entry.set_visibility(!field.text_is_password());
                    entry.set_max_length(field.text_get_max_len().max(0));
                    entry.set_sensitive(writable);
                    entry.connect_changed(move |entry| {
                        if !writable {
                            return;
                        }
                        field.text_set_text(&entry.text());
                        if let Some(ui) = h.get() {
                            ui.pdf_changed(page);
                        }
                    });
                    body.append(&entry);
                }
                FormFieldType::Button
                    if field.button_get_button_type() != poppler::FormButtonType::Push =>
                {
                    let radio = field.button_get_button_type() == poppler::FormButtonType::Radio;
                    let button =
                        CheckButton::with_label(&format!("{} (widget {})", name, field.id()));
                    button.set_widget_name(&name);
                    if radio {
                        if let Some(first) = radios.get(&name) {
                            button.set_group(Some(first));
                        } else {
                            radios.insert(name.clone(), button.clone());
                        }
                    }
                    button.set_active(field.button_get_state());
                    button.set_sensitive(writable);
                    button.connect_toggled(move |button| {
                        if !writable || (radio && !button.is_active()) {
                            return;
                        }
                        field.button_set_state(button.is_active());
                        if let Some(ui) = h.get() {
                            ui.pdf_changed(page);
                        }
                    });
                    body.append(&button);
                }
                FormFieldType::Choice => {
                    let items = (0..field.choice_get_n_items())
                        .map(|i| field.choice_get_item(i).unwrap_or_default().to_string())
                        .collect::<Vec<_>>();
                    if field.choice_can_select_multiple() {
                        for (i, item) in items.iter().enumerate() {
                            let button = CheckButton::with_label(item);
                            button.set_widget_name(&name);
                            button.set_sensitive(writable);
                            button.set_active(field.choice_is_item_selected(i as i32));
                            let field = field.clone();
                            let h = h.clone();
                            button.connect_toggled(move |button| {
                                if !writable {
                                    return;
                                }
                                if field.choice_is_item_selected(i as i32) != button.is_active() {
                                    field.choice_toggle_item(i as i32);
                                }
                                if let Some(ui) = h.get() {
                                    ui.pdf_changed(page);
                                }
                            });
                            body.append(&button);
                        }
                    } else {
                        let options = items.iter().map(String::as_str).collect::<Vec<_>>();
                        let drop = DropDown::from_strings(&options);
                        drop.set_widget_name(&name);
                        drop.set_sensitive(writable);
                        drop.set_selected(
                            (0..items.len())
                                .find(|i| field.choice_is_item_selected(*i as i32))
                                .map(|i| i as u32)
                                .unwrap_or(gtk::INVALID_LIST_POSITION),
                        );
                        let selected_field = field.clone();
                        let selected_h = h.clone();
                        drop.connect_selected_notify(move |drop| {
                            if !writable {
                                return;
                            }
                            selected_field.choice_unselect_all();
                            if drop.selected() != gtk::INVALID_LIST_POSITION {
                                selected_field.choice_select_item(drop.selected() as i32);
                            }
                            if let Some(ui) = selected_h.get() {
                                ui.pdf_changed(page);
                            }
                        });
                        body.append(&drop);
                        if field.choice_is_editable() {
                            let entry = Entry::new();
                            entry.set_placeholder_text(Some("Custom value"));
                            entry.set_sensitive(writable);
                            entry.set_text(field.choice_get_text().as_deref().unwrap_or(""));
                            entry.connect_changed(move |entry| {
                                if !writable {
                                    return;
                                }
                                field.choice_set_text(&entry.text());
                                if let Some(ui) = h.get() {
                                    ui.pdf_changed(page);
                                }
                            });
                            body.append(&entry);
                        }
                    }
                }
                _ => body.append(&Label::new(Some("Unsupported field (not editable)"))),
            }
        }
        let scroll = ScrolledWindow::new();
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.set_max_content_height(480);
        scroll.set_propagate_natural_height(true);
        scroll.set_child(Some(&body));
        Some(self.themed_dialog("Fill Form", &scroll))
    }
}
