//! Integration tests for shell layout geometry, proportions, and hit-testing.

use tableminus_gfx::glyphs::EngineKind;
use tableminus_windows_app::app::{AppController, ScreenMode};
use tableminus_windows_app::i18n::Locale;
use tableminus_windows_app::shell_layout::{
    ShellHitTarget, ShellLayout, ShellLayoutConfig, metrics,
};
use tableminus_windows_app::views::status_bar::TableViewMode;
use tableminus_windows_platform::{AppSettings, WindowsTheme};

#[test]
fn shell_layout_regions_do_not_overlap_and_cover_viewport() {
    let config = ShellLayoutConfig {
        rail_expanded: false,
        sidebar_visible: true,
        sidebar_width: 220.0,
        inspector_visible: true,
        ai_drawer_visible: true,
        open_connection_count: 3,
        tab_count: 2,
        sidebar_item_count: 5,
    };
    let layout = ShellLayout::compute(1440.0, 900.0, config);

    assert_eq!(layout.rail.width, metrics::RAIL_COLLAPSED_WIDTH);
    assert_eq!(layout.sidebar.width, 220.0);
    assert_eq!(layout.toolbar.height, metrics::TOOLBAR_HEIGHT);
    assert_eq!(layout.tab_strip.height, metrics::TAB_STRIP_HEIGHT);
    assert_eq!(layout.status_bar.height, metrics::STATUS_BAR_HEIGHT);
    assert_eq!(layout.inspector.width, metrics::INSPECTOR_WIDTH);
    assert_eq!(layout.ai_drawer.width, metrics::AI_DRAWER_WIDTH);

    // Horizontal continuity: rail -> sidebar -> content -> inspector -> ai_drawer
    assert_eq!(layout.rail.right(), layout.sidebar.x);
    assert_eq!(layout.sidebar.right(), layout.content.x);
    assert_eq!(layout.content.right(), layout.inspector.x);
    assert_eq!(layout.inspector.right(), layout.ai_drawer.x);
    assert_eq!(layout.ai_drawer.right(), 1440.0);

    // Vertical continuity: toolbar -> tab_strip -> content -> status_bar
    assert_eq!(layout.toolbar.bottom(), layout.tab_strip.y);
    assert_eq!(layout.tab_strip.bottom(), layout.content.y);
    assert_eq!(layout.content.bottom(), layout.status_bar.y);
    assert_eq!(layout.status_bar.bottom(), 900.0);
}

#[test]
fn hit_testing_identifies_every_shell_control() {
    let config = ShellLayoutConfig {
        rail_expanded: true,
        sidebar_visible: true,
        sidebar_width: 220.0,
        inspector_visible: true,
        ai_drawer_visible: true,
        open_connection_count: 2,
        tab_count: 2,
        sidebar_item_count: 3,
    };
    let layout = ShellLayout::compute(1440.0, 900.0, config);

    assert_eq!(
        layout.hit_test(
            layout.rail_library_btn.x + 4.0,
            layout.rail_library_btn.y + 4.0
        ),
        ShellHitTarget::RailLibraryButton
    );
    assert_eq!(
        layout.hit_test(
            layout.rail_connections[1].x + 4.0,
            layout.rail_connections[1].y + 4.0
        ),
        ShellHitTarget::RailConnection(1)
    );
    assert_eq!(
        layout.hit_test(layout.sidebar_filter.x + 4.0, layout.sidebar_filter.y + 4.0),
        ShellHitTarget::SidebarFilterBox
    );
    assert_eq!(
        layout.hit_test(layout.tab_pills[0].0.x + 8.0, layout.tab_pills[0].0.y + 8.0),
        ShellHitTarget::Tab(0)
    );
    assert_eq!(
        layout.hit_test(layout.tab_pills[0].1.x + 4.0, layout.tab_pills[0].1.y + 4.0),
        ShellHitTarget::TabClose(0)
    );
    assert_eq!(
        layout.hit_test(layout.tab_new_btn.x + 4.0, layout.tab_new_btn.y + 4.0),
        ShellHitTarget::TabNewButton
    );
    assert_eq!(
        layout.hit_test(layout.content.x + 20.0, layout.content.y + 20.0),
        ShellHitTarget::ContentArea
    );
    assert_eq!(
        layout.hit_test(
            layout.status_structure_btn.x + 4.0,
            layout.status_structure_btn.y + 4.0
        ),
        ShellHitTarget::StatusStructureToggle
    );
    assert_eq!(layout.hit_test(-10.0, 10.0), ShellHitTarget::Outside);
}

#[test]
fn clicking_shell_buttons_updates_controller_state() {
    let mut app = AppController::new(AppSettings::default(), WindowsTheme::Light, Locale::EnUs);
    app.add_rail_connection("pg-1", "Local Postgres", EngineKind::Postgresql, true);
    let layout = app.compute_layout(1280.0, 800.0);

    // Click rail collapse toggle
    let target = app.handle_click(
        layout.rail_collapse_btn.x + 4.0,
        layout.rail_collapse_btn.y + 4.0,
    );
    assert_eq!(target, ShellHitTarget::RailCollapseButton);
    assert!(app.layout_config.rail_expanded);

    // Recompute layout after rail expansion before clicking status bar.
    let layout = app.compute_layout(1280.0, 800.0);

    // Click Structure toggle in status bar
    let target = app.handle_click(
        layout.status_structure_btn.x + 4.0,
        layout.status_structure_btn.y + 4.0,
    );
    assert_eq!(target, ShellHitTarget::StatusStructureToggle);
    assert_eq!(app.status_bar.view_mode, TableViewMode::Structure);

    // Click Settings in rail
    let target = app.handle_click(
        layout.rail_settings_btn.x + 4.0,
        layout.rail_settings_btn.y + 4.0,
    );
    assert_eq!(target, ShellHitTarget::RailSettingsButton);
    assert_eq!(app.screen_mode, ScreenMode::Settings);
}
