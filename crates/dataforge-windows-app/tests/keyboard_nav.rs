//! Integration tests for keyboard navigation, shortcut dispatch, and UIA roles.

use dataforge_windows_app::accessibility::{
    FocusRegion, KeyCommand, Modifiers, handle_wm_getobject, translate_key, uia_role_for_target, vk,
};
use dataforge_windows_app::app::{AppController, ScreenMode};
use dataforge_windows_app::i18n::Locale;
use dataforge_windows_app::shell_layout::ShellHitTarget;
use dataforge_windows_platform::{AppSettings, WindowsTheme};

#[test]
fn keyboard_shortcuts_map_accurately() {
    let ctrl = Modifiers {
        ctrl: true,
        shift: false,
        alt: false,
    };
    let ctrl_shift = Modifiers {
        ctrl: true,
        shift: true,
        alt: false,
    };
    let shift = Modifiers {
        ctrl: false,
        shift: true,
        alt: false,
    };

    assert_eq!(
        translate_key(vk::TAB, Modifiers::default()),
        Some(KeyCommand::FocusNextRegion)
    );
    assert_eq!(
        translate_key(vk::TAB, shift),
        Some(KeyCommand::FocusPrevRegion)
    );
    assert_eq!(translate_key(vk::TAB, ctrl), Some(KeyCommand::NextTab));
    assert_eq!(
        translate_key(vk::TAB, ctrl_shift),
        Some(KeyCommand::PrevTab)
    );
    assert_eq!(
        translate_key(vk::KEY_W, ctrl),
        Some(KeyCommand::CloseTabOrWindow)
    );
    assert_eq!(
        translate_key(vk::KEY_T, ctrl_shift),
        Some(KeyCommand::ReopenClosedTab)
    );
    assert_eq!(
        translate_key(vk::F5, Modifiers::default()),
        Some(KeyCommand::ExecuteQuery)
    );
    assert_eq!(
        translate_key(vk::RETURN, ctrl),
        Some(KeyCommand::ExecuteQuery)
    );
    assert_eq!(
        translate_key(vk::KEY_B, ctrl),
        Some(KeyCommand::ToggleSidebar)
    );
    assert_eq!(
        translate_key(vk::KEY_I, ctrl),
        Some(KeyCommand::ToggleInspector)
    );
    assert_eq!(
        translate_key(vk::KEY_A, ctrl_shift),
        Some(KeyCommand::ToggleAiDrawer)
    );
}

#[test]
fn tab_cycling_skips_hidden_drawers_and_respects_visibility() {
    let mut region = FocusRegion::Toolbar;
    region = region.step(false, true, false, false);
    assert_eq!(region, FocusRegion::TabStrip);
    region = region.step(false, true, false, false);
    assert_eq!(region, FocusRegion::ContentArea);
    // Inspector and AI drawer are hidden, so next is StatusBar.
    region = region.step(false, true, false, false);
    assert_eq!(region, FocusRegion::StatusBar);

    // With inspector visible, stepping backward from StatusBar lands on RowInspector.
    region = region.step(true, true, true, false);
    assert_eq!(region, FocusRegion::RowInspector);
}

#[test]
fn controller_executes_keyboard_toggles_and_navigation() {
    let mut app = AppController::new(AppSettings::default(), WindowsTheme::Light, Locale::EnUs);
    assert!(app.layout_config.sidebar_visible);
    assert!(!app.handle_key_command(KeyCommand::ToggleSidebar));
    assert!(!app.layout_config.sidebar_visible);

    assert!(!app.handle_key_command(KeyCommand::ToggleInspector));
    assert!(app.layout_config.inspector_visible);
    assert!(app.toolbar_status.inspector_open);

    assert!(!app.handle_key_command(KeyCommand::ToggleAiDrawer));
    assert!(app.layout_config.ai_drawer_visible);
    assert!(app.toolbar_status.ai_open);

    assert!(!app.handle_key_command(KeyCommand::OpenSettings));
    assert_eq!(app.screen_mode, ScreenMode::Settings);

    assert!(!app.handle_key_command(KeyCommand::OpenConnectionsLibrary));
    assert_eq!(app.screen_mode, ScreenMode::ConnectionsLibrary);
}

#[test]
fn uia_wm_getobject_and_target_roles_are_well_defined() {
    assert_eq!(handle_wm_getobject(-4), Some(0));
    assert_eq!(handle_wm_getobject(-25), Some(0));
    assert_eq!(handle_wm_getobject(0), None);

    assert_eq!(uia_role_for_target(ShellHitTarget::Tab(0)), "TabItem");
    assert_eq!(
        uia_role_for_target(ShellHitTarget::SidebarFilterBox),
        "Edit"
    );
    assert_eq!(
        uia_role_for_target(ShellHitTarget::RailConnection(0)),
        "TreeItem"
    );
    assert_eq!(
        uia_role_for_target(ShellHitTarget::StatusDataToggle),
        "Button"
    );
}
