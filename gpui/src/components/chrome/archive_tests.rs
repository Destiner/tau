use super::*;

fn fixture(count: usize) -> (SidebarProps, BTreeMap<String, ArchivedDetails>) {
    let mut details = BTreeMap::new();
    let sessions = (0..count)
        .map(|index| {
            // Deliberately reverse lexical IDs: rendering must use caller order, not ID order.
            let id = format!("session-{:04}", count - index);
            details.insert(
                id.clone(),
                ArchivedDetails {
                    group: ARCHIVED_GROUPS[index * 5 / count].into(),
                    project: format!("Project {}", index % 7),
                    model: Some(format!("Model {}", index % 3)),
                },
            );
            SessionItem {
                id,
                title: format!(
                    "Archived investigation {index}: preserve drafts and runtime isolation"
                ),
                time: format!("{}d", index + 1),
                status: None,
                unread: index % 11 == 0,
                can_archive: false,
            }
        })
        .collect();
    (
        SidebarProps {
            sessions,
            active: String::new(),
            collapsed: false,
            present: true,
            archived: true,
        },
        details,
    )
}

fn bounds(window: &Window, id: &str) -> Bounds<Pixels> {
    gpui_kit::base::test_support::find(window, &[], &id.to_owned().into())
        .unwrap_or_else(|| panic!("missing {id}"))
        .bounds()
}

fn archive_window_options() -> gpui_kit::WindowOptions {
    gpui_kit::WindowOptions {
        window_bounds: Some(gpui_kit::WindowBounds::Windowed(Bounds::new(
            gpui_kit::point(px(0.), px(0.)),
            gpui_kit::size(px(800.), px(600.)),
        ))),
        ..Default::default()
    }
}

fn mounted_rows(window: &Window) -> usize {
    gpui_kit::base::test_support::snapshots(window)
        .iter()
        .filter(|snapshot| {
            snapshot
                .path()
                .last()
                .is_some_and(|id| format!("{id:?}").contains("archived-row-"))
        })
        .count()
}

#[test]
fn noncontiguous_activity_groups_are_bucketed_once_in_calendar_order() {
    let (mut props, mut details) = fixture(8);
    let groups = [
        "Older",
        "Today",
        "Older",
        "Last Week",
        "Yesterday",
        "Today",
        "This Week",
        "Yesterday",
    ];
    for (session, group) in props.sessions.iter().zip(groups) {
        details.get_mut(&session.id).unwrap().group = group.into();
    }
    // Caller order represents the global saved sort; runtime activity can cross buckets.
    let ids: Vec<_> = props.sessions.iter().map(|s| s.id.clone()).collect();
    props.active = ids[2].clone();
    let mut app = gpui_kit::TestApp::new();
    app.update(gpui_kit::init);
    let mut window = app.open_window_with_options(archive_window_options(), |_, cx| {
        let mut sidebar = Sidebar::new(props, vec![], "1.2.3".into(), false, cx);
        sidebar.set_archived_details(details, cx);
        sidebar
    });
    window.update(|s, _, _| {
        let headings: Vec<_> = s
            .archive_entries
            .iter()
            .filter_map(|e| match e {
                ArchiveEntry::Heading(label) => Some(label.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(headings, ARCHIVED_GROUPS);
        let rows: Vec<_> = s
            .archive_entries
            .iter()
            .filter_map(|e| match e {
                ArchiveEntry::Session { index, id } => Some((*index, id.clone())),
                _ => None,
            })
            .collect();
        let expected: Vec<_> = [1, 5, 4, 7, 6, 3, 0, 2]
            .into_iter()
            .map(|i| (i, ids[i].clone()))
            .collect();
        assert_eq!(rows, expected);
        s.closed_groups.insert("Today".into());
        s.rebuild_archive();
        assert_eq!(
            s.archive_entries
                .iter()
                .filter(|e| matches!(e, ArchiveEntry::Heading(_)))
                .count(),
            5
        );
        assert!(
            !s.archive_entries
                .iter()
                .any(|e| matches!(e, ArchiveEntry::Session { index: 1 | 5, .. }))
        );
    });
}

#[test]
fn large_archive_is_bounded_ordered_grouped_and_reaches_last_row() {
    for count in [400, 1000] {
        let (props, details) = fixture(count);
        let mut app = gpui_kit::TestApp::new();
        app.update(gpui_kit::init);
        let mut window = app.open_window_with_options(archive_window_options(), |_, cx| {
            let mut sidebar = Sidebar::new(props.clone(), vec![], "1.2.3".into(), false, cx);
            sidebar.set_archived_details(details.clone(), cx);
            sidebar
        });
        window.draw();
        window.update(|s, w, _| {
            assert!(
                mounted_rows(w) > 0 && mounted_rows(w) <= 24,
                "painted rows={}, renderer ids={}, body={:?}",
                mounted_rows(w),
                s.mounted_archive_ids.len(),
                bounds(w, "sidebar-body")
            );
            assert!(s.mounted_archive_ids.len() <= 24);
            assert!(s.tooltip_markdown.is_empty());
            assert_eq!(
                s.archive_entries
                    .iter()
                    .filter_map(|entry| match entry {
                        ArchiveEntry::Session { id, .. } => Some(id),
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
                props
                    .sessions
                    .iter()
                    .map(|session| &session.id)
                    .collect::<Vec<_>>()
            );
            assert_eq!(
                s.archive_entries
                    .iter()
                    .filter_map(|entry| match entry {
                        ArchiveEntry::Heading(group) => Some(group.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
                ARCHIVED_GROUPS
            );
            let first = &props.sessions[0].id;
            let second = &props.sessions[1].id;
            assert!(
                bounds(w, &format!("archived-row-{first}")).top()
                    < bounds(w, &format!("archived-row-{second}")).top()
            );
            assert_eq!(bounds(w, "sidebar").size.height, px(600.));
            let heading = bounds(w, "archived-group-Today");
            assert!(bounds(w, "sidebar-body").contains(&heading.center()));
            assert!(heading.bottom() <= bounds(w, &format!("archived-row-{first}")).top());
            let row = bounds(w, &format!("archived-row-{first}"));
            assert!(row.right() <= bounds(w, "sidebar-body").right());
            assert!(
                bounds(w, "sidebar-body")
                    .contains(&bounds(w, &format!("restore-{first}")).center())
            );
            bounds(w, &format!("archived-meta-{first}"));
        });
        let pointer = window.update(|_, w, _| bounds(w, "sidebar-body").center());
        window.simulate_mouse_move(pointer);
        window.simulate_scroll(pointer, gpui_kit::point(px(0.), px(-340.)));
        window.draw();
        window.update(|s, w, _| {
            assert!(s.archive_list.logical_scroll_top().item_ix > 0);
            assert!(mounted_rows(w) > 0 && mounted_rows(w) <= 24);
            assert!(s.mounted_archive_ids.len() <= 24);
        });
        // Exercise every group and the real row renderer, not merely the flattened data.
        for group in ARCHIVED_GROUPS {
            window.update(|s, _, cx| {
                let index = s
                    .archive_entries
                    .iter()
                    .position(|entry| entry == &ArchiveEntry::Heading(group.into()))
                    .unwrap();
                s.archive_list.scroll_to(ListOffset {
                    item_ix: index,
                    offset_in_item: px(0.),
                });
                cx.notify();
            });
            window.draw();
            window.update(|s, w, _| {
                bounds(w, &format!("archived-group-{group}"));
                assert!(mounted_rows(w) > 0 && mounted_rows(w) <= 24);
                assert!(s.archived_focus.len() <= 48);
            });
        }
        window.update(|s, _, cx| {
            s.archive_list.scroll_to_end();
            cx.notify();
        });
        window.draw();
        window.draw();
        window.update(|s, w, _| {
            let last = &props.sessions[count - 1].id;
            let row = bounds(w, &format!("archived-row-{last}"));
            assert!(bounds(w, "sidebar-body").contains(&row.center()));
            bounds(w, &format!("archived-meta-{last}"));
            assert!(mounted_rows(w) > 0 && mounted_rows(w) <= 24);
            assert!(s.tooltip_markdown.is_empty());
        });
    }
}

#[test]
fn archive_updates_preserve_row_anchor_offset_focus_and_restore_identity() {
    let (mut props, details) = fixture(400);
    let mut app = gpui_kit::TestApp::new();
    app.update(gpui_kit::init);
    let mut window = app.open_window_with_options(archive_window_options(), |_, cx| {
        let mut sidebar = Sidebar::new(props.clone(), vec![], "1.2.3".into(), false, cx);
        sidebar.set_archived_details(details, cx);
        sidebar
    });
    window.draw();
    let anchor_id = window.update(|s, _, cx| {
        let index = 175;
        let id = s.archive_entries[index].key().0.to_owned();
        s.archive_list.scroll_to(ListOffset {
            item_ix: index,
            offset_in_item: px(7.),
        });
        cx.notify();
        id
    });
    window.draw();
    window.update(|s, _, cx| s.set_active_session(anchor_id.clone(), cx));
    window.draw();
    let (before, focus) = window.update(|s, w, cx| {
        let focus = s.archived_focus[&anchor_id].0.clone();
        focus.focus(w, cx);
        (bounds(w, &format!("archived-row-{anchor_id}")), focus)
    });
    props.sessions.remove(0);
    props.sessions[190].title = "Updated archived title".into();
    window.update(|s, _, cx| s.set_props(props.clone(), cx));
    window.draw();
    window.update(|s, w, _| {
        assert_eq!(
            bounds(w, &format!("archived-row-{anchor_id}")).top(),
            before.top()
        );
        assert_eq!(s.archive_list.logical_scroll_top().offset_in_item, px(7.));
        assert!(focus.is_focused(w));
        assert_eq!(s.archived_focus[&anchor_id].0, focus);
    });
    let events = Rc::new(RefCell::new(Vec::new()));
    let received = events.clone();
    let _subscription = window.update(|_, _, cx| {
        cx.subscribe(&cx.entity(), move |_, _, event: &Action, _| {
            received.borrow_mut().push(event.clone());
        })
    });
    let restore = window.update(|_, w, _| {
        let restore = bounds(w, &format!("restore-{anchor_id}"));
        assert!(bounds(w, "sidebar-body").contains(&restore.center()));
        assert!(restore.right() <= bounds(w, &format!("archived-row-{anchor_id}")).right());
        restore.center()
    });
    window.simulate_mouse_move(restore);
    window.draw();
    window.simulate_click(restore, MouseButton::Left);
    window.draw();
    assert_eq!(*events.borrow(), vec![Action::Restore(anchor_id.clone())]);
    let removed = props
        .sessions
        .iter()
        .position(|session| session.id == anchor_id)
        .unwrap();
    let next = props.sessions[removed + 1].id.clone();
    props.sessions.remove(removed);
    window.update(|s, _, cx| s.set_props(props.clone(), cx));
    window.draw();
    window.update(|s, w, _| {
        let top = s.archive_list.logical_scroll_top();
        assert_eq!(s.archive_entries[top.item_ix].key().0, next);
        assert_eq!(top.offset_in_item, px(7.));
        assert_eq!(
            bounds(w, &format!("archived-row-{next}")).top(),
            before.top()
        );
        assert!(!s.archived_focus.contains_key(&anchor_id));
        assert!(s.focus.is_focused(w));
    });
}

#[test]
fn header_reveal_matches_css_and_is_continuous_when_interrupted() {
    let mut reveal = HeaderReveal {
        opacity_from: 0.,
        from: 0.,
        to: 0.,
        started: Duration::ZERO,
    };
    assert_eq!(reveal.value(Duration::ZERO), 0.);
    reveal.retarget(1., Duration::ZERO, true);
    let halfway = Duration::from_millis(80);
    assert!((reveal.value(halfway) - 0.946).abs() < 0.003);
    assert_eq!(reveal.opacity(Duration::from_millis(110)), 1.);
    let before = reveal.value(halfway);
    let opacity = reveal.opacity(halfway);
    reveal.retarget(0., halfway, true);
    assert!((reveal.value(halfway) - before).abs() < 0.0001);
    assert!((reveal.opacity(halfway) - opacity).abs() < 0.0001);
    assert_eq!(reveal.value(Duration::from_millis(240)), 0.);
    reveal.retarget(1., Duration::from_millis(250), false);
    assert_eq!(reveal.value(Duration::from_millis(250)), 1.);
    assert_eq!(reveal.opacity(Duration::from_millis(250)), 1.);
}

#[test]
fn production_header_reveal_animates_without_sync_reset_or_hitbox_teleport() {
    let mut app = gpui_kit::TestApp::new();
    app.update(gpui_kit::init);
    let time = Rc::new(Cell::new(Duration::ZERO));
    let mut window = app.open_window(|_, cx| {
        let mut sidebar = Sidebar::new(
            SidebarProps {
                sessions: vec![],
                active: String::new(),
                collapsed: false,
                present: true,
                archived: false,
            },
            vec![ProjectItem {
                id: "p".into(),
                title: "Project".into(),
                location: "/project".into(),
                collapsed: true,
                sessions: vec![],
            }],
            "1".into(),
            false,
            cx,
        );
        let time = time.clone();
        sidebar.clock = Rc::new(move || time.get());
        sidebar
    });
    window.draw();
    let (hitbox, grip) = window.update(|s, w, _| {
        assert!(s.animations_enabled);
        assert_eq!(s.header_slides["p"].value(Duration::ZERO), 0.);
        (bounds(w, "project-drag-p"), bounds(w, "project-grip-p"))
    });
    window.simulate_mouse_move(hitbox.center());
    window.draw();
    time.set(Duration::from_millis(80));
    window.update(|s, _, cx| {
        let mut projects = s.projects.clone();
        projects[0].title = "Updated Project".into();
        s.set_projects(projects.clone(), cx);
        s.set_projects(projects, cx);
    });
    window.draw();
    window.update(|s, w, _| {
        assert_eq!(bounds(w, "project-drag-p"), hitbox);
        let current = bounds(w, "project-grip-p");
        assert!(current.left() > grip.left() && current.left() < grip.left() + px(8.));
        assert!(s.header_slides["p"].running(time.get()));
    });
    app.update(|cx| cx.set_reduce_motion(true));
    let outside = window.update(|_, w, _| bounds(w, "sidebar-footer").center());
    window.simulate_mouse_move(outside);
    window.draw();
    window.update(|s, _, _| {
        assert_eq!(s.header_slides["p"].value(time.get()), 0.);
        assert_eq!(s.header_slides["p"].opacity(time.get()), 0.);
        assert!(!s.header_slides["p"].running(time.get()));
    });
}
