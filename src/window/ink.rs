use super::*;

pub(crate) struct ActiveInk {
    pub(crate) page: i32,
    pub(crate) points: Vec<(f64, f64)>,
    /// The smoothed curve up to the last segment that new points no longer
    /// change, so long strokes are not re-smoothed on every motion event.
    settled: Vec<(f64, f64)>,
}

impl ActiveInk {
    pub(crate) fn new(page: i32, point: (f64, f64)) -> Self {
        Self {
            page,
            points: vec![point],
            settled: Vec::new(),
        }
    }

    pub(crate) fn push(&mut self, point: (f64, f64)) {
        self.points.push(point);
        let n = self.points.len();
        if n >= 3 {
            if self.settled.is_empty() {
                self.settled.push(self.points[0]);
            }
            crate::doc::smooth_segments(&self.points, n - 3..n - 2, &mut self.settled);
        }
    }

    /// The same curve as `smooth_polyline(&self.points)`.
    pub(crate) fn preview(&self) -> Vec<(f64, f64)> {
        let n = self.points.len();
        if n < 3 {
            return self.points.clone();
        }
        let mut curve = self.settled.clone();
        crate::doc::smooth_segments(&self.points, n - 2..n - 1, &mut curve);
        curve
    }
}

impl Lector {
    pub(crate) fn toggle_draw(&self) {
        if self.draw_mode.get() {
            self.cancel_draw();
        } else {
            self.begin_draw();
        }
    }

    pub(crate) fn begin_draw(&self) {
        if self.inner.borrow().view.doc.is_none() {
            return;
        }
        self.cancel_text_placement();
        self.inner.borrow_mut().view.selection = None;
        self.draw_mode.set(true);
        self.ink_stroke.borrow_mut().take();
        // Each Draw session starts new drawings; strokes within it group per page.
        self.ink_groups.borrow_mut().clear();
        self.pages_box.set_cursor_from_name(Some("crosshair"));
        self.pages_box.grab_focus();
        self.queue_pages_draw();
        self.update_chrome();
    }

    pub(crate) fn cancel_draw(&self) {
        let was_drawing = self.draw_mode.replace(false);
        self.ink_stroke.borrow_mut().take();
        self.ink_groups.borrow_mut().clear();
        if !was_drawing {
            return;
        }
        if !self.text_placement.get() {
            self.pages_box.set_cursor_from_name(None);
        }
        self.queue_pages_draw();
        self.update_chrome();
    }

    pub(crate) fn begin_ink_stroke(&self, x: f64, y: f64) {
        *self.ink_stroke.borrow_mut() = self
            .hit_at(x, y)
            .map(|(page, px, py)| ActiveInk::new(page, (px, py)));
        self.queue_pages_draw();
    }

    pub(crate) fn extend_ink_stroke(&self, x: f64, y: f64) {
        if self.push_ink_point(x, y) {
            self.queue_pages_draw();
        }
    }

    /// Adds the pointer samples GTK coalesced into one motion event, so fast
    /// strokes keep their shape instead of turning into straight segments.
    pub(crate) fn extend_ink_stroke_from_event(&self, event: Option<gdk::Event>, x: f64, y: f64) {
        if let Some(event) = event.filter(|e| e.event_type() == gdk::EventType::MotionNotify) {
            if let Some((ex, ey)) = event.position() {
                let (dx, dy) = (x - ex, y - ey);
                use glib::translate::IntoGlib;
                let (ix, iy) = (
                    gdk::AxisUse::X.into_glib() as usize,
                    gdk::AxisUse::Y.into_glib() as usize,
                );
                for coord in event.history() {
                    let axes = coord.axes();
                    self.push_ink_point(axes[ix] + dx, axes[iy] + dy);
                }
            }
        }
        self.extend_ink_stroke(x, y);
    }

    fn push_ink_point(&self, x: f64, y: f64) -> bool {
        let Some((page, px, py)) = self.hit_at(x, y) else {
            return false;
        };
        let scale = {
            let st = self.inner.borrow();
            st.view
                .layout
                .page_geom(page)
                .map(|geom| geom.scale)
                .unwrap_or(1.0)
                .max(0.01)
        };
        let mut stroke = self.ink_stroke.borrow_mut();
        let Some(active) = stroke.as_mut() else {
            return false;
        };
        if active.page != page {
            return false;
        }
        // Ignore sub-pixel jitter; it only adds wobble to the smoothed curve.
        let min = 1.2 / scale;
        if let Some(&(lx, ly)) = active.points.last() {
            if (px - lx).powi(2) + (py - ly).powi(2) < min * min {
                return false;
            }
        }
        active.push((px, py));
        true
    }

    pub(crate) fn finish_ink_stroke(&self) {
        let Some(stroke) = self.ink_stroke.borrow_mut().take() else {
            return;
        };
        let style = self.pen.get().ink_style();
        let group = self
            .ink_groups
            .borrow()
            .get(&stroke.page)
            .filter(|(group_style, _)| *group_style == style)
            .map(|(_, annot)| annot.clone());
        let result = {
            let st = self.inner.borrow();
            let Some(doc) = st.view.doc.as_ref() else {
                return;
            };
            let points = stroke
                .points
                .iter()
                .map(|(x, y)| display_point_to_document(doc, stroke.page, st.view.rotation, *x, *y))
                .collect::<Vec<_>>();
            match group {
                Some(annot) => doc
                    .append_ink_stroke(stroke.page, &annot, &points)
                    .map(|_| annot),
                None => doc.add_ink_drawing(stroke.page, &points, style),
            }
        };
        match result {
            Ok(annot) => {
                self.ink_groups
                    .borrow_mut()
                    .insert(stroke.page, (style, annot.clone()));
                self.push_undo(EditUndo::Stroke {
                    page: stroke.page,
                    annot,
                });
                self.pdf_changed(stroke.page);
            }
            Err(error) if error == "Stroke is too short" => self.queue_pages_draw(),
            Err(error) => {
                self.queue_pages_draw();
                self.alert("Could not add drawing", &error);
            }
        }
    }
}
