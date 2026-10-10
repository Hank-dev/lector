use super::*;
use crate::doc::{FreeTextSpec, InkStyle};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PenColor {
    Black,
    Blue,
    Red,
}

impl PenColor {
    pub(crate) const ALL: [PenColor; 3] = [PenColor::Black, PenColor::Blue, PenColor::Red];

    pub(crate) fn rgb(self) -> (f64, f64, f64) {
        match self {
            PenColor::Black => crate::doc::INK_BLACK,
            PenColor::Blue => (0.09, 0.26, 0.66),
            PenColor::Red => (0.78, 0.12, 0.12),
        }
    }

    fn name(self) -> &'static str {
        match self {
            PenColor::Black => "Black",
            PenColor::Blue => "Blue",
            PenColor::Red => "Red",
        }
    }

    fn css(self) -> &'static str {
        match self {
            PenColor::Black => "black",
            PenColor::Blue => "blue",
            PenColor::Red => "red",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PenWidth {
    Fine,
    Medium,
    Bold,
}

impl PenWidth {
    const ALL: [PenWidth; 3] = [PenWidth::Fine, PenWidth::Medium, PenWidth::Bold];

    pub(crate) fn points(self) -> f64 {
        match self {
            PenWidth::Fine => 0.9,
            PenWidth::Medium => crate::doc::INK_STROKE_WIDTH,
            PenWidth::Bold => 2.8,
        }
    }

    fn name(self) -> &'static str {
        match self {
            PenWidth::Fine => "Fine",
            PenWidth::Medium => "Medium",
            PenWidth::Bold => "Bold",
        }
    }
}

const TEXT_SIZES: [f64; 12] = [
    8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 28.0, 32.0,
];

/// Shared by Draw and Add Text: one colour, a stroke width and a text size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Pen {
    pub(crate) color: PenColor,
    pub(crate) width: PenWidth,
    pub(crate) text_size: f64,
}

impl Default for Pen {
    fn default() -> Self {
        Self {
            color: PenColor::Black,
            width: PenWidth::Medium,
            text_size: crate::doc::FREE_TEXT_SIZE,
        }
    }
}

impl Pen {
    pub(crate) fn ink_style(self) -> InkStyle {
        InkStyle {
            rgb: self.color.rgb(),
            width: self.width.points(),
        }
    }
}

fn step_text_size(size: f64, larger: bool) -> f64 {
    if larger {
        TEXT_SIZES
            .iter()
            .copied()
            .find(|s| *s > size + 0.01)
            .unwrap_or(TEXT_SIZES[TEXT_SIZES.len() - 1])
    } else {
        TEXT_SIZES
            .iter()
            .rev()
            .copied()
            .find(|s| *s < size - 0.01)
            .unwrap_or(TEXT_SIZES[0])
    }
}

/// One step of Ctrl+Z. Text is identified by content because editing
/// re-creates Poppler objects; a drawing keeps its object while strokes are
/// added. Removing from the manager dialog clears the stack.
pub(crate) enum EditUndo {
    Stroke {
        page: i32,
        annot: poppler::AnnotInk,
    },
    Text {
        page: i32,
        added: Option<FreeTextSpec>,
        replaced: Option<FreeTextSpec>,
    },
}

/// Bar under the toolbar while drawing or adding text.
pub(crate) struct EditBar {
    pub(crate) revealer: Revealer,
    pub(crate) title: Label,
    pub(crate) hint: Label,
    pub(crate) colors: Vec<(PenColor, ToggleButton)>,
    pub(crate) widths_box: GtkBox,
    pub(crate) widths: Vec<(PenWidth, ToggleButton)>,
    pub(crate) sizes_box: GtkBox,
    pub(crate) size_label: Label,
    pub(crate) smaller: Button,
    pub(crate) larger: Button,
    pub(crate) undo: Button,
    pub(crate) done: Button,
}

fn bar_button(button: &impl IsA<gtk::Widget>) {
    // Keep keyboard focus on the page so typing and Esc keep working.
    button.set_focus_on_click(false);
    button.set_can_focus(false);
    button.set_valign(gtk::Align::Center);
}

impl EditBar {
    pub(crate) fn new() -> Self {
        let title = Label::new(None);
        title.add_css_class("lector-editbar-title");
        let hint = Label::new(None);
        hint.add_css_class("dim-label");
        hint.set_hexpand(true);
        hint.set_xalign(0.0);
        hint.set_ellipsize(gtk::pango::EllipsizeMode::End);

        let colors_box = GtkBox::new(Orientation::Horizontal, 2);
        let mut colors = Vec::new();
        for color in PenColor::ALL {
            let dot = GtkBox::new(Orientation::Horizontal, 0);
            dot.add_css_class("lector-swatch-dot");
            dot.add_css_class(color.css());
            let button = ToggleButton::new();
            button.set_child(Some(&dot));
            button.add_css_class("flat");
            button.add_css_class("lector-swatch");
            button.set_tooltip_text(Some(color.name()));
            if let Some((_, first)) = colors.first() {
                button.set_group(Some(first));
            }
            bar_button(&button);
            colors_box.append(&button);
            colors.push((color, button));
        }

        let widths_box = GtkBox::new(Orientation::Horizontal, 2);
        let mut widths: Vec<(PenWidth, ToggleButton)> = Vec::new();
        for width in PenWidth::ALL {
            let button = ToggleButton::with_label(width.name());
            button.add_css_class("flat");
            if let Some((_, first)) = widths.first() {
                button.set_group(Some(first));
            }
            bar_button(&button);
            widths_box.append(&button);
            widths.push((width, button));
        }

        let sizes_box = GtkBox::new(Orientation::Horizontal, 2);
        let smaller = Button::with_label("A−");
        smaller.add_css_class("flat");
        smaller.set_tooltip_text(Some("Smaller text"));
        let size_label = Label::new(None);
        size_label.set_width_chars(5);
        let larger = Button::with_label("A+");
        larger.add_css_class("flat");
        larger.set_tooltip_text(Some("Larger text"));
        bar_button(&smaller);
        bar_button(&larger);
        sizes_box.append(&smaller);
        sizes_box.append(&size_label);
        sizes_box.append(&larger);

        let undo = Button::from_icon_name("edit-undo-symbolic");
        undo.add_css_class("flat");
        undo.set_tooltip_text(Some("Undo (Ctrl+Z)"));
        let done = Button::with_label("Done");
        done.set_tooltip_text(Some("Done (Esc)"));
        bar_button(&undo);
        bar_button(&done);

        let row = GtkBox::new(Orientation::Horizontal, 8);
        row.add_css_class("lector-editbar");
        row.append(&title);
        row.append(&hint);
        row.append(&colors_box);
        row.append(&widths_box);
        row.append(&sizes_box);
        row.append(&undo);
        row.append(&done);
        let revealer = Revealer::new();
        revealer.set_child(Some(&row));
        Self {
            revealer,
            title,
            hint,
            colors,
            widths_box,
            widths,
            sizes_box,
            size_label,
            smaller,
            larger,
            undo,
            done,
        }
    }
}

impl Lector {
    pub(crate) fn connect_edit_bar(&self) {
        let bar = &self.edit_bar;
        for (color, button) in &bar.colors {
            let h = Handle(self.me.borrow().clone());
            let color = *color;
            button.connect_toggled(move |button| {
                if !button.is_active() {
                    return;
                }
                if let Some(ui) = h.get() {
                    ui.set_pen_color(color);
                }
            });
        }
        for (width, button) in &bar.widths {
            let h = Handle(self.me.borrow().clone());
            let width = *width;
            button.connect_toggled(move |button| {
                if !button.is_active() {
                    return;
                }
                if let Some(ui) = h.get() {
                    let mut pen = ui.pen.get();
                    pen.width = width;
                    ui.pen.set(pen);
                    ui.update_chrome();
                }
            });
        }
        for (button, larger) in [(&bar.smaller, false), (&bar.larger, true)] {
            let h = Handle(self.me.borrow().clone());
            button.connect_clicked(move |_| {
                if let Some(ui) = h.get() {
                    ui.step_text_size(larger);
                }
            });
        }
        let h = Handle(self.me.borrow().clone());
        bar.undo.connect_clicked(move |_| {
            if let Some(ui) = h.get() {
                ui.undo_edit();
            }
        });
        let h = Handle(self.me.borrow().clone());
        bar.done.connect_clicked(move |_| {
            if let Some(ui) = h.get() {
                ui.finish_edit_mode();
            }
        });
    }

    pub(crate) fn sync_edit_bar(&self) {
        let bar = &self.edit_bar;
        let drawing = self.draw_mode.get();
        let texting = self.text_placement.get();
        bar.revealer.set_reveal_child(drawing || texting);
        if !(drawing || texting) {
            return;
        }
        let drafting = self.text_draft.borrow().is_some();
        let pen = self.pen.get();
        bar.title
            .set_text(if drawing { "Draw" } else { "Add Text" });
        bar.hint.set_text(if drawing {
            "Drag to write. Strokes on a page form one drawing."
        } else if drafting {
            "Enter adds · Shift+Enter new line · drag the box to move"
        } else {
            "Click to type · click added text to edit, drag it to move"
        });
        for (color, button) in &bar.colors {
            if *color == pen.color && !button.is_active() {
                button.set_active(true);
            }
        }
        for (width, button) in &bar.widths {
            if *width == pen.width && !button.is_active() {
                button.set_active(true);
            }
        }
        bar.widths_box.set_visible(drawing);
        bar.sizes_box.set_visible(texting);
        let size = self
            .text_draft
            .borrow()
            .as_ref()
            .map(|draft| draft.size)
            .unwrap_or(pen.text_size);
        bar.size_label.set_text(&format!("{} pt", size.round()));
        bar.undo
            .set_sensitive(drafting || !self.edit_undo.borrow().is_empty());
    }

    fn set_pen_color(&self, color: PenColor) {
        let mut pen = self.pen.get();
        pen.color = color;
        self.pen.set(pen);
        if let Some(draft) = self.text_draft.borrow_mut().as_mut() {
            draft.rgb = color.rgb();
        }
        self.queue_pages_draw();
        self.update_chrome();
    }

    fn step_text_size(&self, larger: bool) {
        let mut pen = self.pen.get();
        let current = self
            .text_draft
            .borrow()
            .as_ref()
            .map(|draft| draft.size)
            .unwrap_or(pen.text_size);
        pen.text_size = step_text_size(current, larger);
        self.pen.set(pen);
        if let Some(draft) = self.text_draft.borrow_mut().as_mut() {
            draft.size = pen.text_size;
        }
        self.queue_pages_draw();
        self.update_chrome();
    }

    /// Esc / Done: keep what was written and leave the current tool.
    pub(crate) fn finish_edit_mode(&self) {
        if self.draw_mode.get() {
            self.cancel_draw();
        }
        if self.text_placement.get() {
            self.cancel_text_placement();
        }
    }

    pub(crate) fn push_undo(&self, step: EditUndo) {
        self.edit_undo.borrow_mut().push(step);
    }

    /// Forget grouping and undo history, e.g. when annotations change
    /// outside the tools or another document is shown.
    pub(crate) fn reset_edit_history(&self) {
        self.edit_undo.borrow_mut().clear();
        self.ink_groups.borrow_mut().clear();
    }

    pub(crate) fn undo_edit(&self) {
        if self.text_draft.borrow().is_some() {
            self.discard_text_draft();
            return;
        }
        let Some(step) = self.edit_undo.borrow_mut().pop() else {
            return;
        };
        let page = match &step {
            EditUndo::Stroke { page, .. } | EditUndo::Text { page, .. } => *page,
        };
        let result = {
            let st = self.inner.borrow();
            let Some(doc) = st.view.doc.as_ref() else {
                return;
            };
            match &step {
                EditUndo::Stroke { page, annot } => {
                    doc.pop_ink_stroke(*page, annot).map(|removed| {
                        if removed {
                            self.ink_groups
                                .borrow_mut()
                                .retain(|_, (_, group)| group != annot);
                        }
                    })
                }
                EditUndo::Text {
                    page,
                    added,
                    replaced,
                } => added
                    .as_ref()
                    .map(|spec| {
                        doc.find_free_text(*page, spec)
                            .ok_or_else(|| "The text is no longer in the PDF".to_string())
                            .and_then(|annot| doc.remove_fill_text(*page, &annot))
                    })
                    .unwrap_or(Ok(()))
                    .and_then(|_| match replaced {
                        Some(spec) => doc.add_free_text(*page, spec).map(|_| ()),
                        None => Ok(()),
                    }),
            }
        };
        match result {
            Ok(()) => self.pdf_changed(page),
            Err(error) => self.alert("Could not undo", &error),
        }
    }

    pub(crate) fn present_edits_manager(&self) {
        if let Some(dialog) = self.edits_manager_dialog() {
            dialog.present();
        }
    }

    /// Lists added text and drawings with Go to and Remove.
    pub(crate) fn edits_manager_dialog(&self) -> Option<gtk::Window> {
        use poppler::prelude::*;
        self.commit_text_draft();
        let st = self.inner.borrow();
        let doc = st.view.doc.as_ref()?;
        let document = doc.document.clone();
        let mut entries = doc
            .free_text_annotations()
            .into_iter()
            .map(|(page, annot)| {
                let contents = annot.contents().unwrap_or_default().to_string();
                let preview = contents.split_whitespace().collect::<Vec<_>>().join(" ");
                (page, 0, format!("Text — {preview}"), contents, annot)
            })
            .collect::<Vec<_>>();
        entries.extend(doc.ink_annotations().into_iter().map(|(page, annot)| {
            let strokes = OpenDoc::ink_stroke_count(&annot);
            let label = format!(
                "Drawing — {strokes} stroke{}",
                if strokes == 1 { "" } else { "s" }
            );
            (page, 1, label, String::new(), annot)
        }));
        entries.sort_by_key(|(page, kind, ..)| (*page, *kind));
        let labels = (0..doc.n_pages)
            .map(|page| display_page_label(&doc.labels, page))
            .collect::<Vec<_>>();
        drop(st);

        let body = GtkBox::new(Orientation::Vertical, 6);
        let guidance = Label::new(Some(
            "Added text and drawings are embedded when you Save a Copy (Ctrl+S). The original file is not changed.",
        ));
        guidance.set_wrap(true);
        guidance.set_xalign(0.0);
        body.append(&guidance);
        if entries.is_empty() {
            body.append(&Label::new(Some("No added text or drawings in this PDF.")));
        }
        for (page, _, label, contents, annot) in entries {
            let row = GtkBox::new(Orientation::Horizontal, 8);
            let page_label = Label::new(Some(&format!(
                "p. {}",
                labels.get(page as usize).cloned().unwrap_or_default()
            )));
            page_label.add_css_class("dim-label");
            page_label.set_width_chars(6);
            page_label.set_xalign(0.0);
            row.append(&page_label);
            let text = Label::new(Some(&label));
            text.set_xalign(0.0);
            text.set_hexpand(true);
            text.set_ellipsize(gtk::pango::EllipsizeMode::End);
            if !contents.is_empty() {
                text.set_tooltip_text(Some(&contents));
            }
            row.append(&text);
            let go = Button::with_label("Go to");
            go.add_css_class("flat");
            row.append(&go);
            let remove = Button::with_label("Remove");
            if !contents.is_empty() {
                remove.set_tooltip_text(Some(&contents));
            }
            row.append(&remove);
            body.append(&row);
            let h = Handle(self.me.borrow().clone());
            go.connect_clicked(move |_| {
                if let Some(ui) = h.get() {
                    ui.goto_page(page);
                }
            });
            let h = Handle(self.me.borrow().clone());
            let document = document.clone();
            remove.connect_clicked(move |_| {
                let Some(ui) = h.get() else {
                    return;
                };
                let result = {
                    let st = ui.inner.borrow();
                    st.view
                        .doc
                        .as_ref()
                        .filter(|doc| doc.document == document)
                        .ok_or_else(|| "Document changed".to_string())
                        .and_then(|doc| match annot.annot_type() {
                            poppler::AnnotType::Ink => doc.remove_ink(page, &annot),
                            _ => doc.remove_fill_text(page, &annot),
                        })
                };
                match result {
                    Ok(()) => {
                        ui.reset_edit_history();
                        ui.pdf_changed(page);
                        row.set_visible(false);
                    }
                    Err(error) => ui.alert("Could not remove", &error),
                }
            });
        }
        let scroll = ScrolledWindow::new();
        scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        scroll.set_max_content_height(480);
        scroll.set_min_content_width(420);
        scroll.set_propagate_natural_height(true);
        scroll.set_child(Some(&body));
        Some(self.themed_dialog("Added Text & Drawings", &scroll))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_size_steps_clamp_to_the_scale() {
        assert_eq!(step_text_size(12.0, true), 14.0);
        assert_eq!(step_text_size(12.0, false), 11.0);
        assert_eq!(step_text_size(13.0, false), 12.0);
        assert_eq!(step_text_size(32.0, true), 32.0);
        assert_eq!(step_text_size(8.0, false), 8.0);
    }
}
