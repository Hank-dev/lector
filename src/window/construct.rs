use super::*;

impl Lector {
    pub fn new(app: &Application) -> Rc<Self> {
        let theme = Theme::load();
        let css = CssProvider::new();
        theme::install_provider(&css);
        theme::apply_css(&css, &theme);

        let window = ApplicationWindow::builder()
            .application(app)
            .title("")
            .default_width(1100)
            .default_height(860)
            .build();
        window.add_css_class("lector-window");
        window.set_titlebar(None::<&gtk::Box>);

        let (annotations, annotation_error) = match AnnotationStore::load() {
            Ok(store) => (Some(store), None),
            Err(error) => (None, Some(error)),
        };

        let inner = Rc::new(RefCell::new(Inner {
            view: ViewState::new(),
            search: SearchState::new(),
            pane: PaneState::new(),
            presentation: false,
            toolbar_hidden: false,
            theme,
            session: Session::load(),
            annotations,
            annotation_error,
            ignore_zoom_dropdown: false,
            ignore_scroll: false,
            ignore_menu_checks: false,
            layout_gen: 0,
            thumb_gen: 0,
            pres_restore: None,
            raster: RasterCache::new(12),
            raster_gen: 0,
            navigation: NavigationHistory::default(),
        }));
        {
            let mut st = inner.borrow_mut();
            st.view.pdf_mode = PdfMode::parse(&st.session.pdf_mode, st.session.invert);
            st.view.layout_mode = session::parse_layout(&st.session.layout);
            st.view.dual_cover = st.session.dual_cover;
            st.pane.sidebar = parse_sidebar(&st.session.sidebar);
        }

        // Slim Omarchy-native bar: one flat surface, no boxed clusters.
        let title_label = Label::new(Some("Open a document"));
        title_label.add_css_class("lector-doc-title");
        title_label.set_hexpand(true);
        title_label.set_ellipsize(gtk::pango::EllipsizeMode::End);

        let sidebar_btn = toggle_pill("view-list-symbolic", "Side pane (F9)");
        let open_btn = icon_pill("document-open-symbolic", "Open (Ctrl+O)");
        let back_btn = icon_pill("edit-undo-symbolic", "Back (Alt+Left)");
        let forward_btn = icon_pill("edit-redo-symbolic", "Forward (Alt+Right)");
        let prev_btn = icon_pill("go-previous-symbolic", "Previous page (p)");
        let next_btn = icon_pill("go-next-symbolic", "Next page (n)");

        let page_entry = Entry::new();
        page_entry.set_width_chars(4);
        page_entry.set_max_width_chars(8);
        page_entry.set_placeholder_text(Some("1"));
        page_entry.set_tooltip_text(Some("Go to page"));
        page_entry.add_css_class("lector-page-entry");
        page_entry.set_valign(gtk::Align::Center);
        let page_total = Label::new(Some("/ 0"));
        page_total.add_css_class("dim-label");
        page_total.set_valign(gtk::Align::Center);

        let zoom_out_btn = icon_pill("zoom-out-symbolic", "Zoom out (Ctrl+-)");
        let zoom_in_btn = icon_pill("zoom-in-symbolic", "Zoom in (Ctrl++)");
        let zoom_drop = DropDown::from_strings(&ZOOM_LABELS);
        zoom_drop.set_tooltip_text(Some("Zoom"));
        zoom_drop.set_selected(2);
        zoom_drop.set_valign(gtk::Align::Center);
        zoom_drop.add_css_class("lector-zoom");

        let find_btn = icon_pill("edit-find-symbolic", "Find (Ctrl+F)");

        let menu_btn = MenuButton::new();
        menu_btn.set_icon_name("open-menu-symbolic");
        menu_btn.set_tooltip_text(Some("More"));
        menu_btn.add_css_class("lector-pill");

        let cont_item = menu_check_row("Continuous", "c");
        let dual_item = menu_check_row("Dual", "d");
        let cover_item = menu_check_row("Separate cover page", "");
        let single_item = menu_check_row("Single page", "s");
        dual_item.set_group(Some(&cont_item));
        single_item.set_group(Some(&cont_item));
        let normal_item = menu_check_row("Normal colors", "");
        let omarchy_item = menu_check_row("Follow Omarchy", "");
        let invert_item = menu_check_row("Inverted colors", "Ctrl+I");
        omarchy_item.set_group(Some(&normal_item));
        invert_item.set_group(Some(&normal_item));
        let highlight_item = bare_menu_row(
            "format-text-highlight-symbolic",
            "Highlight selection",
            "Ctrl+Shift+H",
        );
        let note_item = bare_menu_row(
            "document-edit-symbolic",
            "Add note to selection…",
            "Ctrl+Shift+N",
        );
        let delete_annotation_item = bare_menu_row(
            "edit-delete-symbolic",
            "Delete targeted annotation",
            "Delete",
        );
        highlight_item.set_sensitive(false);
        note_item.set_sensitive(false);
        delete_annotation_item.set_sensitive(false);
        {
            let st = inner.borrow();
            cont_item.set_active(st.view.layout_mode == LayoutMode::Continuous);
            dual_item.set_active(st.view.layout_mode == LayoutMode::Dual);
            cover_item.set_active(st.view.dual_cover);
            cover_item.set_sensitive(st.view.layout_mode == LayoutMode::Dual);
            single_item.set_active(st.view.layout_mode == LayoutMode::Single);
            normal_item.set_active(st.view.pdf_mode == PdfMode::Normal);
            omarchy_item.set_active(st.view.pdf_mode == PdfMode::Omarchy);
            invert_item.set_active(st.view.pdf_mode == PdfMode::Inverted);
        }

        back_btn.set_margin_start(8);
        prev_btn.set_margin_start(8);
        zoom_out_btn.set_margin_start(8);
        find_btn.set_margin_start(8);

        let toolbar = GtkBox::new(Orientation::Horizontal, 2);
        toolbar.add_css_class("lector-toolbar");
        toolbar.set_valign(gtk::Align::Center);
        toolbar.append(&sidebar_btn);
        toolbar.append(&open_btn);
        toolbar.append(&back_btn);
        toolbar.append(&forward_btn);
        toolbar.append(&prev_btn);
        toolbar.append(&page_entry);
        toolbar.append(&page_total);
        toolbar.append(&next_btn);
        toolbar.append(&title_label);
        toolbar.append(&zoom_out_btn);
        toolbar.append(&zoom_drop);
        toolbar.append(&zoom_in_btn);
        toolbar.append(&find_btn);
        toolbar.append(&menu_btn);

        // --- find bar (reveals under header) ---
        let search_entry = Entry::new();
        search_entry.set_placeholder_text(Some("Find"));
        search_entry.set_hexpand(true);
        let search_info = Label::new(None);
        search_info.add_css_class("dim-label");
        let find_prev = Button::from_icon_name("go-up-symbolic");
        find_prev.add_css_class("flat");
        find_prev.set_tooltip_text(Some("Previous match"));
        let find_next = Button::from_icon_name("go-down-symbolic");
        find_next.add_css_class("flat");
        find_next.set_tooltip_text(Some("Next match"));
        let find_close = Button::from_icon_name("window-close-symbolic");
        find_close.add_css_class("flat");
        let case_btn = ToggleButton::with_label("Aa");
        case_btn.add_css_class("flat");
        case_btn.set_tooltip_text(Some("Match case"));
        let word_btn = ToggleButton::with_label("W");
        word_btn.add_css_class("flat");
        word_btn.set_tooltip_text(Some("Whole words"));
        let find_row = GtkBox::new(Orientation::Horizontal, 8);
        find_row.append(&Label::new(Some("Find:")));
        find_row.append(&search_entry);
        find_row.append(&case_btn);
        find_row.append(&word_btn);
        find_row.append(&search_info);
        find_row.append(&find_prev);
        find_row.append(&find_next);
        find_row.append(&find_close);
        let search_results = ListBox::new();
        search_results.add_css_class("lector-search-results");
        search_results.set_selection_mode(gtk::SelectionMode::Single);
        let search_results_sw = ScrolledWindow::new();
        search_results_sw.set_child(Some(&search_results));
        search_results_sw.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        search_results_sw.set_max_content_height(148);
        search_results_sw.set_propagate_natural_height(true);
        search_results_sw.set_visible(false);
        let find_col = GtkBox::new(Orientation::Vertical, 4);
        find_col.add_css_class("lector-find");
        find_col.append(&find_row);
        find_col.append(&search_results_sw);
        let find_bar = Revealer::new();
        find_bar.set_child(Some(&find_col));

        let file_banner_label = Label::new(Some("This file has changed on disk."));
        file_banner_label.set_hexpand(true);
        file_banner_label.set_xalign(0.0);
        file_banner_label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        let file_reload_btn = Button::with_label("Reload");
        file_reload_btn.add_css_class("lector-pill");
        let file_dismiss_btn = Button::from_icon_name("window-close-symbolic");
        file_dismiss_btn.add_css_class("flat");
        file_dismiss_btn.set_tooltip_text(Some("Dismiss"));
        let file_row = GtkBox::new(Orientation::Horizontal, 8);
        file_row.add_css_class("lector-file-banner");
        file_row.append(&file_banner_label);
        file_row.append(&file_reload_btn);
        file_row.append(&file_dismiss_btn);
        let file_banner = Revealer::new();
        file_banner.set_child(Some(&file_row));

        // --- side pane ---
        let thumbs = ListBox::new();
        thumbs.add_css_class("lector-thumbs");
        thumbs.set_selection_mode(gtk::SelectionMode::Single);
        let outline = ListBox::new();
        outline.add_css_class("lector-outline");
        outline.set_selection_mode(gtk::SelectionMode::Single);
        let marks = ListBox::new();
        marks.add_css_class("lector-outline");
        marks.set_selection_mode(gtk::SelectionMode::Single);

        let thumbs_sw = ScrolledWindow::new();
        thumbs_sw.set_child(Some(&thumbs));
        thumbs_sw.set_vexpand(true);
        let outline_sw = ScrolledWindow::new();
        outline_sw.set_child(Some(&outline));
        outline_sw.set_vexpand(true);
        let marks_sw = ScrolledWindow::new();
        marks_sw.set_child(Some(&marks));
        marks_sw.set_vexpand(true);

        let stack = Stack::new();
        stack.set_vexpand(true);
        stack.add_titled(&outline_sw, Some("outline"), "Outline");
        stack.add_titled(&thumbs_sw, Some("thumbs"), "Thumbnails");
        stack.add_titled(&marks_sw, Some("marks"), "Marks");
        let switcher = StackSwitcher::new();
        switcher.set_stack(Some(&stack));
        switcher.set_hexpand(true);
        switcher.add_css_class("lector-side-switcher");

        let side = GtkBox::new(Orientation::Vertical, 0);
        side.add_css_class("lector-sidebar");
        side.set_size_request(220, -1);
        side.set_hexpand(false);
        side.set_hexpand_set(true);
        side.append(&stack);
        side.append(&switcher);

        let sidebar_revealer = Revealer::new();
        sidebar_revealer.set_transition_type(gtk::RevealerTransitionType::SlideRight);
        sidebar_revealer.set_hexpand(false);
        sidebar_revealer.set_child(Some(&side));

        // --- canvas: one drawing area per visible page on a Fixed whose
        // size is the full layout. GTK/GSK cannot composite a 30k-px-tall
        // surface; we also must not instantiate a widget per page of a
        // 2k-page manual.
        let pages_box = Fixed::new();
        pages_box.add_css_class("lector-canvas");
        pages_box.set_hexpand(false);
        pages_box.set_vexpand(false);
        pages_box.set_can_focus(true);
        pages_box.set_halign(gtk::Align::Center);
        pages_box.set_valign(gtk::Align::Start);

        let scrolled = ScrolledWindow::new();
        scrolled.set_child(Some(&pages_box));
        scrolled.set_hexpand(true);
        scrolled.set_vexpand(true);
        scrolled.set_kinetic_scrolling(true);
        scrolled.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);

        let body = GtkBox::new(Orientation::Horizontal, 0);
        body.set_vexpand(true);
        body.append(&sidebar_revealer);
        body.append(&scrolled);

        let edit_bar = EditBar::new();
        let keys = gtk::EventControllerKey::new();
        let im = gtk::IMMulticontext::new();
        im.set_client_widget(Some(&pages_box));

        let root = GtkBox::new(Orientation::Vertical, 0);
        root.append(&toolbar);
        root.append(&edit_bar.revealer);
        root.append(&find_bar);
        root.append(&file_banner);
        root.append(&body);
        window.set_child(Some(&root));

        let ui = Rc::new(Self {
            window: window.clone(),
            pdf_dirty: Cell::new(false),
            text_placement: Cell::new(false),
            draw_mode: Cell::new(false),
            ink_stroke: RefCell::new(None),
            text_draft: RefCell::new(None),
            text_press: RefCell::new(None),
            pen: Cell::new(Pen::default()),
            ink_groups: RefCell::new(HashMap::new()),
            edit_undo: RefCell::new(Vec::new()),
            edit_bar,
            im: im.clone(),
            keys: keys.clone(),
            pages_box: pages_box.clone(),
            scrolled: scrolled.clone(),
            toolbar: toolbar.clone(),
            page_entry: page_entry.clone(),
            page_total: page_total.clone(),
            search_entry: search_entry.clone(),
            search_info: search_info.clone(),
            search_results: search_results.clone(),
            search_results_sw: search_results_sw.clone(),
            case_btn: case_btn.clone(),
            word_btn: word_btn.clone(),
            find_bar: find_bar.clone(),
            file_banner: file_banner.clone(),
            file_banner_label: file_banner_label.clone(),
            file_reload_btn: file_reload_btn.clone(),
            sidebar_revealer: sidebar_revealer.clone(),
            sidebar_btn: sidebar_btn.clone(),
            stack: stack.clone(),
            thumbs: thumbs.clone(),
            outline: outline.clone(),
            marks: marks.clone(),
            zoom_drop: zoom_drop.clone(),
            title_label: title_label.clone(),
            back_btn: back_btn.clone(),
            forward_btn: forward_btn.clone(),
            prev_btn: prev_btn.clone(),
            next_btn: next_btn.clone(),
            zoom_out_btn: zoom_out_btn.clone(),
            zoom_in_btn: zoom_in_btn.clone(),
            find_btn: find_btn.clone(),
            cont_item: cont_item.clone(),
            dual_item: dual_item.clone(),
            cover_item: cover_item.clone(),
            single_item: single_item.clone(),
            normal_item: normal_item.clone(),
            omarchy_item: omarchy_item.clone(),
            invert_item: invert_item.clone(),
            highlight_item: highlight_item.clone(),
            note_item: note_item.clone(),
            delete_annotation_item: delete_annotation_item.clone(),
            document_menu_items: RefCell::new(Vec::new()),
            thumbs_sw: thumbs_sw.clone(),
            page_widgets: RefCell::new(Vec::new()),
            rendered_structure: RefCell::new(None),
            raster_pending: RefCell::new(HashSet::new()),
            sidebar_ready: Cell::new(0),
            raster_busy: Cell::new(false),
            pinch_active: Cell::new(false),
            inner,
            css,
            me: RefCell::new(Weak::new()),
            hold: RefCell::new(None),
            _monitor: RefCell::new(None),
            doc_monitor: RefCell::new(None),
            search_debounce_gen: Cell::new(0),
            file_change_gen: Cell::new(0),
            ignore_search_changed: Cell::new(false),
            ignore_search_results: Cell::new(false),
            ignore_marks: Cell::new(false),
            pending_disk_change: Cell::new(None),
            file_watch_ignore_until: Cell::new(None),
        });
        *ui.me.borrow_mut() = Rc::downgrade(&ui);
        let h = Handle(Rc::downgrade(&ui));

        let menu_pop = Popover::new();
        menu_pop.add_css_class("lector-menu");
        menu_pop.set_has_arrow(false);
        menu_pop.set_position(gtk::PositionType::Bottom);
        menu_pop.set_offset(0, 6);
        menu_pop.set_halign(gtk::Align::End);
        menu_pop.connect_realize(|p| {
            p.remove_css_class("background");
        });
        menu_pop.connect_show({
            let btn = menu_btn.clone();
            move |p| {
                p.remove_css_class("background");
                let w = btn.width().max(1);
                let h = btn.height().max(1);
                p.set_pointing_to(Some(&gdk::Rectangle::new(w - 2, 0, 2, h)));
            }
        });
        let menu_col = GtkBox::new(Orientation::Vertical, 1);
        menu_col.add_css_class("lector-menu-card");
        append_menu_section(
            &menu_col,
            "FILE",
            &[
                ("document-open-symbolic", "Open…", "Ctrl+O", menu_open),
                (
                    "document-save-as-symbolic",
                    "Save a Copy…",
                    "Ctrl+S",
                    menu_save,
                ),
                ("document-print-symbolic", "Print…", "Ctrl+P", menu_print),
                (
                    "insert-text-symbolic",
                    "Add Text…",
                    "Ctrl+Shift+T",
                    Lector::add_text_dialog,
                ),
                (
                    "input-tablet-symbolic",
                    "Draw…",
                    "Ctrl+Shift+D",
                    Lector::toggle_draw,
                ),
                (
                    "view-list-symbolic",
                    "Added Text & Drawings…",
                    "",
                    Lector::present_edits_manager,
                ),
                (
                    "document-edit-symbolic",
                    "Fill Form…",
                    "Ctrl+Shift+F",
                    |ui| {
                        if let Some(dialog) = ui.forms_dialog() {
                            dialog.present();
                        }
                    },
                ),
            ],
            &menu_pop,
            &h,
        );
        menu_col.append(&menu_sep());
        menu_col.append(&menu_heading("ANNOTATIONS"));
        for (button, action) in [
            (&highlight_item, menu_highlight as fn(&Lector)),
            (&note_item, menu_note),
            (&delete_annotation_item, menu_delete_annotation),
        ] {
            connect_menu_row(button, &menu_pop, &h, action);
            menu_col.append(button);
        }
        menu_col.append(&menu_sep());
        menu_col.append(&menu_heading("VIEW"));
        menu_col.append(&menu_row(
            "sidebar-show-symbolic",
            "Side pane",
            "F9",
            &menu_pop,
            &h,
            menu_sidebar,
        ));
        menu_col.append(&cont_item);
        menu_col.append(&dual_item);
        menu_col.append(&cover_item);
        menu_col.append(&single_item);
        menu_col.append(&menu_sep());
        menu_col.append(&menu_heading("PDF COLORS"));
        menu_col.append(&normal_item);
        menu_col.append(&omarchy_item);
        menu_col.append(&invert_item);
        for (icon, label, shortcut, action) in [
            (
                "object-rotate-left-symbolic",
                "Rotate left",
                "Ctrl+L",
                menu_rot_l as fn(&Lector),
            ),
            (
                "object-rotate-right-symbolic",
                "Rotate right",
                "Ctrl+R",
                menu_rot_r,
            ),
            ("view-fullscreen-symbolic", "Fullscreen", "F11", menu_fs),
            ("video-display-symbolic", "Presentation", "F5", menu_pres),
        ] {
            menu_col.append(&menu_row(icon, label, shortcut, &menu_pop, &h, action));
        }
        menu_col.append(&menu_sep());
        append_menu_section(
            &menu_col,
            "DOCUMENT",
            &[
                (
                    "view-refresh-symbolic",
                    "Reload",
                    "Ctrl+Shift+R",
                    menu_reload,
                ),
                (
                    "document-open-recent-symbolic",
                    "Reading History…",
                    "Ctrl+H",
                    menu_history,
                ),
                ("document-properties-symbolic", "Properties", "", menu_props),
                (
                    "input-keyboard-symbolic",
                    "Keyboard shortcuts",
                    "",
                    menu_keys,
                ),
                ("help-about-symbolic", "About Lector", "", menu_about),
            ],
            &menu_pop,
            &h,
        );
        menu_col.set_overflow(gtk::Overflow::Hidden);
        let menu_scroll = ScrolledWindow::new();
        menu_scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
        menu_scroll.set_max_content_height(540);
        menu_scroll.set_propagate_natural_height(true);
        menu_scroll.set_child(Some(&menu_col));
        menu_pop.set_child(Some(&menu_scroll));
        menu_btn.set_popover(Some(&menu_pop));
        {
            let document_labels = [
                "Fill Form…",
                "Add Text…",
                "Draw…",
                "Added Text & Drawings…",
                "Save a Copy…",
                "Print…",
                "Rotate left",
                "Rotate right",
                "Presentation",
                "Reload",
                "Properties",
            ];
            let mut items = Vec::new();
            let mut child = menu_col.first_child();
            while let Some(widget) = child {
                child = widget.next_sibling();
                if let Ok(button) = widget.clone().downcast::<Button>() {
                    if button
                        .tooltip_text()
                        .as_deref()
                        .is_some_and(|label| document_labels.contains(&label))
                    {
                        items.push(widget);
                    }
                }
            }
            *ui.document_menu_items.borrow_mut() = items;
        }
        menu_pop.connect_show({
            let h = h.clone();
            move |_| {
                if let Some(ui) = h.get() {
                    ui.update_annotation_actions();
                }
            }
        });

        for (item, mode) in [
            (cont_item.clone(), LayoutMode::Continuous),
            (dual_item.clone(), LayoutMode::Dual),
            (single_item.clone(), LayoutMode::Single),
        ] {
            let h = h.clone();
            let pop = menu_pop.clone();
            item.connect_toggled(move |item| {
                let Some(ui) = h.get() else { return };
                if !item.is_active() || ui.inner.borrow().ignore_menu_checks {
                    return;
                }
                pop.popdown();
                ui.set_layout(mode);
            });
        }
        {
            let h = h.clone();
            let pop = menu_pop.clone();
            cover_item.connect_toggled(move |item| {
                let Some(ui) = h.get() else { return };
                if ui.inner.borrow().ignore_menu_checks {
                    return;
                }
                pop.popdown();
                ui.set_dual_cover(item.is_active());
            });
        }
        for (item, mode) in [
            (normal_item.clone(), PdfMode::Normal),
            (omarchy_item.clone(), PdfMode::Omarchy),
            (invert_item.clone(), PdfMode::Inverted),
        ] {
            let h = h.clone();
            let pop = menu_pop.clone();
            item.connect_toggled(move |item| {
                let Some(ui) = h.get() else { return };
                if !item.is_active() || ui.inner.borrow().ignore_menu_checks {
                    return;
                }
                pop.popdown();
                ui.set_pdf_mode(mode);
            });
        }
        if std::env::var_os("LECTOR_DEBUG_MENU").is_some() {
            let btn = menu_btn.clone();
            let pop = menu_pop.clone();
            window.connect_map(move |_| {
                let btn = btn.clone();
                let pop = pop.clone();
                glib::timeout_add_local_once(std::time::Duration::from_millis(500), move || {
                    pop.remove_css_class("background");
                    btn.set_active(true);
                    pop.popup();
                });
            });
        }

        {
            let last = std::rc::Rc::new(std::cell::Cell::new((0, 0)));
            let pending = std::rc::Rc::new(std::cell::Cell::new(false));
            let schedule = {
                let h = h.clone();
                let last = last.clone();
                let pending = pending.clone();
                let sw = scrolled.clone();
                move || {
                    if pending.get() {
                        return;
                    }
                    pending.set(true);
                    let h = h.clone();
                    let last = last.clone();
                    let pending = pending.clone();
                    let sw = sw.clone();
                    glib::timeout_add_local_once(std::time::Duration::from_millis(16), move || {
                        pending.set(false);
                        let sz = (sw.width(), sw.height());
                        if allocation_changed(last.get(), sz) {
                            last.set(sz);
                            if let Some(ui) = h.get() {
                                ui.on_resize(sz.0 as f64, sz.1 as f64);
                            }
                        }
                    });
                }
            };
            // GtkScrolledWindow has no width/height GObject properties.  A
            // tick sees its allocated size, including changes caused by the
            // sidebar and find bar, and the timeout above coalesces reflows.
            scrolled.add_tick_callback({
                let last = last.clone();
                move |sw, _| {
                    if allocation_changed(last.get(), (sw.width(), sw.height())) {
                        schedule();
                    }
                    glib::ControlFlow::Continue
                }
            });
        }
        scrolled.vadjustment().connect_value_changed({
            let h = h.clone();
            move |_| {
                if let Some(ui) = h.get() {
                    ui.on_scroll();
                }
            }
        });

        let drag = GestureDrag::new();
        drag.set_button(1);
        drag.connect_drag_begin({
            let h = h.clone();
            move |_, x, y| {
                if let Some(ui) = h.get() {
                    ui.drag_begin(x, y);
                }
            }
        });
        drag.connect_drag_update({
            let h = h.clone();
            move |g, dx, dy| {
                if let (Some(ui), Some((ox, oy))) = (h.get(), g.start_point()) {
                    if ui.draw_mode.get() {
                        ui.extend_ink_stroke_from_event(g.current_event(), ox + dx, oy + dy);
                    } else {
                        ui.drag_update(ox + dx, oy + dy);
                    }
                }
            }
        });
        drag.connect_drag_end({
            let h = h.clone();
            move |g, dx, dy| {
                if let Some(ui) = h.get() {
                    if ui.draw_mode.get() {
                        if let Some((x, y)) = g.start_point() {
                            ui.extend_ink_stroke(x + dx, y + dy);
                        }
                        ui.finish_ink_stroke();
                    } else if ui.text_placement.get() {
                        if let Some((x, y)) = g.start_point() {
                            ui.drag_text(x + dx, y + dy);
                        }
                        ui.release_text();
                    } else if let Some((x, y)) = g.start_point() {
                        ui.drag_update(x + dx, y + dy);
                    }
                }
            }
        });
        pages_box.add_controller(drag);

        let click = GestureClick::new();
        click.set_button(1);
        let first_was_link = std::rc::Rc::new(std::cell::Cell::new(false));
        click.connect_released({
            let h = h.clone();
            let first_was_link = first_was_link.clone();
            move |_, n, x, y| {
                if let Some(ui) = h.get() {
                    if ui.draw_mode.get() {
                        first_was_link.set(false);
                        return;
                    }
                    if n == 1 {
                        let selecting = ui.inner.borrow().view.selection.is_some_and(|s| {
                            (s.x2 - s.x1).abs() > 3.0 || (s.y2 - s.y1).abs() > 3.0
                        });
                        if ui.draw_mode.get() || ui.text_placement.get() || selecting {
                            first_was_link.set(false);
                            return;
                        }
                        if ui.activate_note_at(x, y) {
                            first_was_link.set(false);
                        } else {
                            let link = ui.activate_link_at(x, y);
                            if !link {
                                ui.target_annotation_at(x, y);
                            }
                            first_was_link.set(link);
                        }
                    } else {
                        let has_link = ui.link_at_view(x, y).is_some();
                        if should_toggle_fit(n, has_link, first_was_link.get()) {
                            ui.toggle_fit();
                        }
                        first_was_link.set(false);
                    }
                }
            }
        });
        pages_box.add_controller(click);

        // Capture-phase on the scrolled window so Ctrl+scroll can zoom
        // without a child controller stealing kinetic two-finger pans.
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
        scroll.set_propagation_phase(gtk::PropagationPhase::Capture);
        scroll.connect_scroll({
            let h = h.clone();
            move |c, _dx, dy| {
                let ctrl = c
                    .current_event_state()
                    .contains(gdk::ModifierType::CONTROL_MASK);
                h.get()
                    .map(|ui| ui.canvas_scroll(ctrl, dy))
                    .unwrap_or(glib::Propagation::Proceed)
            }
        });
        scrolled.add_controller(scroll);

        let pinch = GestureZoom::new();
        pinch.set_propagation_phase(gtk::PropagationPhase::Capture);
        let pinch_start = Rc::new(Cell::new(1.0));
        let pinch_anchor = Rc::new(Cell::new(None::<ViewportAnchor>));
        let pinch_latest = Rc::new(Cell::new(None::<(Zoom, ViewRestore)>));
        let pinch_frame_pending = Rc::new(Cell::new(false));
        pinch.connect_begin({
            let h = h.clone();
            let pinch_start = pinch_start.clone();
            let pinch_anchor = pinch_anchor.clone();
            let pinch_latest = pinch_latest.clone();
            move |gesture, _| {
                if let Some(ui) = h.get() {
                    pinch_latest.set(None);
                    ui.pinch_active.set(true);
                    ui.inner.borrow_mut().ignore_scroll = pinch_begin_ignore_scroll();
                    pinch_start.set(ui.inner.borrow().view.layout.scale.max(0.01));
                    let focus = gesture
                        .current_event()
                        .and_then(|_| gesture.bounding_box_center());
                    pinch_anchor.set(Some(ui.capture_anchor(focus)));
                }
            }
        });
        pinch.connect_scale_changed({
            let h = h.clone();
            let pinch_start = pinch_start.clone();
            let pinch_anchor = pinch_anchor.clone();
            let pinch_latest = pinch_latest.clone();
            let pinch_frame_pending = pinch_frame_pending.clone();
            move |_, scale| {
                if let Some(ui) = h.get() {
                    let Some(anchor) = pinch_anchor.get() else {
                        return;
                    };
                    if !scale.is_finite() || scale <= 0.0 {
                        return;
                    }
                    let (zoom, restore) = pinch_zoom_update(pinch_start.get(), scale, anchor);
                    pinch_latest.set(Some((zoom, restore)));
                    // Keep only the latest target and apply once per display frame.
                    // A scale threshold makes slow pinches visibly staircase instead.
                    if !pinch_frame_pending.replace(true) {
                        let h = h.clone();
                        let latest = pinch_latest.clone();
                        let pending = pinch_frame_pending.clone();
                        ui.scrolled.add_tick_callback(move |_, _| {
                            pending.set(false);
                            if let Some(ui) = h.get() {
                                if let Some((zoom, restore)) = latest.take() {
                                    ui.set_zoom_restore(zoom, restore);
                                }
                            }
                            glib::ControlFlow::Break
                        });
                    }
                }
            }
        });
        let finish_pinch = {
            let h = h.clone();
            let pinch_anchor = pinch_anchor.clone();
            let pinch_latest = pinch_latest.clone();
            move || {
                pinch_anchor.set(None);
                if let Some(ui) = h.get() {
                    if let Some((zoom, restore)) = pinch_latest.take() {
                        ui.set_zoom_restore(zoom, restore);
                    }
                    ui.finish_pinch();
                }
            }
        };
        pinch.connect_end({
            let finish_pinch = finish_pinch.clone();
            move |_, _| finish_pinch()
        });
        pinch.connect_cancel({
            let finish_pinch = finish_pinch.clone();
            move |_, _| finish_pinch()
        });
        scrolled.add_controller(pinch);

        let drop = gtk::DropTarget::new(gio::File::static_type(), gdk::DragAction::COPY);
        drop.connect_drop({
            let h = h.clone();
            move |_, val, _, _| {
                if let Ok(file) = val.get::<gio::File>() {
                    if let Some(path) = file.path() {
                        if let Some(ui) = h.get() {
                            ui.open_path(&path, None);
                            return true;
                        }
                    }
                }
                false
            }
        });
        window.add_controller(drop);

        ui.connect_edit_bar();
        im.connect_commit({
            let h = h.clone();
            move |_, text| {
                if let Some(ui) = h.get() {
                    ui.insert_inline_text(text);
                }
            }
        });
        keys.connect_key_pressed({
            let h = h.clone();
            move |_, key, _, mods| {
                h.get()
                    .map(|ui| ui.key(key, mods))
                    .unwrap_or(glib::Propagation::Proceed)
            }
        });
        window.add_controller(keys);

        sidebar_btn.connect_toggled({
            let h = h.clone();
            move |btn| {
                if let Some(ui) = h.get() {
                    ui.set_sidebar_visible(btn.is_active());
                }
            }
        });
        open_btn.connect_clicked({
            let h = h.clone();
            move |_| {
                if let Some(ui) = h.get() {
                    ui.open_dialog();
                }
            }
        });
        back_btn.connect_clicked({
            let h = h.clone();
            move |_| {
                if let Some(ui) = h.get() {
                    ui.navigate_back();
                }
            }
        });
        forward_btn.connect_clicked({
            let h = h.clone();
            move |_| {
                if let Some(ui) = h.get() {
                    ui.navigate_forward();
                }
            }
        });
        prev_btn.connect_clicked({
            let h = h.clone();
            move |_| {
                if let Some(ui) = h.get() {
                    ui.goto_page_rel(-1);
                }
            }
        });
        next_btn.connect_clicked({
            let h = h.clone();
            move |_| {
                if let Some(ui) = h.get() {
                    ui.goto_page_rel(1);
                }
            }
        });
        zoom_out_btn.connect_clicked({
            let h = h.clone();
            move |_| {
                if let Some(ui) = h.get() {
                    ui.zoom_by(1.0 / 1.15);
                }
            }
        });
        zoom_in_btn.connect_clicked({
            let h = h.clone();
            move |_| {
                if let Some(ui) = h.get() {
                    ui.zoom_by(1.15);
                }
            }
        });
        find_btn.connect_clicked({
            let h = h.clone();
            move |_| {
                if let Some(ui) = h.get() {
                    ui.show_search();
                }
            }
        });
        find_prev.connect_clicked({
            let h = h.clone();
            move |_| {
                if let Some(ui) = h.get() {
                    ui.search_step(-1);
                }
            }
        });
        find_next.connect_clicked({
            let h = h.clone();
            move |_| {
                if let Some(ui) = h.get() {
                    ui.search_step(1);
                }
            }
        });
        find_close.connect_clicked({
            let h = h.clone();
            move |_| {
                if let Some(ui) = h.get() {
                    ui.hide_search();
                }
            }
        });
        page_entry.connect_activate({
            let h = h.clone();
            move |e| {
                if let Some(ui) = h.get() {
                    let labels = ui
                        .inner
                        .borrow()
                        .view
                        .doc
                        .as_ref()
                        .map(|doc| doc.labels.clone())
                        .unwrap_or_default();
                    if let Some(page) = page_index_from_input(&e.text(), &labels) {
                        ui.navigate_to_page(page);
                        ui.pages_box.grab_focus();
                    }
                }
            }
        });
        search_entry.connect_activate({
            let h = h.clone();
            move |e| {
                if let Some(ui) = h.get() {
                    ui.cancel_live_search();
                    ui.search(&e.text());
                }
            }
        });
        search_entry.connect_changed({
            let h = h.clone();
            move |e| {
                if let Some(ui) = h.get() {
                    if ui.ignore_search_changed.get() {
                        return;
                    }
                    ui.queue_live_search();
                    let _ = e;
                }
            }
        });
        case_btn.connect_toggled({
            let h = h.clone();
            move |btn| {
                if let Some(ui) = h.get() {
                    ui.inner.borrow_mut().search.options.case_sensitive = btn.is_active();
                    ui.rerun_visible_search();
                }
            }
        });
        word_btn.connect_toggled({
            let h = h.clone();
            move |btn| {
                if let Some(ui) = h.get() {
                    ui.inner.borrow_mut().search.options.whole_word = btn.is_active();
                    ui.rerun_visible_search();
                }
            }
        });
        search_results.connect_row_activated({
            let h = h.clone();
            move |_, row| {
                if let Some(ui) = h.get() {
                    if ui.ignore_search_results.get() {
                        return;
                    }
                    if let Ok(index) = row.widget_name().trim_start_matches('s').parse::<usize>() {
                        ui.inner.borrow_mut().search.idx = Some(index);
                        ui.jump_search();
                    }
                }
            }
        });
        file_reload_btn.connect_clicked({
            let h = h.clone();
            move |_| {
                if let Some(ui) = h.get() {
                    ui.hide_file_banner();
                    ui.reload();
                }
            }
        });
        file_dismiss_btn.connect_clicked({
            let h = h.clone();
            move |_| {
                if let Some(ui) = h.get() {
                    ui.hide_file_banner();
                }
            }
        });
        zoom_drop.connect_selected_notify({
            let h = h.clone();
            move |drop| {
                if let Some(ui) = h.get() {
                    ui.zoom_from_dropdown(drop.selected());
                }
            }
        });
        thumbs_sw.vadjustment().connect_value_changed({
            let h = h.clone();
            move |_| {
                if let Some(ui) = h.get() {
                    ui.schedule_thumbs();
                }
            }
        });
        thumbs.connect_row_activated({
            let h = h.clone();
            move |_, row| {
                if let Some(ui) = h.get() {
                    ui.navigate_to_page(row.index());
                }
            }
        });
        outline.connect_row_activated({
            let h = h.clone();
            move |_, row| {
                if let Some(ui) = h.get() {
                    let index = row
                        .widget_name()
                        .trim_start_matches('o')
                        .parse::<usize>()
                        .ok();
                    let page = index.and_then(|index| {
                        ui.inner
                            .borrow()
                            .view
                            .doc
                            .as_ref()
                            .and_then(|doc| doc.outline.get(index))
                            .and_then(|item| item.page)
                    });
                    if let Some(page) = page {
                        ui.navigate_to_page(page);
                    }
                }
            }
        });
        marks.connect_row_activated({
            let h = h.clone();
            move |_, row| {
                if let Some(ui) = h.get() {
                    if ui.ignore_marks.get() {
                        return;
                    }
                    let id = row.widget_name().to_string();
                    ui.activate_mark(&id);
                }
            }
        });
        stack.connect_visible_child_name_notify({
            let h = h.clone();
            move |stack| {
                if let Some(ui) = h.get() {
                    let name = stack.visible_child_name();
                    let mut st = ui.inner.borrow_mut();
                    if st.pane.sidebar == Sidebar::Hidden {
                        return;
                    }
                    st.pane.sidebar = match name.as_deref() {
                        Some("outline") => Sidebar::Outline,
                        Some("marks") => Sidebar::Marks,
                        _ => Sidebar::Thumbs,
                    };
                    std::mem::drop(st);
                    ui.populate_sidebar();
                }
            }
        });
        window.connect_close_request({
            let h = h.clone();
            move |_| {
                if let Some(ui) = h.get() {
                    ui.commit_text_draft();
                    if ui.pdf_dirty.get() {
                        ui.confirm_discard(|ui| ui.window.close());
                        return glib::Propagation::Stop;
                    }
                    ui.persist();
                    ui.hold.borrow_mut().take();
                }
                glib::Propagation::Proceed
            }
        });

        let monitor = theme::watch({
            let h = h.clone();
            move || {
                if let Some(ui) = h.get() {
                    ui.reload_theme();
                }
            }
        });
        *ui._monitor.borrow_mut() = monitor;

        ui.sync_sidebar();
        ui.update_chrome();
        *ui.hold.borrow_mut() = Some(ui.clone());
        ui
    }
}
