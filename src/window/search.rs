use super::*;

impl Lector {
    pub(crate) fn search(&self, query: &str) {
        let q = query.trim().to_string();
        let gen = {
            let mut st = self.inner.borrow_mut();
            st.search.hits.clear();
            st.search.idx = None;
            st.search.query = q.clone();
            st.search.gen = st.search.gen.wrapping_add(1);
            st.search.gen
        };
        if q.is_empty() {
            self.search_info.set_text("");
            self.clear_search_results();
            self.queue_pages_draw();
            return;
        }
        let start = self.inner.borrow().view.page;
        {
            let mut st = self.inner.borrow_mut();
            if let Some(doc) = st.view.doc.as_ref() {
                st.search.hits = doc.find_page(start, &q, st.search.options);
                if !st.search.hits.is_empty() {
                    st.search.idx = Some(0);
                }
            }
        }
        self.jump_search();
        self.search_info.set_text("Searching…");
        let n_pages = self
            .inner
            .borrow()
            .view
            .doc
            .as_ref()
            .map(|doc| doc.n_pages)
            .unwrap_or(0);
        if let Some(job) = SearchJob::after_first_page(n_pages, start) {
            self.continue_search(gen, job);
        } else {
            self.finish_search();
        }
    }

    pub(crate) fn continue_search(&self, gen: u64, job: SearchJob) {
        let h = Handle(self.me.borrow().clone());
        glib::idle_add_local_once(move || {
            let Some(ui) = h.get() else { return };
            if ui.inner.borrow().search.gen != gen {
                return;
            }
            let n_pages = ui
                .inner
                .borrow()
                .view
                .doc
                .as_ref()
                .map(|d| d.n_pages)
                .unwrap_or(0);
            let mut job = job;
            let Some(page) = job.take_page(n_pages) else {
                ui.finish_search();
                return;
            };
            let q = ui.inner.borrow().search.query.clone();
            let options = ui.inner.borrow().search.options;
            let extra = ui
                .inner
                .borrow()
                .view
                .doc
                .as_ref()
                .map(|d| d.find_page(page, &q, options))
                .unwrap_or_default();
            if !extra.is_empty() {
                let mut st = ui.inner.borrow_mut();
                st.search.hits.extend(extra);
                if st.search.idx.is_none() {
                    st.search.idx = Some(0);
                    drop(st);
                    ui.jump_search();
                } else {
                    let i = st.search.idx.unwrap_or(0);
                    let n_hits = st.search.hits.len();
                    drop(st);
                    ui.search_info.set_text(&format!("{} of {}", i + 1, n_hits));
                    ui.rebuild_search_results();
                    ui.queue_pages_draw();
                }
            }
            if job.remaining > 0 {
                ui.continue_search(gen, job);
            } else {
                ui.finish_search();
            }
        });
    }

    pub(crate) fn finish_search(&self) {
        let (n_hits, search_idx) = {
            let st = self.inner.borrow();
            (st.search.hits.len(), st.search.idx)
        };
        if n_hits == 0 {
            self.search_info.set_text("Not found");
            self.clear_search_results();
        } else if search_idx.is_none() {
            self.inner.borrow_mut().search.idx = Some(0);
            self.jump_search();
        } else {
            let index = search_idx.unwrap_or(0);
            self.search_info
                .set_text(&format!("{} of {}", index + 1, n_hits));
            self.rebuild_search_results();
            self.queue_pages_draw();
        }
    }

    pub(crate) fn search_step(&self, dir: i32) {
        let mut st = self.inner.borrow_mut();
        let n = st.search.hits.len() as i32;
        if n == 0 {
            return;
        }
        let cur = st.search.idx.unwrap_or(0) as i32;
        st.search.idx = Some((cur + dir).rem_euclid(n) as usize);
        drop(st);
        self.jump_search();
    }

    pub(crate) fn jump_search(&self) {
        let (page, n, i) = {
            let st = self.inner.borrow();
            let n = st.search.hits.len();
            match st.search.idx {
                Some(i) if i < n => (st.search.hits[i].0, n, i),
                _ => {
                    self.search_info
                        .set_text(if n == 0 { "Not found" } else { "" });
                    self.queue_pages_draw();
                    return;
                }
            }
        };
        self.search_info.set_text(&format!("{} of {}", i + 1, n));
        self.scroll_to_search_hit(page, i);
        self.rebuild_search_results();
        self.queue_pages_draw();
    }

    pub(crate) fn scroll_to_search_hit(&self, page: i32, hit: usize) {
        let (y, horizontal) = {
            let st = self.inner.borrow();
            let Some((_, rect)) = st.search.hits.get(hit) else {
                return;
            };
            let Some(doc) = st.view.doc.as_ref() else {
                return;
            };
            let y = search_hit_scroll_y(
                &st.view.layout.pages,
                doc,
                st.view.rotation,
                page,
                rect,
                (
                    self.scrolled.vadjustment().value(),
                    self.scrolled.height() as f64,
                ),
                SEARCH_HIT_MARGIN,
            );
            (y, self.scrolled.hadjustment().value())
        };
        {
            let mut st = self.inner.borrow_mut();
            let n = st.view.doc.as_ref().map(|doc| doc.n_pages).unwrap_or(1);
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
        }
        self.update_chrome();
        self.inner.borrow_mut().ignore_scroll = true;
        if self.inner.borrow().view.layout_mode != LayoutMode::Continuous {
            self.inner.borrow_mut().ignore_scroll = false;
            self.reflow_restore(ViewRestore::Location(ViewLocation::new(
                page, horizontal, y,
            )));
            return;
        }
        let v = self.scrolled.vadjustment();
        v.set_value(clamp_adjustment(y, v.lower(), v.upper(), v.page_size()));
        self.inner.borrow_mut().ignore_scroll = false;
        self.sync_visible_page_widgets();
        self.ensure_visible_rasters();
    }

    pub(crate) fn hide_search(&self) {
        self.cancel_live_search();
        self.find_bar.set_reveal_child(false);
        {
            let mut st = self.inner.borrow_mut();
            st.search.hits.clear();
            st.search.idx = None;
            st.search.gen = st.search.gen.wrapping_add(1);
        }
        self.clear_search_results();
        self.queue_pages_draw();
        self.pages_box.grab_focus();
    }

    pub(crate) fn show_search(&self) {
        self.find_bar.set_reveal_child(true);
        self.search_entry.grab_focus();
    }

    pub(crate) fn cancel_live_search(&self) {
        self.search_debounce_gen
            .set(self.search_debounce_gen.get().wrapping_add(1));
    }

    pub(crate) fn queue_live_search(&self) {
        let gen = self.search_debounce_gen.get().wrapping_add(1);
        self.search_debounce_gen.set(gen);
        let h = Handle(self.me.borrow().clone());
        glib::timeout_add_local(Duration::from_millis(LIVE_SEARCH_MS), move || {
            let Some(ui) = h.get() else {
                return glib::ControlFlow::Break;
            };
            if ui.search_debounce_gen.get() != gen {
                return glib::ControlFlow::Break;
            }
            let query = ui.search_entry.text();
            ui.search(&query);
            glib::ControlFlow::Break
        });
    }

    pub(crate) fn rerun_visible_search(&self) {
        if !self.find_bar.reveals_child() {
            return;
        }
        let query = self.search_entry.text();
        if query.is_empty() {
            return;
        }
        self.cancel_live_search();
        self.search(&query);
    }

    pub(crate) fn clear_search_results(&self) {
        self.ignore_search_results.set(true);
        while let Some(child) = self.search_results.first_child() {
            self.search_results.remove(&child);
        }
        self.search_results_sw.set_visible(false);
        self.ignore_search_results.set(false);
    }

    pub(crate) fn rebuild_search_results(&self) {
        let (rows, selected) = {
            let st = self.inner.borrow();
            if st.search.hits.is_empty() {
                (Vec::new(), None)
            } else {
                let query = st.search.query.clone();
                let case_sensitive = st.search.options.case_sensitive;
                let labels = st
                    .view
                    .doc
                    .as_ref()
                    .map(|doc| doc.labels.clone())
                    .unwrap_or_default();
                let mut texts = HashMap::new();
                if let Some(doc) = st.view.doc.as_ref() {
                    for (page, _) in &st.search.hits {
                        texts.entry(*page).or_insert_with(|| {
                            doc.page(*page)
                                .and_then(|page| page.text().map(|text| text.to_string()))
                                .unwrap_or_default()
                        });
                    }
                }
                let rows = st
                    .search
                    .hits
                    .iter()
                    .enumerate()
                    .map(|(index, (page, _))| {
                        let label = display_page_label(&labels, *page);
                        let snippet = texts
                            .get(page)
                            .map(|text| search_snippet(text, &query, case_sensitive))
                            .unwrap_or_default();
                        (index, search_result_row_text(&label, &snippet))
                    })
                    .collect::<Vec<_>>();
                (rows, st.search.idx)
            }
        };
        if rows.is_empty() {
            self.clear_search_results();
            return;
        }
        self.ignore_search_results.set(true);
        while let Some(child) = self.search_results.first_child() {
            self.search_results.remove(&child);
        }
        let mut selected_row = None;
        for (index, caption) in rows {
            let label = Label::new(Some(&caption));
            label.set_xalign(0.0);
            label.set_ellipsize(gtk::pango::EllipsizeMode::End);
            label.set_hexpand(true);
            let row = gtk::ListBoxRow::new();
            row.set_widget_name(&format!("s{index}"));
            row.set_child(Some(&label));
            self.search_results.append(&row);
            if selected == Some(index) {
                selected_row = Some(row);
            }
        }
        if let Some(row) = selected_row {
            self.search_results.select_row(Some(&row));
        }
        self.search_results_sw.set_visible(true);
        self.ignore_search_results.set(false);
    }
}
