use super::*;

impl Lector {
    pub(crate) fn update_chrome(&self) {
        let st = self.inner.borrow();
        let has_doc = st.view.doc.is_some();
        self.prev_btn.set_sensitive(has_doc);
        self.next_btn.set_sensitive(has_doc);
        self.page_entry.set_sensitive(has_doc);
        self.zoom_out_btn.set_sensitive(has_doc);
        self.zoom_in_btn.set_sensitive(has_doc);
        self.zoom_drop.set_sensitive(has_doc);
        self.find_btn.set_sensitive(has_doc);
        for item in self.document_menu_items.borrow().iter() {
            item.set_sensitive(has_doc);
        }
        if let Some(doc) = st.view.doc.as_ref() {
            let title = if self.pdf_dirty.get() {
                format!("{} — Unsaved PDF changes", doc.title)
            } else {
                doc.title.clone()
            };
            self.window.set_title(Some(&title));
            self.title_label.set_text(&title);
            self.page_total.set_text(&format!("/ {}", doc.n_pages));
            if !self.page_entry.has_focus() {
                self.page_entry
                    .set_text(&display_page_label(&doc.labels, st.view.page));
            }
        } else {
            self.window.set_title(Some(""));
            self.title_label.set_text("Open a document");
            self.page_total.set_text("/ 0");
            self.page_entry.set_text("");
        }
        drop(st);
        self.update_navigation_buttons();
        self.update_annotation_actions();
        self.sync_edit_bar();
    }

    pub(crate) fn sync_toolbar(&self) {
        let st = self.inner.borrow();
        let show = toolbar_is_shown(st.presentation, st.toolbar_hidden);
        drop(st);
        self.toolbar.set_visible(show);
    }

    pub(crate) fn toggle_toolbar(&self) {
        if self.inner.borrow().presentation {
            return;
        }
        {
            let mut st = self.inner.borrow_mut();
            st.toolbar_hidden = !st.toolbar_hidden;
        }
        self.sync_toolbar();
    }

    pub(crate) fn key(&self, key: Key, mods: gdk::ModifierType) -> glib::Propagation {
        let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
        let shift = mods.contains(gdk::ModifierType::SHIFT_MASK);

        if let Some(direction) = navigation_shortcut(key, mods) {
            match direction {
                HistoryDirection::Back => self.navigate_back(),
                HistoryDirection::Forward => self.navigate_forward(),
            }
            return glib::Propagation::Stop;
        }

        if key == Key::Escape && (self.draw_mode.get() || self.text_placement.get()) {
            self.finish_edit_mode();
            self.inner.borrow_mut().view.selection = None;
            return glib::Propagation::Stop;
        }
        if let Some(propagation) = self.handle_inline_text_key(key, mods) {
            return propagation;
        }
        if ctrl
            && !shift
            && matches!(key, Key::z | Key::Z)
            && !self.search_entry.has_focus()
            && !self.page_entry.has_focus()
            && (self.draw_mode.get()
                || self.text_placement.get()
                || !self.edit_undo.borrow().is_empty())
        {
            self.undo_edit();
            return glib::Propagation::Stop;
        }
        if self.search_entry.has_focus() && !ctrl {
            if key == Key::Escape {
                self.hide_search();
                return glib::Propagation::Stop;
            }
            return glib::Propagation::Proceed;
        }
        if self.page_entry.has_focus() && !ctrl {
            if key == Key::Escape {
                self.pages_box.grab_focus();
                return glib::Propagation::Stop;
            }
            return glib::Propagation::Proceed;
        }

        if ctrl && shift {
            match key {
                Key::t | Key::T => {
                    self.add_text_dialog();
                    return glib::Propagation::Stop;
                }
                Key::d | Key::D => {
                    self.toggle_draw();
                    return glib::Propagation::Stop;
                }
                Key::f | Key::F => {
                    if let Some(dialog) = self.forms_dialog() {
                        dialog.present();
                    }
                    return glib::Propagation::Stop;
                }
                _ => {}
            }
        }

        if let Some(shortcut) = annotation_shortcut(key, mods) {
            match shortcut {
                AnnotationShortcut::History => self.show_reading_history(),
                AnnotationShortcut::Highlight => self.add_highlight(),
                AnnotationShortcut::Note => self.add_note(),
            }
            return glib::Propagation::Stop;
        }

        if ctrl {
            match key {
                Key::o => {
                    self.open_dialog();
                    return glib::Propagation::Stop;
                }
                Key::s => {
                    self.save_copy();
                    return glib::Propagation::Stop;
                }
                Key::p => {
                    self.print_doc();
                    return glib::Propagation::Stop;
                }
                Key::f => {
                    self.show_search();
                    return glib::Propagation::Stop;
                }
                Key::g => {
                    self.search_step(if shift { -1 } else { 1 });
                    return glib::Propagation::Stop;
                }
                Key::c => {
                    self.copy_selection();
                    return glib::Propagation::Stop;
                }
                Key::w | Key::q => {
                    self.window.close();
                    return glib::Propagation::Stop;
                }
                Key::r => {
                    if shift {
                        self.reload();
                    } else {
                        self.rotate(90);
                    }
                    return glib::Propagation::Stop;
                }
                Key::l => {
                    self.rotate(-90);
                    return glib::Propagation::Stop;
                }
                Key::i => {
                    let next = if self.inner.borrow().view.pdf_mode == PdfMode::Inverted {
                        PdfMode::Normal
                    } else {
                        PdfMode::Inverted
                    };
                    self.set_pdf_mode(next);
                    return glib::Propagation::Stop;
                }
                Key::plus | Key::equal | Key::KP_Add => {
                    self.zoom_by(1.15);
                    return glib::Propagation::Stop;
                }
                Key::minus | Key::KP_Subtract => {
                    self.zoom_by(1.0 / 1.15);
                    return glib::Propagation::Stop;
                }
                Key::_0 | Key::KP_0 => {
                    self.set_zoom(Zoom::Actual);
                    return glib::Propagation::Stop;
                }
                Key::Home => {
                    self.goto_page(0);
                    return glib::Propagation::Stop;
                }
                Key::End => {
                    let n = self
                        .inner
                        .borrow()
                        .view
                        .doc
                        .as_ref()
                        .map(|d| d.n_pages)
                        .unwrap_or(1);
                    self.goto_page(n - 1);
                    return glib::Propagation::Stop;
                }
                Key::Page_Down => {
                    self.goto_page_rel(1);
                    return glib::Propagation::Stop;
                }
                Key::Page_Up => {
                    self.goto_page_rel(-1);
                    return glib::Propagation::Stop;
                }
                _ => {}
            }
        }

        if !ctrl && !shift {
            let mode = self.inner.borrow().view.layout_mode;
            let adjustment = self.scrolled.vadjustment();
            if let Some(target) = continuous_arrow_scroll_target(
                mode,
                key,
                adjustment.value(),
                adjustment.lower(),
                adjustment.upper(),
                adjustment.page_size(),
            ) {
                adjustment.set_value(target);
                return glib::Propagation::Stop;
            }
        }

        match key {
            Key::Escape => {
                if self.inner.borrow().presentation {
                    self.toggle_presentation();
                } else if self.find_bar.reveals_child() {
                    self.hide_search();
                } else if self.window.is_fullscreen() {
                    self.toggle_fullscreen();
                }
                glib::Propagation::Stop
            }
            Key::F11 => {
                self.toggle_fullscreen();
                glib::Propagation::Stop
            }
            Key::F8 => {
                self.toggle_toolbar();
                glib::Propagation::Stop
            }
            Key::F5 => {
                self.toggle_presentation();
                glib::Propagation::Stop
            }
            Key::F9 => {
                self.toggle_sidebar();
                glib::Propagation::Stop
            }
            Key::Delete => {
                self.delete_targeted_annotation();
                glib::Propagation::Stop
            }
            Key::slash => {
                self.show_search();
                glib::Propagation::Stop
            }
            Key::w => {
                self.set_zoom(Zoom::FitWidth);
                glib::Propagation::Stop
            }
            Key::a => {
                self.set_zoom(Zoom::FitPage);
                glib::Propagation::Stop
            }
            Key::c => {
                self.set_layout(LayoutMode::Continuous);
                glib::Propagation::Stop
            }
            Key::d => {
                self.set_layout(LayoutMode::Dual);
                glib::Propagation::Stop
            }
            Key::s => {
                self.set_layout(LayoutMode::Single);
                glib::Propagation::Stop
            }
            Key::n | Key::Page_Down | Key::space | Key::Right => {
                if self.inner.borrow().view.layout_mode == LayoutMode::Continuous
                    && key == Key::space
                {
                    let v = self.scrolled.vadjustment();
                    v.set_value((v.value() + v.page_size() * 0.9).min(v.upper() - v.page_size()));
                } else {
                    self.goto_page_rel(1);
                }
                glib::Propagation::Stop
            }
            Key::p | Key::Page_Up | Key::BackSpace | Key::Left => {
                self.goto_page_rel(-1);
                glib::Propagation::Stop
            }
            Key::Home => {
                self.goto_page(0);
                glib::Propagation::Stop
            }
            Key::End => {
                let n = self
                    .inner
                    .borrow()
                    .view
                    .doc
                    .as_ref()
                    .map(|d| d.n_pages)
                    .unwrap_or(1);
                self.goto_page(n - 1);
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    }
}
