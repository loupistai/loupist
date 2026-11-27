use super::*;

fn state(language: MenuLanguage) -> MenuState {
    MenuState {
        context: AppMenuContext {
            language,
            shell: true,
            loupist: true,
            print: true,
            sidebar: true,
            modal: false,
        },
        window: MainWindow {
            available: true,
            ..MainWindow::default()
        },
        zoom: ACTUAL_SIZE,
    }
}

#[test]
fn every_menu_item_has_an_explicit_label_in_both_languages() {
    crate::extension_availability::with_test_flags(false, false, assert_menu_labels);
}

fn assert_menu_labels() {
    let english = state(MenuLanguage::En);
    let korean = state(MenuLanguage::Ko);
    let app_name = "Loupist Dev";
    let labels = |state: MenuState| {
        MENU.iter()
            .flat_map(|group| {
                let entries = group
                    .items
                    .iter()
                    .filter(|row| !matches!(row.role, Some(NativeRole::Separator)))
                    .map(move |row| row.label(state, app_name, true));
                std::iter::once(group.label(state.context.language, app_name)).chain(entries)
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        labels(english),
        [
            "Loupist Dev",
            "About Loupist Dev",
            "Check for Updates…",
            "Settings…",
            "Services",
            "Hide Loupist Dev",
            "Hide Others",
            "Show All",
            "Quit Loupist Dev",
            "File",
            "New research",
            "New Loupist…",
            "Print…",
            "Close Window",
            "Edit",
            "Undo",
            "Redo",
            "Cut",
            "Copy",
            "Paste",
            "Select All",
            "View",
            "Toggle Sidebar",
            "Bot",
            "Following",
            "World",
            "Lens",
            "Schedule",
            "Zoom In",
            "Zoom Out",
            "Actual Size",
            "Enter Full Screen",
            "Window",
            "Minimize",
            "Zoom",
            "Bring All to Front",
        ]
    );
    assert_eq!(
        labels(korean),
        [
            "Loupist Dev",
            "Loupist Dev에 관하여",
            "업데이트 확인…",
            "설정…",
            "서비스",
            "Loupist Dev 가리기",
            "기타 가리기",
            "모두 보기",
            "Loupist Dev 종료",
            "파일",
            "새 리서치",
            "새 Loupist…",
            "인쇄…",
            "윈도우 닫기",
            "편집",
            "실행 취소",
            "실행 복귀",
            "오려두기",
            "복사하기",
            "붙여넣기",
            "모두 선택",
            "보기",
            "사이드바 표시 전환",
            "봇",
            "관심 자산",
            "월드",
            "렌즈",
            "예약",
            "확대",
            "축소",
            "실제 크기",
            "전체 화면 시작",
            "윈도우",
            "최소화",
            "확대/축소",
            "모두 앞으로 가져오기",
        ]
    );
    for id in [
        "about",
        "services",
        "hide",
        "hide-others",
        "show-all",
        "quit",
        "undo",
        "redo",
        "cut",
        "copy",
        "paste",
        "select-all",
        "fullscreen",
        "minimize",
        "maximize",
        "bring-all-to-front",
    ] {
        assert!(MENU
            .iter()
            .flat_map(|group| &group.items)
            .any(|row| row.id.as_deref() == Some(id) && row.role.is_some()));
    }
}

#[test]
fn changing_language_rebuilds_once_and_keeps_renderer_event_ids() {
    let original = state(MenuLanguage::En);
    let menu = InstalledMenu(Mutex::new(original));
    let mut changed = Vec::new();
    menu.update(
        |next| next.context.language = MenuLanguage::Ko,
        |next| {
            changed.push(next);
            Ok(())
        },
    )
    .unwrap();
    menu.update(
        |next| next.context.language = MenuLanguage::Ko,
        |_| {
            panic!("unchanged menu was rebuilt");
        },
    )
    .unwrap();
    assert_eq!(changed.len(), 1);
    for id in [
        "check-updates",
        "settings",
        "new-chat",
        "new-loupist",
        "print-chat",
        "toggle-sidebar",
        "chat",
        "following",
        "map",
        "lens",
        "schedule",
    ] {
        assert_eq!(action(id, original), Some(id));
        assert_eq!(action(id, changed[0]), Some(id));
    }
}

#[test]
fn actions_are_enabled_only_for_their_current_context() {
    let mut current = state(MenuLanguage::En);
    current.context.loupist = false;
    current.context.print = false;
    assert_eq!(action("new-chat", current), None);
    assert_eq!(action("print-chat", current), None);
    assert_eq!(action("map", current), None);
    assert_eq!(action("new-loupist", current), Some("new-loupist"));
    current.context.shell = false;
    assert_eq!(action("new-loupist", current), None);
    assert_eq!(action("close-window", current), Some("close-window"));
    current.context.shell = true;
    current.context.loupist = true;
    current.context.print = true;
    current.context.modal = true;
    assert_eq!(action("print-chat", current), None);
    assert_eq!(action("close-window", current), None);
    assert_eq!(action("quit", current), Some("quit"));
    current.context.modal = false;
    current.window.available = false;
    assert_eq!(action("settings", current), None);
    assert_eq!(action("zoom-in", current), None);
}

#[test]
fn native_panels_and_webview_reloads_release_their_own_menu_gates() {
    let mut current = state(MenuLanguage::Ko);
    let commands = [
        "new-chat",
        "new-loupist",
        "print-chat",
        "close-window",
        "map",
        "zoom-in",
        "fullscreen",
    ];
    current.window.native_modal = true;
    current.context.modal = true;
    assert!(commands.iter().all(|id| action(id, current).is_none()));
    current.window.native_modal = false;
    assert!(commands.iter().all(|id| action(id, current).is_none()));

    current.context.modal = false;
    assert!(commands.iter().all(|id| action(id, current).is_some()));
    current.context.modal = true;
    current.window.native_modal = true;
    current.reset_renderer_context();
    assert_eq!(current.context.language, MenuLanguage::Ko);
    assert!(commands.iter().all(|id| action(id, current).is_none()));
    current.window.native_modal = false;
    assert!(commands.iter().all(|id| action(id, current).is_some()));
}

#[test]
fn saved_webview_zoom_survives_reopen_and_limits_actions() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path();
    assert_eq!(read_zoom(path), ACTUAL_SIZE);
    write_zoom(path, 80).unwrap();
    assert_eq!(read_zoom(path), 80);
    write_zoom(path, 150).unwrap();
    assert_eq!(read_zoom(path), 150);
    assert_eq!(zoom_step(150, -1), Some(125));
    assert_eq!(zoom_step(50, -1), None);
    let mut current = state(MenuLanguage::En);
    assert_eq!(action("actual-size", current), None);
    current.zoom = 200;
    assert_eq!(action("zoom-in", current), None);
    assert_eq!(action("actual-size", current), Some("actual-size"));
    std::fs::write(path.join(ZOOM_FILE), "9999").unwrap();
    assert_eq!(read_zoom(path), ACTUAL_SIZE);
}

#[test]
fn fullscreen_role_keeps_an_enter_or_exit_title_in_both_languages() {
    let row = MENU
        .iter()
        .flat_map(|group| &group.items)
        .find(|row| row.id.as_deref() == Some("fullscreen"))
        .unwrap();
    let mut english = state(MenuLanguage::En);
    english.window.fullscreen = true;
    assert_eq!(row.label(english, "Loupist", true), "Exit Full Screen");
    let mut korean = state(MenuLanguage::Ko);
    korean.window.fullscreen = true;
    assert_eq!(row.label(korean, "Loupist", true), "전체 화면 종료");
}

#[test]
fn non_mac_window_size_labels_follow_the_window_state() {
    let row = MENU
        .iter()
        .flat_map(|group| &group.items)
        .find(|row| row.id.as_deref() == Some("maximize"))
        .unwrap();
    let mut english = state(MenuLanguage::En);
    let mut korean = state(MenuLanguage::Ko);
    assert_eq!(row.label(english, "Loupist", false), "Maximize");
    assert_eq!(row.label(korean, "Loupist", false), "최대화");
    english.window.maximized = true;
    korean.window.maximized = true;
    assert_eq!(row.label(english, "Loupist", false), "Restore");
    assert_eq!(row.label(korean, "Loupist", false), "복원");
}
