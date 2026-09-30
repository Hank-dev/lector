use super::*;

impl Lector {
    pub(crate) fn canvas_scroll(&self, ctrl: bool, dy: f64) -> glib::Propagation {
        if canvas_scroll_response(ctrl, dy, self.pinch_active.get()) == glib::Propagation::Stop {
            if ctrl && dy != 0.0 && !self.pinch_active.get() {
                self.zoom_by_at(ctrl_scroll_zoom_factor(dy), None);
            }
            glib::Propagation::Stop
        } else {
            // New user panning wins over the bounded post-layout restore.
            let mut st = self.inner.borrow_mut();
            st.layout_gen = st.layout_gen.wrapping_add(1);
            st.ignore_scroll = false;
            glib::Propagation::Proceed
        }
    }

    pub(crate) fn draw_page(
        &self,
        cr: &gtk::cairo::Context,
        index: i32,
        dpr: f64,
        width: i32,
        height: i32,
    ) {
        let view_top = self.scrolled.vadjustment().value();
        let view_h = self.scrolled.height() as f64;
        let dpr = dpr.max(1.0);
        let prepared = {
            let mut st = self.inner.borrow_mut();
            let Some(geom) = st.view.layout.page_geom(index) else {
                return;
            };
            let near =
                pages_intersecting_view(&st.view.layout.pages, view_top, view_h, VIEW_PAGE_PAD)
                    .contains(&index);
            let key = RasterKey::from_geom(
                &geom,
                st.view.rotation,
                st.view.pdf_mode,
                st.theme.pdf_palette(),
                dpr,
            );
            let cached = st.raster.get(&key);
            let exact_raster = cached.is_some();
            let visible_raster = cached
                .map(|surf| (surf, 0))
                .or_else(|| st.raster.get_page(&key));
            Some((
                geom,
                near,
                exact_raster,
                visible_raster,
                st.view.rotation,
                st.view.pdf_mode == PdfMode::Inverted,
                st.view.selection,
                st.search.idx,
            ))
        };
        let Some((
            geom,
            near,
            exact_raster,
            visible_raster,
            rotation,
            invert,
            selection,
            search_idx,
        )) = prepared
        else {
            return;
        };
        if let Some((surf, delta)) = visible_raster {
            // Rotate in normalized page coordinates before fitting the new CSS geometry.
            // No new surface or Poppler rendering is needed for immediate feedback.
            let (dest_w, dest_h) = page_paint_size(&geom);
            let dest_w = (width as f64).max(dest_w);
            let dest_h = (height as f64).max(dest_h);
            cr.save().ok();
            cr.scale(dest_w, dest_h);
            match delta {
                90 => cr.translate(1.0, 0.0),
                180 => cr.translate(1.0, 1.0),
                270 => cr.translate(0.0, 1.0),
                _ => {}
            }
            cr.rotate((delta as f64).to_radians());
            paint_raster(cr, &surf, 1.0, 1.0);
            cr.restore().ok();
        }
        if near && !exact_raster {
            self.queue_raster(index);
        }
        let st = self.inner.borrow();
        let Some(doc) = st.view.doc.as_ref() else {
            return;
        };
        let annotations = st
            .annotations
            .as_ref()
            .map(|store| store.for_document(&doc.path))
            .unwrap_or(&[]);
        let (page_width, page_height) = doc.size(index, 0);
        draw_annotations(
            cr,
            AnnotationDrawing {
                annotations,
                page: index,
                page_width,
                page_height,
                rotation,
                scale: geom.scale,
                targeted: st.pane.targeted_annotation.as_deref(),
            },
        );
        draw_page_overlays(
            cr,
            PageDrawing {
                doc,
                geom: &geom,
                rotation,
                invert,
                selection,
                search_hits: &st.search.hits,
                current_hit: search_idx,
            },
        );
        let pdf_mode = st.view.pdf_mode;
        let palette = st.theme.pdf_palette();
        let accent = st.theme.accent_rgb();
        let ink = |rgb| preview_rgb(rgb, pdf_mode, palette);
        if !exact_raster {
            // Until Poppler re-renders, keep committed ink visible.
            for (style, stroke) in doc.ink_display_strokes(index, rotation) {
                paint_ink_polyline(cr, &stroke, geom.scale, ink(style.rgb), style.width);
            }
        }
        if let Some(stroke) = self.ink_stroke.borrow().as_ref() {
            if stroke.page == index {
                let pen = self.pen.get();
                let smooth = crate::doc::smooth_polyline(&stroke.points);
                paint_ink_polyline(
                    cr,
                    &smooth,
                    geom.scale,
                    ink(pen.color.rgb()),
                    pen.width.points(),
                );
            }
        }
        if self.text_placement.get() {
            paint_free_text_frames(
                cr,
                &doc.free_text_rects(index),
                page_width,
                page_height,
                rotation,
                geom.scale,
                accent,
            );
        }
        if let Some(draft) = self.text_draft.borrow().as_ref() {
            if draft.page == index {
                paint_free_text_draft(
                    cr,
                    TextPreview {
                        rect: draft.rect(doc),
                        text: &draft.text,
                        cursor: draft.cursor,
                        size: draft.size,
                        rgb: ink(draft.rgb),
                        accent,
                        page_width,
                        page_height,
                        rotation,
                        scale: geom.scale,
                    },
                );
            }
        }
    }

    pub(crate) fn on_resize(&self, w: f64, h: f64) {
        if !viewport_resize_should_reflow(self.inner.borrow().ignore_scroll) {
            return;
        }
        let changed = {
            let mut st = self.inner.borrow_mut();
            let nw = w.max(1.0);
            let nh = h.max(1.0);
            let changed = (st.view.view_w - nw).abs() > 0.5 || (st.view.view_h - nh).abs() > 0.5;
            st.view.view_w = nw;
            st.view.view_h = nh;
            changed
        };
        if changed {
            self.reflow();
        }
    }

    pub(crate) fn on_scroll(&self) {
        let st = self.inner.borrow();
        if st.ignore_scroll {
            return;
        }
        if st.view.layout_mode == LayoutMode::Continuous && st.view.doc.is_some() {
            drop(st);
            self.sync_visible_page_widgets();
            let page = page_in_view(
                &self.inner.borrow().view.layout.pages,
                self.scrolled.vadjustment().value(),
                self.scrolled.height() as f64,
            );
            let chrome = {
                let mut st = self.inner.borrow_mut();
                if let Some(next) = continuous_scroll_page_changed(st.view.page, page) {
                    st.view.page = next;
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
                    true
                } else {
                    false
                }
            };
            if chrome {
                self.update_chrome();
            }
            self.ensure_visible_rasters();
        }
    }

    pub(crate) fn queue_pages_draw(&self) {
        let view_top = self.scrolled.vadjustment().value();
        let view_h = self.scrolled.height() as f64;
        let visible = {
            let st = self.inner.borrow();
            pages_intersecting_view(&st.view.layout.pages, view_top, view_h, VIEW_PAGE_PAD)
        };
        for (index, da) in self.page_widgets.borrow().iter() {
            if visible.contains(index) {
                da.queue_draw();
            }
        }
    }

    pub(crate) fn queue_raster(&self, index: i32) {
        self.raster_pending.borrow_mut().insert(index);
        self.kick_raster_idle();
    }

    pub(crate) fn ensure_visible_rasters(&self) {
        let view_top = self.scrolled.vadjustment().value();
        let view_h = self.scrolled.height() as f64;
        let needed = {
            let st = self.inner.borrow();
            if st.view.doc.is_none() {
                Vec::new()
            } else {
                pages_intersecting_view(&st.view.layout.pages, view_top, view_h, VIEW_PAGE_PAD)
            }
        };
        {
            let dpr = self.page_dpr();
            let st = self.inner.borrow();
            let mut pending = self.raster_pending.borrow_mut();
            pending.retain(|index| needed.contains(index));
            for index in needed {
                let Some(geom) = st.view.layout.page_geom(index) else {
                    continue;
                };
                let key = RasterKey::from_geom(
                    &geom,
                    st.view.rotation,
                    st.view.pdf_mode,
                    st.theme.pdf_palette(),
                    dpr,
                );
                if !st.raster.contains(&key) {
                    pending.insert(index);
                } else {
                    pending.remove(&index);
                }
            }
        }
        self.kick_raster_idle();
    }

    pub(crate) fn kick_raster_idle(&self) {
        if self.raster_busy.get() || self.raster_pending.borrow().is_empty() {
            return;
        }
        self.raster_busy.set(true);
        let gen = self.inner.borrow().raster_gen;
        let h = Handle(self.me.borrow().clone());
        glib::idle_add_local_once(move || {
            if let Some(ui) = h.get() {
                ui.raster_one(gen);
            }
        });
    }

    pub(crate) fn raster_one(&self, gen: u64) {
        if raster_idle_is_stale(gen, self.inner.borrow().raster_gen) {
            self.raster_busy.set(false);
            self.kick_raster_idle();
            return;
        }
        let index = self.take_next_raster_index();
        let Some(index) = index else {
            self.raster_busy.set(false);
            return;
        };
        // Reuse compatible pixels during a gesture. Poppler and recoloring run
        // on this thread and would otherwise stall input at every intermediate
        // scale. Newly exposed pages without a fallback still get rendered.
        let preview_available = if self.pinch_active.get() {
            let mut st = self.inner.borrow_mut();
            let key = st.view.layout.page_geom(index).map(|geom| {
                RasterKey::from_geom(
                    &geom,
                    st.view.rotation,
                    st.view.pdf_mode,
                    st.theme.pdf_palette(),
                    self.page_dpr(),
                )
            });
            key.is_some_and(|key| st.raster.get_page(&key).is_some())
        } else {
            false
        };
        if preview_available {
            self.raster_busy.set(false);
            self.kick_raster_idle();
            return;
        }
        let rendered = {
            let st = self.inner.borrow();
            match (st.view.doc.as_ref(), st.view.layout.page_geom(index)) {
                (Some(doc), Some(geom)) => {
                    let dpr = self.page_dpr();
                    let palette = st.theme.pdf_palette();
                    let key = RasterKey::from_geom(
                        &geom,
                        st.view.rotation,
                        st.view.pdf_mode,
                        palette,
                        dpr,
                    );
                    if st.raster.contains(&key) {
                        None
                    } else {
                        raster_page(doc, &geom, st.view.rotation, st.view.pdf_mode, palette, dpr)
                            .map(|surf| (key, surf))
                    }
                }
                _ => None,
            }
        };
        if let Some((key, surf)) = rendered {
            self.inner.borrow_mut().raster.insert(key, surf);
            if let Some((_, da)) = self.page_widgets.borrow().iter().find(|(i, _)| *i == index) {
                da.queue_draw();
            }
        }
        self.raster_busy.set(false);
        self.kick_raster_idle();
    }

    pub(crate) fn take_next_raster_index(&self) -> Option<i32> {
        let view_top = self.scrolled.vadjustment().value();
        let st = self.inner.borrow();
        let pages = &st.view.layout.pages;
        let needed = pages_intersecting_view(
            pages,
            view_top,
            self.scrolled.height() as f64,
            VIEW_PAGE_PAD,
        );
        let mut pending = self.raster_pending.borrow_mut();
        pending.retain(|index| needed.contains(index));
        let next = pending.iter().copied().min_by_key(|index| {
            let y = pages
                .iter()
                .find(|page| page.index == *index)
                .map(|page| page.y)
                .unwrap_or(0.0);
            ((y - view_top).abs() * 1000.0) as i64
        })?;
        pending.remove(&next);
        Some(next)
    }

    pub(crate) fn finish_pinch(&self) {
        self.pinch_active.set(false);
        self.ensure_visible_rasters();
    }

    pub(crate) fn page_dpr(&self) -> f64 {
        self.window.scale_factor().max(1) as f64
    }

    pub(crate) fn hit_at(&self, x: f64, y: f64) -> Option<(i32, f64, f64)> {
        let st = self.inner.borrow();
        let widgets = self.page_widgets.borrow();
        for (index, da) in widgets.iter() {
            let Some(b) = da.compute_bounds(&self.pages_box) else {
                continue;
            };
            let x0 = b.x() as f64;
            let y0 = b.y() as f64;
            let w = b.width() as f64;
            let h = b.height() as f64;
            if x >= x0 && x <= x0 + w && y >= y0 && y <= y0 + h {
                let geom = st.view.layout.page_geom(*index)?;
                let scale = geom.scale.max(0.01);
                return Some((*index, (x - x0) / scale, (y - y0) / scale));
            }
        }
        None
    }

    pub(crate) fn alert(&self, message: &str, detail: &str) {
        let alert = gtk::AlertDialog::builder()
            .modal(true)
            .message(message)
            .detail(detail)
            .build();
        alert.show(Some(&self.window));
    }

    pub(crate) fn capture_anchor(&self, focus: Option<(f64, f64)>) -> ViewportAnchor {
        let (view_x, view_y) = focus.unwrap_or_else(|| {
            (
                (self.scrolled.width() as f64 * 0.5).max(0.0),
                (self.scrolled.height() as f64 * 0.5).max(0.0),
            )
        });
        let pages = self.inner.borrow().view.layout.pages.clone();
        let (offset_x, offset_y) = self.canvas_center_offset(&pages);
        let content_x = self.scrolled.hadjustment().value() + view_x - offset_x;
        let content_y = self.scrolled.vadjustment().value() + view_y - offset_y;
        anchor_from_content(&pages, content_x, content_y, view_x, view_y)
    }

    fn canvas_center_offset(&self, pages: &[crate::view::PageGeom]) -> (f64, f64) {
        let (w, h) = layout_content_size(pages);
        (
            ((self.scrolled.width() as f64 - w.ceil()) * 0.5)
                .floor()
                .max(0.0),
            ((self.scrolled.height() as f64 - h.ceil()) * 0.5)
                .floor()
                .max(0.0),
        )
    }

    pub(crate) fn apply_anchor(&self, anchor: ViewportAnchor) {
        let pages = self.inner.borrow().view.layout.pages.clone();
        let (offset_x, offset_y) = self.canvas_center_offset(&pages);
        let (x, y) = restored_scroll(anchor, &pages);
        let (x, y) = (x + offset_x, y + offset_y);
        let h = self.scrolled.hadjustment();
        let v = self.scrolled.vadjustment();
        h.set_value(clamp_adjustment(x, h.lower(), h.upper(), h.page_size()));
        v.set_value(clamp_adjustment(y, v.lower(), v.upper(), v.page_size()));
    }

    pub(crate) fn apply_view_restore(&self, restore: ViewRestore) {
        match restore {
            ViewRestore::Anchor(anchor) => self.apply_anchor(anchor),
            ViewRestore::CurrentPage => self.scroll_to_current(),
            ViewRestore::Location(location) => {
                let horizontal = self.scrolled.hadjustment();
                horizontal.set_value(clamp_adjustment(
                    location.horizontal,
                    horizontal.lower(),
                    horizontal.upper(),
                    horizontal.page_size(),
                ));
                let vertical = self.scrolled.vadjustment();
                vertical.set_value(clamp_adjustment(
                    location.vertical,
                    vertical.lower(),
                    vertical.upper(),
                    vertical.page_size(),
                ));
            }
        }
        // Adjustment callbacks are suppressed during reflow. Reconcile the
        // virtualized pages explicitly against the restored viewport, not the
        // old scroll position at which the new layout was initially installed.
        self.sync_visible_page_widgets();
        self.ensure_visible_rasters();
    }

    pub(crate) fn schedule_view_restore(&self, restore: ViewRestore) {
        let gen = {
            let mut st = self.inner.borrow_mut();
            st.ignore_scroll = true;
            st.layout_gen = st.layout_gen.wrapping_add(1);
            st.layout_gen
        };
        let h = Handle(self.me.borrow().clone());
        let mut attempts = 0_u8;
        glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
            let Some(ui) = h.get() else {
                return glib::ControlFlow::Break;
            };
            if ui.inner.borrow().layout_gen != gen {
                return glib::ControlFlow::Break;
            }

            ui.apply_view_restore(restore);
            attempts += 1;
            if attempts >= 6 {
                ui.inner.borrow_mut().ignore_scroll = false;
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
    }

    pub(crate) fn reflow(&self) {
        self.reflow_restore(ViewRestore::Anchor(self.capture_anchor(None)));
    }

    pub(crate) fn reflow_restore(&self, restore: ViewRestore) {
        let reset_rasters = {
            let st = &mut *self.inner.borrow_mut();
            if st.view.doc.is_none() {
                return;
            }
            st.ignore_scroll = true;
            let doc = st.view.doc.as_ref().expect("document checked above");
            let vw = self.scrolled.width() as f64;
            let vh = self.scrolled.height() as f64;
            if vw > 1.0 {
                st.view.view_w = vw;
            }
            if vh > 1.0 {
                st.view.view_h = vh;
            }
            let old_scale = st.view.layout.scale;
            let old_rotation = st.view.rotation;
            let old_mode = st.view.pdf_mode;
            st.view.layout = Layout::compute(
                doc,
                st.view.layout_mode,
                st.view.zoom,
                st.view.rotation,
                st.view.page,
                st.view.view_w.max(200.0),
                st.view.view_h.max(200.0),
                st.view.dual_cover,
            );
            let reset = raster_cache_needs_reset(
                old_scale,
                st.view.layout.scale,
                old_rotation,
                st.view.rotation,
                old_mode,
                st.view.pdf_mode,
            );
            let clear = raster_cache_needs_clear(
                old_scale,
                st.view.layout.scale,
                old_rotation,
                st.view.rotation,
                old_mode,
                st.view.pdf_mode,
            );
            if clear {
                st.raster.clear();
            }
            if reset {
                st.raster_gen = st.raster_gen.wrapping_add(1);
            }
            reset
        };
        if reset_rasters {
            self.raster_pending.borrow_mut().clear();
        }
        let structure = {
            let st = self.inner.borrow();
            PageStructure::from_layout(st.view.layout_mode, &st.view.layout)
        };
        if self.rendered_structure.borrow().as_ref() == Some(&structure) {
            self.resize_page_widgets();
        } else {
            self.rebuild_pages();
        }
        self.update_chrome();
        self.apply_view_restore(restore);
        self.schedule_view_restore(restore);
        self.ensure_visible_rasters();
    }

    pub(crate) fn resize_page_widgets(&self) {
        self.sync_visible_page_widgets();
    }

    pub(crate) fn visible_canvas_pages(&self) -> Vec<i32> {
        let view_top = self.scrolled.vadjustment().value();
        let view_h = self.scrolled.height() as f64;
        let st = self.inner.borrow();
        if st.view.layout_mode != LayoutMode::Continuous {
            return st.view.layout.pages.iter().map(|page| page.index).collect();
        }
        pages_intersecting_view(&st.view.layout.pages, view_top, view_h, VIEW_PAGE_PAD)
    }

    pub(crate) fn sync_visible_page_widgets(&self) {
        let pages = self.inner.borrow().view.layout.pages.clone();
        let (content_w, content_h) = layout_content_size(&pages);
        self.pages_box.set_size_request(
            content_w.ceil().max(1.0) as i32,
            content_h.ceil().max(1.0) as i32,
        );
        let view_w = self.scrolled.width() as f64;
        let view_h = self.scrolled.height() as f64;
        self.pages_box
            .set_valign(if content_h + 0.5 < view_h.max(1.0) {
                gtk::Align::Center
            } else {
                gtk::Align::Start
            });
        if content_w + 0.5 <= view_w.max(1.0) {
            self.scrolled.hadjustment().set_value(0.0);
        }
        if pages.is_empty() {
            return;
        }
        let needed = self.visible_canvas_pages();
        let current = self
            .page_widgets
            .borrow()
            .iter()
            .map(|(index, _)| *index)
            .collect::<Vec<_>>();
        if current != needed {
            let keep = needed.iter().copied().collect::<HashSet<_>>();
            let h = Handle(self.me.borrow().clone());
            let mut widgets = self.page_widgets.borrow_mut();
            widgets.retain(|(index, da)| {
                if keep.contains(index) {
                    true
                } else {
                    self.pages_box.remove(da);
                    false
                }
            });
            for index in &needed {
                if widgets.iter().any(|(existing, _)| existing == index) {
                    continue;
                }
                let Some(geom) = pages.iter().copied().find(|page| page.index == *index) else {
                    continue;
                };
                let da = self.make_page_da(geom, &h);
                self.pages_box.put(&da, geom.x, geom.y);
                widgets.push((*index, da));
            }
        }
        let widgets = self.page_widgets.borrow();
        for geom in &pages {
            if let Some((_, da)) = widgets.iter().find(|(i, _)| *i == geom.index) {
                let (pw, ph) = page_alloc_size(geom);
                da.set_content_width(pw);
                da.set_content_height(ph);
                da.set_size_request(pw, ph);
                self.pages_box.move_(da, geom.x, geom.y);
                da.queue_draw();
            }
        }
    }

    pub(crate) fn scroll_to_current(&self) {
        let y = {
            let st = self.inner.borrow();
            page_scroll_y(&st.view.layout.pages, st.view.page)
        };
        let v = self.scrolled.vadjustment();
        let max = (v.upper() - v.page_size()).max(0.0);
        v.set_value(y.min(max));
    }

    pub(crate) fn make_page_da(&self, geom: crate::view::PageGeom, h: &Handle) -> DrawingArea {
        let da = DrawingArea::new();
        let (pw, ph) = page_alloc_size(&geom);
        da.set_content_width(pw);
        da.set_content_height(ph);
        da.set_size_request(pw, ph);
        da.set_halign(gtk::Align::Center);
        da.set_hexpand(false);
        da.add_css_class("lector-page");
        let index = geom.index;
        da.set_draw_func({
            let h = h.clone();
            move |da, cr, width, height| {
                if let Some(ui) = h.get() {
                    ui.draw_page(cr, index, da.scale_factor() as f64, width, height);
                }
            }
        });
        da
    }

    pub(crate) fn rebuild_pages(&self) {
        let pages = self.inner.borrow().view.layout.pages.clone();
        // Remove the empty-document placeholder, but let viewport synchronization
        // reconcile real page widgets instead of tearing down reusable DrawingAreas.
        if pages.is_empty() || self.page_widgets.borrow().is_empty() {
            while let Some(child) = self.pages_box.first_child() {
                self.pages_box.remove(&child);
            }
            self.page_widgets.borrow_mut().clear();
        }
        let mode = self.inner.borrow().view.layout_mode;
        *self.rendered_structure.borrow_mut() = Some(PageStructure::new(
            mode,
            pages.iter().map(|page| page.index).collect(),
        ));
        if pages.is_empty() {
            let lab = Label::new(Some("Open a document — Ctrl+O"));
            self.pages_box.put(&lab, 48.0, 48.0);
            self.pages_box.set_size_request(400, 200);
            return;
        }
        self.sync_visible_page_widgets();
        self.ensure_visible_rasters();
    }
}
