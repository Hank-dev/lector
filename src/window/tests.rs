use super::*;

#[test]
fn pinch_and_restore_do_not_steal_viewport_or_pan() {
    assert!(!viewport_resize_should_reflow(true));
    assert!(viewport_resize_should_reflow(false));
    assert_eq!(
        canvas_scroll_response(false, 1.0, true),
        glib::Propagation::Stop
    );
    assert_eq!(
        canvas_scroll_response(false, 1.0, false),
        glib::Propagation::Proceed
    );
    assert_eq!(
        canvas_scroll_response(true, 1.0, false),
        glib::Propagation::Stop
    );
}

#[test]
fn revealing_the_sidebar_opens_the_outline() {
    assert_eq!(sidebar_when_revealed(Sidebar::Hidden), Sidebar::Outline);
    assert_eq!(sidebar_when_revealed(Sidebar::Outline), Sidebar::Outline);
    assert_eq!(sidebar_when_revealed(Sidebar::Thumbs), Sidebar::Thumbs);
    assert_eq!(sidebar_when_revealed(Sidebar::Marks), Sidebar::Marks);
}

#[test]
fn sidebar_defaults_to_hidden_unless_explicitly_opened() {
    assert_eq!(parse_sidebar(""), Sidebar::Hidden);
    assert_eq!(parse_sidebar("hidden"), Sidebar::Hidden);
    assert_eq!(parse_sidebar("unknown"), Sidebar::Hidden);
    assert_eq!(parse_sidebar("thumbs"), Sidebar::Thumbs);
    assert_eq!(parse_sidebar("outline"), Sidebar::Outline);
    assert_eq!(parse_sidebar("marks"), Sidebar::Marks);
}

#[test]
fn pinch_begin_does_not_stick_ignore_scroll() {
    assert!(!pinch_begin_ignore_scroll());
}

#[test]
fn toolbar_stays_hidden_until_toggled_back() {
    assert!(toolbar_is_shown(false, false));
    assert!(!toolbar_is_shown(false, true));
    assert!(!toolbar_is_shown(true, false));
    assert!(!toolbar_is_shown(true, true));
}

#[test]
fn atomic_replace_is_a_reload_when_the_path_still_exists() {
    assert_eq!(
        resolved_disk_change(Some(DiskChange::Removed), true),
        Some(DiskChange::Modified)
    );
    assert_eq!(
        resolved_disk_change(Some(DiskChange::Removed), false),
        Some(DiskChange::Removed)
    );
    assert_eq!(
        resolved_disk_change(Some(DiskChange::Modified), true),
        Some(DiskChange::Modified)
    );
    assert_eq!(resolved_disk_change(None, true), None);
}

#[test]
fn history_shortcuts_are_recognized_before_focused_entry_input() {
    assert_eq!(
        navigation_shortcut(Key::Left, gdk::ModifierType::ALT_MASK),
        Some(HistoryDirection::Back)
    );
    assert_eq!(
        navigation_shortcut(Key::Right, gdk::ModifierType::ALT_MASK),
        Some(HistoryDirection::Forward)
    );
    assert_eq!(
        navigation_shortcut(Key::Left, gdk::ModifierType::empty()),
        None
    );
}

#[test]
fn outline_toggle_changes_visible_rows() {
    let items = vec![
        crate::doc::OutlineItem {
            title: "Chapter".into(),
            page: Some(0),
            depth: 0,
        },
        crate::doc::OutlineItem {
            title: "Section".into(),
            page: Some(1),
            depth: 1,
        },
    ];
    let mut collapsed = HashSet::new();

    assert!(toggle_outline_collapsed(&items, &mut collapsed, 0));
    assert_eq!(
        crate::doc::visible_outline_indices(&items, &collapsed),
        vec![0]
    );
    assert!(toggle_outline_collapsed(&items, &mut collapsed, 0));
    assert_eq!(
        crate::doc::visible_outline_indices(&items, &collapsed),
        vec![0, 1]
    );
    assert!(!toggle_outline_collapsed(&items, &mut collapsed, 1));
}

#[test]
fn navigation_history_moves_backward_and_forward_without_duplicates() {
    let first = ViewLocation::new(2, 0.0, 320.0);
    let second = ViewLocation::new(8, 0.0, 910.0);
    let third = ViewLocation::new(13, 0.0, 1450.0);
    let mut history = NavigationHistory::default();

    history.visit(first, second);
    history.visit(second, third);
    history.visit(third, third);

    assert_eq!(history.back(third), Some(second));
    assert_eq!(history.back(second), Some(first));
    assert_eq!(history.forward(first), Some(second));
    assert_eq!(history.forward(second), Some(third));
    assert_eq!(history.forward(third), None);
}

#[test]
fn pinch_updates_reuse_the_gesture_start_anchor() {
    let anchor = ViewportAnchor {
        page: 2,
        rel_x: 0.5,
        rel_y: 0.4,
        view_x: 250.0,
        view_y: 300.0,
    };

    let (zoom, restore) = pinch_zoom_update(1.0, 1.5, anchor);
    assert_eq!(zoom, Zoom::Custom(1.5));
    assert_eq!(restore, ViewRestore::Anchor(anchor));
}

#[test]
fn double_click_fit_is_suppressed_over_document_links() {
    assert!(should_toggle_fit(2, false, false));
    assert!(!should_toggle_fit(2, true, false));
    assert!(!should_toggle_fit(2, false, true));
    assert!(!should_toggle_fit(1, false, false));
}

#[test]
fn external_link_policy_allows_web_and_mail_but_rejects_local_handlers() {
    for uri in [
        "https://omarchy.org/docs",
        "HTTP://example.com",
        "mailto:hello@example.com",
    ] {
        assert!(external_uri_allowed(uri), "{uri}");
    }
    for uri in [
        "file:///etc/passwd",
        "javascript:alert(1)",
        "ssh://host",
        "custom-handler:payload",
        "not a uri",
    ] {
        assert!(!external_uri_allowed(uri), "{uri}");
    }
}

#[test]
fn search_snippet_centers_the_query_and_collapses_whitespace() {
    let text = "Preface\n\nThe Needle sits in the middle of this paragraph about search.";
    assert_eq!(
        search_snippet(text, "needle", false),
        "The Needle sits in the middle of this paragraph…"
    );
    assert_eq!(
        search_snippet(text, "Needle", true),
        search_snippet(text, "needle", false)
    );
    assert!(search_snippet(text, "Needle", true).contains("Needle"));
    assert!(search_snippet("no match here", "zzz", false).starts_with("no match"));
    assert_eq!(
        search_result_row_text("iv", "The Needle sits…"),
        "iv  The Needle sits…"
    );
}

#[test]
fn document_monitor_events_distinguish_edits_from_removal() {
    assert_eq!(
        classify_document_event(gio::FileMonitorEvent::ChangesDoneHint),
        Some(DiskChange::Modified)
    );
    assert_eq!(
        classify_document_event(gio::FileMonitorEvent::Changed),
        Some(DiskChange::Modified)
    );
    assert_eq!(
        classify_document_event(gio::FileMonitorEvent::Renamed),
        Some(DiskChange::Modified)
    );
    assert_eq!(
        classify_document_event(gio::FileMonitorEvent::Deleted),
        Some(DiskChange::Removed)
    );
    assert_eq!(
        classify_document_event(gio::FileMonitorEvent::MovedOut),
        Some(DiskChange::Removed)
    );
    assert_eq!(
        classify_document_event(gio::FileMonitorEvent::AttributeChanged),
        None
    );
}

#[test]
fn image_surface_can_be_copied_into_a_texture() {
    let surface =
        cairo::ImageSurface::create(cairo::Format::ARgb32, 10, 10).expect("create Cairo surface");
    assert!(
        surface_texture(&surface).is_some(),
        "a valid shared Cairo surface must produce a texture"
    );
}

#[test]
fn page_structure_requires_matching_layout_and_page_identities() {
    let continuous = PageStructure::new(LayoutMode::Continuous, vec![0, 1]);
    assert!(continuous.matches(LayoutMode::Continuous, &[0, 1]));
    assert!(!continuous.matches(LayoutMode::Dual, &[0, 1]));
    assert!(!continuous.matches(LayoutMode::Single, &[1]));
    assert!(!continuous.matches(LayoutMode::Continuous, &[1, 2]));
}

#[test]
fn presentation_restore_restores_all_state_before_fullscreen_exit() {
    let mut presentation = true;
    let mut restore = Some((LayoutMode::Dual, Zoom::Custom(1.25), Sidebar::Outline));
    let mut layout = LayoutMode::Single;
    let mut zoom = Zoom::FitPage;
    let mut sidebar = Sidebar::Hidden;

    assert!(restore_presentation_state(
        &mut presentation,
        &mut restore,
        &mut layout,
        &mut zoom,
        &mut sidebar,
    ));
    assert!(!presentation);
    assert_eq!(restore, None);
    assert_eq!(layout, LayoutMode::Dual);
    assert_eq!(zoom, Zoom::Custom(1.25));
    assert_eq!(sidebar, Sidebar::Outline);
}

#[test]
fn allocation_changes_are_filtered_without_missing_sidebar_or_find_resizes() {
    assert!(!allocation_changed((800, 700), (800, 700)));
    assert!(allocation_changed((800, 700), (640, 700)));
    assert!(allocation_changed((640, 700), (640, 650)));
    assert!(!allocation_changed((640, 650), (16, 650)));
}

#[test]
fn replaced_search_work_is_generation_cancelled_and_bounded() {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    let context = glib::MainContext::default();
    let scheduled_generation: u64 = 41;
    let generation = Rc::new(Cell::new(scheduled_generation));
    let cancelled_runs = Rc::new(Cell::new(0));
    glib::idle_add_local_once({
        let generation = generation.clone();
        let cancelled_runs = cancelled_runs.clone();
        move || {
            if generation.get() == scheduled_generation {
                cancelled_runs.set(cancelled_runs.get() + 1);
            }
        }
    });
    generation.set(scheduled_generation.wrapping_add(1));

    let mut turns = 0;
    while context.pending() {
        turns += 1;
        assert!(
            turns <= 8,
            "cancelled idle work must leave the main context promptly"
        );
        context.iteration(false);
    }
    assert_eq!(cancelled_runs.get(), 0);

    // A defensive bound also prevents a pathological old job from
    // scheduling forever if a document changes size unexpectedly.
    let old_job = Rc::new(RefCell::new(
        SearchJob::after_first_page(8, 6).expect("pages remain to search"),
    ));
    let inspected = Rc::new(RefCell::new(Vec::new()));
    glib::idle_add_local({
        let old_job = old_job.clone();
        let inspected = inspected.clone();
        move || {
            let mut job = old_job.borrow_mut();
            if let Some(page) = job.take_page(1) {
                inspected.borrow_mut().push(page);
                if job.remaining > 0 {
                    glib::ControlFlow::Continue
                } else {
                    glib::ControlFlow::Break
                }
            } else {
                glib::ControlFlow::Break
            }
        }
    });
    while context.pending() {
        turns += 1;
        assert!(
            turns <= 16,
            "old work must remain bounded after a short replacement"
        );
        context.iteration(false);
    }
    assert_eq!(inspected.borrow().as_slice(), &[0, 0, 0, 0, 0, 0, 0]);
}

#[test]
fn document_replacement_invalidates_query_and_pending_hits() {
    let mut search = SearchState::new();
    search.gen = 9;
    search.query = "needle".to_string();
    let mut rectangle = poppler::Rectangle::new();
    rectangle.set_x1(1.0);
    search.hits = vec![(4, rectangle)];
    search.idx = Some(0);

    search.invalidate();
    assert_eq!(search.gen, 10);
    assert!(search.query.is_empty());
    assert!(search.hits.is_empty());
    assert_eq!(search.idx, None);
}

#[test]
fn outgoing_document_state_is_available_to_a_same_file_reload() {
    let mut session = Session::default();
    let path = Path::new("/documents/reload.pdf");
    remember_document_state(&mut session, path, 6, Zoom::Custom(1.4), 270);

    let restored = session
        .restore(path)
        .expect("state remembered before reload");
    assert_eq!(restored.page, 6);
    assert_eq!(restored.rotation, 270);
    assert_eq!(
        session::parse_zoom(&restored.zoom, restored.custom),
        Zoom::Custom(1.4)
    );
}

#[test]
fn ctrl_scroll_zoom_uses_stepped_wheel_and_smooth_touchpad_deltas() {
    assert_eq!(ctrl_scroll_zoom_factor(0.0), 1.0);
    assert_eq!(ctrl_scroll_zoom_factor(-1.0), 1.1);
    assert_eq!(ctrl_scroll_zoom_factor(1.0), 1.0 / 1.1);
    assert!((ctrl_scroll_zoom_factor(-0.2) - 1.1_f64.powf(0.2)).abs() < 1e-12);
}

#[test]
fn pinch_and_factor_zoom_clamp_to_supported_range() {
    assert_eq!(pinch_target_scale(1.0, 2.0), 2.0);
    assert_eq!(pinch_target_scale(1.0, 0.01), 0.1);
    assert_eq!(pinch_target_scale(8.0, 4.0), MAX_ZOOM);
    assert_eq!(zoom_from_factor(1.0, 1.15), Zoom::Custom(1.15));
    assert_eq!(zoom_from_factor(0.1, 0.5), Zoom::Custom(0.1));
    assert_eq!(zoom_from_factor(10.0, 4.0), Zoom::Custom(MAX_ZOOM));
    assert_eq!(zoom_chrome(Zoom::Custom(1.37)), ZoomChrome::Percent(137));
    assert_eq!(zoom_chrome(Zoom::Custom(1.15)), ZoomChrome::Percent(115));
    assert_eq!(zoom_chrome(Zoom::Actual), ZoomChrome::Preset(7));
    assert_eq!(zoom_chrome(Zoom::Custom(1.25)), ZoomChrome::Preset(8));
    assert_eq!(zoom_chrome(Zoom::FitWidth), ZoomChrome::Preset(2));
}

#[test]
fn zoom_restore_keeps_the_same_spot_on_the_same_page() {
    let old = [
        crate::view::PageGeom {
            index: 0,
            x: 8.0,
            y: 16.0,
            w: 612.0,
            h: 792.0,
            scale: 1.0,
        },
        crate::view::PageGeom {
            index: 8,
            x: 8.0,
            y: 16.0 + 8.0 * (792.0 + 16.0),
            w: 612.0,
            h: 792.0,
            scale: 1.0,
        },
    ];
    let focus_x = old[1].x + old[1].w * 0.5;
    let focus_y = old[1].y + old[1].h * 0.5;
    let anchor = anchor_from_content(&old, focus_x, focus_y, 40.0, 50.0);
    let new = [
        crate::view::PageGeom {
            index: 0,
            x: 8.0,
            y: 16.0,
            w: 1224.0,
            h: 1584.0,
            scale: 2.0,
        },
        crate::view::PageGeom {
            index: 8,
            x: 8.0,
            y: 16.0 + 8.0 * (1584.0 + 16.0),
            w: 1224.0,
            h: 1584.0,
            scale: 2.0,
        },
    ];
    let (x, y) = restored_scroll(anchor, &new);
    let expected_y = new[1].y + new[1].h * 0.5 - 50.0;
    let naive_y = focus_y * 2.0 - 50.0;
    assert!(
        (y - expected_y).abs() < 0.5,
        "restore must keep mid-page 8, got {y} expected {expected_y}"
    );
    assert!(
        (naive_y - expected_y).abs() > 100.0,
        "uniform scale would be wrong by more than a line: {naive_y} vs {expected_y}"
    );
    assert!((x - (new[1].x + new[1].w * 0.5 - 40.0)).abs() < 0.5);
}

#[test]
fn zoom_restores_the_content_point_under_the_viewport_focus() {
    assert_eq!(clamp_adjustment(500.0, 0.0, 400.0, 100.0), 300.0);
    assert_eq!(clamp_adjustment(-10.0, 0.0, 400.0, 100.0), 0.0);
}

#[test]
fn same_page_scroll_does_not_request_a_chrome_update() {
    assert_eq!(continuous_scroll_page_changed(0, 0), None);
    assert_eq!(continuous_scroll_page_changed(2, 2), None);
    assert_eq!(continuous_scroll_page_changed(0, 4), Some(4));
}

#[test]
fn arrow_keys_scroll_and_clamp_only_in_continuous_view() {
    for (mode, key, value, expected) in [
        (LayoutMode::Continuous, Key::Down, 100.0, Some(164.0)),
        (LayoutMode::Continuous, Key::Up, 20.0, Some(0.0)),
        (LayoutMode::Continuous, Key::Down, 680.0, Some(700.0)),
        (LayoutMode::Single, Key::Down, 100.0, None),
        (LayoutMode::Continuous, Key::Right, 100.0, None),
    ] {
        assert_eq!(
            continuous_arrow_scroll_target(mode, key, value, 0.0, 1000.0, 300.0),
            expected
        );
    }
}

#[test]
fn shortcut_overlay_is_grouped_like_the_app_menu() {
    let headings: Vec<_> = shortcut_sections().iter().map(|s| s.heading).collect();
    assert_eq!(
        headings,
        [
            "FILE",
            "ANNOTATIONS",
            "VIEW",
            "FIND",
            "NAVIGATION",
            "DOCUMENT"
        ]
    );
    assert!(shortcut_sections().iter().any(|section| {
        section
            .rows
            .iter()
            .any(|row| row.action == "Open" && row.keys == "Ctrl+O")
    }));
    assert!(shortcut_sections()
        .iter()
        .any(|section| section.rows.iter().any(|row| row.action == "Pinch to zoom")));
    assert!(shortcut_sections().iter().any(|section| section
        .rows
        .iter()
        .any(|row| row.action == "Highlight selection" && row.keys == "Ctrl+Shift+H")));
    assert!(shortcut_sections().iter().any(|section| section
        .rows
        .iter()
        .any(|row| row.action == "Draw on PDF" && row.keys == "Ctrl+Shift+D")));
}

#[test]
fn shifted_annotation_shortcuts_accept_uppercase_keyvals() {
    let modifiers = gdk::ModifierType::CONTROL_MASK | gdk::ModifierType::SHIFT_MASK;
    assert_eq!(
        annotation_shortcut(Key::H, modifiers),
        Some(AnnotationShortcut::Highlight)
    );
    assert_eq!(
        annotation_shortcut(Key::N, modifiers),
        Some(AnnotationShortcut::Note)
    );
    assert_eq!(
        annotation_shortcut(Key::h, gdk::ModifierType::CONTROL_MASK),
        Some(AnnotationShortcut::History)
    );
}

#[test]
fn beginning_selection_and_empty_annotation_hits_clear_the_old_target() {
    let mut selection = None;
    let mut targeted = Some("old-annotation".to_string());
    begin_selection_state(&mut selection, &mut targeted, Some((2, 10.0, 20.0)));
    let selection = selection.expect("selection begins at the hit");
    assert_eq!(selection.page, 2);
    assert_eq!((selection.x1, selection.y1), (10.0, 20.0));
    assert_eq!((selection.x2, selection.y2), (10.0, 20.0));
    assert_eq!(targeted, None);

    targeted = Some("another-annotation".to_string());
    assert!(replace_annotation_target(&mut targeted, None));
    assert_eq!(targeted, None);
}

#[test]
fn rotation_clears_display_coordinate_selection() {
    let mut view = ViewState::new();
    view.rotation = 270;
    view.selection = Some(Selection {
        page: 0,
        x1: 10.0,
        y1: 20.0,
        x2: 30.0,
        y2: 40.0,
    });

    view.rotate_by(90);

    assert_eq!(view.rotation, 0);
    assert!(view.selection.is_none());
}

#[test]
fn document_properties_use_em_dash_for_missing_metadata() {
    assert_eq!(display_meta(None), "—");
    assert_eq!(display_meta(Some("")), "—");
    assert_eq!(display_meta(Some("  ")), "—");
    assert_eq!(display_meta(Some("Hanky")), "Hanky");
    let path = crate::doc::write_test_pdf("props-meta", &["BT /F1 20 Tf 72 700 Td (hi) Tj ET"]);
    let doc = crate::doc::OpenDoc::open(&path, None).expect("fixture pdf");
    let rows = property_fields(&doc);
    assert!(rows.iter().any(|(key, value)| *key == "File"
        && Path::new(value)
            .file_name()
            .is_some_and(|name| name.to_string_lossy().contains("props-meta"))));
    assert!(rows
        .iter()
        .any(|(key, value)| *key == "Pages" && value == "1"));
    for key in ["Author", "Subject"] {
        assert!(
            rows.iter().any(|(k, v)| *k == key && v == "—"),
            "{key} should use an em dash when Poppler has no value"
        );
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn about_copy_names_lector_not_a_generic_toolkit_dialog() {
    let about = about_copy();
    assert_eq!(about.name, "Lector");
    assert_eq!(about.tagline, "Native PDF viewer for Omarchy");
    assert_eq!(about.version, env!("CARGO_PKG_VERSION"));
    assert!(
        about.detail.contains("not in the PDF"),
        "about copy must say marks stay in Lector, not in the file: {}",
        about.detail
    );
    assert!(!about.detail.contains("GNOME"));
}

#[test]
fn marks_empty_copy_says_sidecar_storage() {
    assert!(marks_empty_copy().contains("not in the PDF"));
}

#[test]
fn save_a_copy_suggests_a_new_name() {
    assert_eq!(
        copy_file_name(Path::new("/tmp/Exam form.pdf")).as_deref(),
        Some("Exam form (copy).pdf")
    );
    assert_eq!(
        copy_file_name(Path::new("/tmp/scan")).as_deref(),
        Some("scan (copy)")
    );
}

#[test]
fn live_stroke_preview_matches_the_full_smoothing() {
    let points = (0..200)
        .map(|i| {
            let t = i as f64 * 0.1;
            (t * 7.0, (t * 1.3).sin() * 20.0 + (t * 0.4).cos() * 5.0)
        })
        .collect::<Vec<_>>();
    let mut stroke = super::ink::ActiveInk::new(3, points[0]);
    for (n, &point) in points.iter().enumerate().skip(1) {
        stroke.push(point);
        let full = crate::doc::smooth_polyline(&points[..=n]);
        let preview = stroke.preview();
        assert_eq!(preview.len(), full.len(), "after {} points", n + 1);
        for (a, b) in preview.iter().zip(&full) {
            assert!((a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9);
        }
    }
}

#[test]
fn search_snippet_survives_case_folding_that_changes_byte_length() {
    // U+212B ANGSTROM SIGN and U+212A KELVIN SIGN shrink when lower-cased;
    // U+0130 grows. Byte offsets in the folded text must not cut the original.
    assert!(search_snippet("1.5 \u{212B}, résumé", "sum", false).contains("résumé"));
    assert!(search_snippet("300 \u{212A} in résumé", "RÉSUMÉ", false).contains("in résumé"));
    assert!(
        search_snippet("\u{130}stanbul ẞtraße needle", "needle", false)
            .starts_with("ẞtraße needle")
    );
    assert!(search_snippet("\u{212B}ngstrom unit", "\u{e5}ngstrom", false).starts_with('\u{212B}'));
}
