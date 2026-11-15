//! Shell layout geometry and pixel-to-target hit-testing for TableMinus.
//!
//! Computes exact DIP coordinates for the 6-region desktop shell matching
//! `.github/assets/app-light.png` and `.github/assets/app-dark.png`:
//! Connection Rail, Object Sidebar, Top Toolbar, Pill Tab Strip, Content Area
//! (with optional Row Inspector and AI Assistant drawers), and Bottom Status Bar.

/// Axis-aligned rectangle in device-independent pixels (DIPs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    /// Creates a rectangle with non-negative width and height.
    #[must_use]
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width: if width < 0.0 { 0.0 } else { width },
            height: if height < 0.0 { 0.0 } else { height },
        }
    }

    /// Empty zero-sized rectangle.
    pub const ZERO: Self = Self {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };

    /// Right edge (`x + width`).
    #[must_use]
    pub const fn right(self) -> f32 {
        self.x + self.width
    }

    /// Bottom edge (`y + height`).
    #[must_use]
    pub const fn bottom(self) -> f32 {
        self.y + self.height
    }

    /// Returns `true` if `(px, py)` lies inside `[x, right) x [y, bottom)`.
    #[must_use]
    pub fn contains(self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }
}

/// Configuration knobs controlling shell panel visibility and dimensions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShellLayoutConfig {
    pub rail_expanded: bool,
    pub sidebar_visible: bool,
    pub sidebar_width: f32,
    pub inspector_visible: bool,
    pub ai_drawer_visible: bool,
    pub open_connection_count: usize,
    pub tab_count: usize,
    pub sidebar_item_count: usize,
}

impl Default for ShellLayoutConfig {
    fn default() -> Self {
        Self {
            rail_expanded: false,
            sidebar_visible: true,
            sidebar_width: 220.0,
            inspector_visible: false,
            ai_drawer_visible: false,
            open_connection_count: 0,
            tab_count: 1,
            sidebar_item_count: 0,
        }
    }
}

/// Canonical layout constants in DIPs.
pub mod metrics {
    pub const RAIL_COLLAPSED_WIDTH: f32 = 48.0;
    pub const RAIL_EXPANDED_WIDTH: f32 = 180.0;
    pub const MIN_SIDEBAR_WIDTH: f32 = 160.0;
    pub const MAX_SIDEBAR_WIDTH: f32 = 360.0;
    pub const TOOLBAR_HEIGHT: f32 = 42.0;
    pub const TAB_STRIP_HEIGHT: f32 = 36.0;
    pub const STATUS_BAR_HEIGHT: f32 = 28.0;
    pub const INSPECTOR_WIDTH: f32 = 300.0;
    pub const AI_DRAWER_WIDTH: f32 = 320.0;
    pub const RAIL_ITEM_HEIGHT: f32 = 36.0;
    pub const SIDEBAR_FILTER_HEIGHT: f32 = 32.0;
    pub const SIDEBAR_TOGGLE_HEIGHT: f32 = 28.0;
    pub const SIDEBAR_ROW_HEIGHT: f32 = 26.0;
    pub const TAB_PILL_WIDTH: f32 = 148.0;
    pub const TAB_CLOSE_WIDTH: f32 = 20.0;
    pub const TAB_NEW_BUTTON_WIDTH: f32 = 28.0;
}

/// Computed rectangles for every major region of the shell.
#[derive(Debug, Clone, PartialEq)]
pub struct ShellLayout {
    pub viewport: Rect,
    pub rail: Rect,
    pub rail_library_btn: Rect,
    pub rail_collapse_btn: Rect,
    pub rail_settings_btn: Rect,
    pub rail_connections: Vec<Rect>,
    pub sidebar: Rect,
    pub sidebar_filter: Rect,
    pub sidebar_mode_toggle: Rect,
    pub sidebar_rows: Vec<Rect>,
    pub toolbar: Rect,
    pub toolbar_connect_btn: Rect,
    pub toolbar_refresh_btn: Rect,
    pub toolbar_apply_btn: Rect,
    pub toolbar_status_pill: Rect,
    pub toolbar_inspector_btn: Rect,
    pub toolbar_ai_btn: Rect,
    pub tab_strip: Rect,
    pub tab_pills: Vec<(Rect, Rect)>,
    pub tab_new_btn: Rect,
    pub content: Rect,
    pub inspector: Rect,
    pub ai_drawer: Rect,
    pub status_bar: Rect,
    pub status_data_btn: Rect,
    pub status_structure_btn: Rect,
    pub status_columns_btn: Rect,
    pub status_filters_btn: Rect,
    pub status_prev_page_btn: Rect,
    pub status_next_page_btn: Rect,
    pub status_settings_btn: Rect,
}

/// Semantic target identified by [`ShellLayout::hit_test`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellHitTarget {
    RailLibraryButton,
    RailCollapseButton,
    RailSettingsButton,
    RailConnection(usize),
    SidebarFilterBox,
    SidebarFavoritesToggle,
    SidebarItem(usize),
    ToolbarConnectButton,
    ToolbarRefreshButton,
    ToolbarApplyButton,
    ToolbarStatusPill,
    ToolbarInspectorButton,
    ToolbarAiButton,
    Tab(usize),
    TabClose(usize),
    TabNewButton,
    ContentArea,
    RowInspector,
    AiDrawer,
    StatusDataToggle,
    StatusStructureToggle,
    StatusColumnsButton,
    StatusFiltersButton,
    StatusPrevPage,
    StatusNextPage,
    StatusSettingsButton,
    Outside,
}

impl ShellLayout {
    /// Computes the complete shell layout for a client area of `width x height` DIPs.
    #[must_use]
    pub fn compute(width: f32, height: f32, config: ShellLayoutConfig) -> Self {
        let viewport = Rect::new(0.0, 0.0, width.max(1.0), height.max(1.0));
        let rail_w = if config.rail_expanded {
            metrics::RAIL_EXPANDED_WIDTH
        } else {
            metrics::RAIL_COLLAPSED_WIDTH
        };
        let rail = Rect::new(0.0, 0.0, rail_w.min(viewport.width), viewport.height);
        let (rail_library_btn, rail_collapse_btn, rail_settings_btn, rail_connections) =
            compute_rail_items(rail, config.open_connection_count);

        let rest_x = rail.right();
        let rest_w = (viewport.width - rest_x).max(0.0);

        let toolbar = Rect::new(rest_x, 0.0, rest_w, metrics::TOOLBAR_HEIGHT);
        let (
            toolbar_connect_btn,
            toolbar_refresh_btn,
            toolbar_apply_btn,
            toolbar_status_pill,
            toolbar_inspector_btn,
            toolbar_ai_btn,
        ) = compute_toolbar_items(toolbar);

        let status_y = (viewport.height - metrics::STATUS_BAR_HEIGHT).max(toolbar.bottom());
        let status_bar = Rect::new(rest_x, status_y, rest_w, metrics::STATUS_BAR_HEIGHT);
        let (
            status_data_btn,
            status_structure_btn,
            status_columns_btn,
            status_filters_btn,
            status_prev_page_btn,
            status_next_page_btn,
            status_settings_btn,
        ) = compute_status_bar_items(status_bar);

        let body_y = toolbar.bottom();
        let body_h = (status_bar.y - body_y).max(0.0);

        let sidebar_w = if config.sidebar_visible {
            config
                .sidebar_width
                .clamp(metrics::MIN_SIDEBAR_WIDTH, metrics::MAX_SIDEBAR_WIDTH)
                .min(rest_w * 0.45)
        } else {
            0.0
        };
        let sidebar = Rect::new(rest_x, body_y, sidebar_w, body_h);
        let (sidebar_filter, sidebar_mode_toggle, sidebar_rows) =
            compute_sidebar_items(sidebar, config.sidebar_item_count);

        let main_x = sidebar.right();
        let main_w = (viewport.width - main_x).max(0.0);
        let tab_strip = Rect::new(main_x, body_y, main_w, metrics::TAB_STRIP_HEIGHT);
        let (tab_pills, tab_new_btn) = compute_tab_items(tab_strip, config.tab_count);

        let content_y = tab_strip.bottom();
        let content_h = (status_bar.y - content_y).max(0.0);
        let (content, inspector, ai_drawer) = compute_content_and_drawers(
            main_x,
            content_y,
            main_w,
            content_h,
            config.inspector_visible,
            config.ai_drawer_visible,
        );

        Self {
            viewport,
            rail,
            rail_library_btn,
            rail_collapse_btn,
            rail_settings_btn,
            rail_connections,
            sidebar,
            sidebar_filter,
            sidebar_mode_toggle,
            sidebar_rows,
            toolbar,
            toolbar_connect_btn,
            toolbar_refresh_btn,
            toolbar_apply_btn,
            toolbar_status_pill,
            toolbar_inspector_btn,
            toolbar_ai_btn,
            tab_strip,
            tab_pills,
            tab_new_btn,
            content,
            inspector,
            ai_drawer,
            status_bar,
            status_data_btn,
            status_structure_btn,
            status_columns_btn,
            status_filters_btn,
            status_prev_page_btn,
            status_next_page_btn,
            status_settings_btn,
        }
    }

    /// Maps a DIP cursor coordinate `(px, py)` to its semantic [`ShellHitTarget`].
    #[must_use]
    pub fn hit_test(&self, px: f32, py: f32) -> ShellHitTarget {
        if !self.viewport.contains(px, py) {
            return ShellHitTarget::Outside;
        }
        if self.rail.contains(px, py) {
            return self.hit_test_rail(px, py);
        }
        if self.toolbar.contains(px, py) {
            return self.hit_test_toolbar(px, py);
        }
        if self.status_bar.contains(px, py) {
            return self.hit_test_status_bar(px, py);
        }
        if self.sidebar.contains(px, py) {
            return self.hit_test_sidebar(px, py);
        }
        if self.tab_strip.contains(px, py) {
            return self.hit_test_tabs(px, py);
        }
        if self.ai_drawer.contains(px, py) {
            return ShellHitTarget::AiDrawer;
        }
        if self.inspector.contains(px, py) {
            return ShellHitTarget::RowInspector;
        }
        if self.content.contains(px, py) {
            return ShellHitTarget::ContentArea;
        }
        ShellHitTarget::Outside
    }

    fn hit_test_rail(&self, px: f32, py: f32) -> ShellHitTarget {
        if self.rail_library_btn.contains(px, py) {
            return ShellHitTarget::RailLibraryButton;
        }
        if self.rail_collapse_btn.contains(px, py) {
            return ShellHitTarget::RailCollapseButton;
        }
        if self.rail_settings_btn.contains(px, py) {
            return ShellHitTarget::RailSettingsButton;
        }
        for (idx, rect) in self.rail_connections.iter().enumerate() {
            if rect.contains(px, py) {
                return ShellHitTarget::RailConnection(idx);
            }
        }
        ShellHitTarget::Outside
    }

    fn hit_test_toolbar(&self, px: f32, py: f32) -> ShellHitTarget {
        if self.toolbar_connect_btn.contains(px, py) {
            return ShellHitTarget::ToolbarConnectButton;
        }
        if self.toolbar_refresh_btn.contains(px, py) {
            return ShellHitTarget::ToolbarRefreshButton;
        }
        if self.toolbar_apply_btn.contains(px, py) {
            return ShellHitTarget::ToolbarApplyButton;
        }
        if self.toolbar_status_pill.contains(px, py) {
            return ShellHitTarget::ToolbarStatusPill;
        }
        if self.toolbar_inspector_btn.contains(px, py) {
            return ShellHitTarget::ToolbarInspectorButton;
        }
        if self.toolbar_ai_btn.contains(px, py) {
            return ShellHitTarget::ToolbarAiButton;
        }
        ShellHitTarget::Outside
    }

    fn hit_test_sidebar(&self, px: f32, py: f32) -> ShellHitTarget {
        if self.sidebar_filter.contains(px, py) {
            return ShellHitTarget::SidebarFilterBox;
        }
        if self.sidebar_mode_toggle.contains(px, py) {
            return ShellHitTarget::SidebarFavoritesToggle;
        }
        for (idx, rect) in self.sidebar_rows.iter().enumerate() {
            if rect.contains(px, py) {
                return ShellHitTarget::SidebarItem(idx);
            }
        }
        ShellHitTarget::Outside
    }

    fn hit_test_tabs(&self, px: f32, py: f32) -> ShellHitTarget {
        for (idx, (pill, close)) in self.tab_pills.iter().enumerate() {
            if close.contains(px, py) {
                return ShellHitTarget::TabClose(idx);
            }
            if pill.contains(px, py) {
                return ShellHitTarget::Tab(idx);
            }
        }
        if self.tab_new_btn.contains(px, py) {
            return ShellHitTarget::TabNewButton;
        }
        ShellHitTarget::Outside
    }

    fn hit_test_status_bar(&self, px: f32, py: f32) -> ShellHitTarget {
        if self.status_data_btn.contains(px, py) {
            return ShellHitTarget::StatusDataToggle;
        }
        if self.status_structure_btn.contains(px, py) {
            return ShellHitTarget::StatusStructureToggle;
        }
        if self.status_columns_btn.contains(px, py) {
            return ShellHitTarget::StatusColumnsButton;
        }
        if self.status_filters_btn.contains(px, py) {
            return ShellHitTarget::StatusFiltersButton;
        }
        if self.status_prev_page_btn.contains(px, py) {
            return ShellHitTarget::StatusPrevPage;
        }
        if self.status_next_page_btn.contains(px, py) {
            return ShellHitTarget::StatusNextPage;
        }
        if self.status_settings_btn.contains(px, py) {
            return ShellHitTarget::StatusSettingsButton;
        }
        ShellHitTarget::Outside
    }
}

fn compute_rail_items(rail: Rect, connection_count: usize) -> (Rect, Rect, Rect, Vec<Rect>) {
    let pad = 6.0;
    let w = (rail.width - pad * 2.0).max(0.0);
    let library_btn = Rect::new(rail.x + pad, rail.y + 8.0, w, metrics::RAIL_ITEM_HEIGHT);
    let settings_btn = Rect::new(
        rail.x + pad,
        (rail.bottom() - metrics::RAIL_ITEM_HEIGHT - 8.0).max(library_btn.bottom()),
        w,
        metrics::RAIL_ITEM_HEIGHT,
    );
    let collapse_btn = Rect::new(
        rail.x + pad,
        (settings_btn.y - metrics::RAIL_ITEM_HEIGHT - 4.0).max(library_btn.bottom()),
        w,
        metrics::RAIL_ITEM_HEIGHT,
    );

    let mut connections = Vec::with_capacity(connection_count);
    let mut y = library_btn.bottom() + 8.0;
    for _ in 0..connection_count {
        if y + metrics::RAIL_ITEM_HEIGHT > collapse_btn.y {
            break;
        }
        connections.push(Rect::new(rail.x + pad, y, w, metrics::RAIL_ITEM_HEIGHT));
        y += metrics::RAIL_ITEM_HEIGHT + 4.0;
    }

    (library_btn, collapse_btn, settings_btn, connections)
}

fn compute_toolbar_items(toolbar: Rect) -> (Rect, Rect, Rect, Rect, Rect, Rect) {
    let btn_y = toolbar.y + 7.0;
    let btn_h = (toolbar.height - 14.0).max(0.0);
    let btn_w = 28.0;
    let connect_btn = Rect::new(toolbar.x + 10.0, btn_y, btn_w, btn_h);
    let refresh_btn = Rect::new(connect_btn.right() + 6.0, btn_y, btn_w, btn_h);
    let apply_btn = Rect::new(refresh_btn.right() + 6.0, btn_y, btn_w, btn_h);

    let ai_btn = Rect::new((toolbar.right() - 38.0).max(toolbar.x), btn_y, btn_w, btn_h);
    let inspector_btn = Rect::new((ai_btn.x - 34.0).max(toolbar.x), btn_y, btn_w, btn_h);

    let pill_w = 340.0_f32.min((inspector_btn.x - apply_btn.right() - 24.0).max(0.0));
    let pill_x = toolbar.x + ((toolbar.width - pill_w) * 0.5).max(0.0);
    let status_pill = Rect::new(
        pill_x,
        toolbar.y + 6.0,
        pill_w,
        (toolbar.height - 12.0).max(0.0),
    );

    (
        connect_btn,
        refresh_btn,
        apply_btn,
        status_pill,
        inspector_btn,
        ai_btn,
    )
}

fn compute_sidebar_items(sidebar: Rect, item_count: usize) -> (Rect, Rect, Vec<Rect>) {
    if sidebar.width <= 0.0 || sidebar.height <= 0.0 {
        return (Rect::ZERO, Rect::ZERO, Vec::new());
    }
    let pad = 8.0;
    let inner_w = (sidebar.width - pad * 2.0).max(0.0);
    let filter = Rect::new(
        sidebar.x + pad,
        sidebar.y + 8.0,
        inner_w,
        metrics::SIDEBAR_FILTER_HEIGHT,
    );
    let toggle = Rect::new(
        sidebar.x + pad,
        filter.bottom() + 6.0,
        inner_w,
        metrics::SIDEBAR_TOGGLE_HEIGHT,
    );

    let mut rows = Vec::with_capacity(item_count);
    let mut y = toggle.bottom() + 8.0;
    for _ in 0..item_count {
        if y + metrics::SIDEBAR_ROW_HEIGHT > sidebar.bottom() {
            break;
        }
        rows.push(Rect::new(
            sidebar.x + 4.0,
            y,
            (sidebar.width - 8.0).max(0.0),
            metrics::SIDEBAR_ROW_HEIGHT,
        ));
        y += metrics::SIDEBAR_ROW_HEIGHT + 2.0;
    }
    (filter, toggle, rows)
}

fn compute_tab_items(tab_strip: Rect, tab_count: usize) -> (Vec<(Rect, Rect)>, Rect) {
    let mut pills = Vec::with_capacity(tab_count);
    let mut x = tab_strip.x + 8.0;
    let y = tab_strip.y + 4.0;
    let h = (tab_strip.height - 8.0).max(0.0);
    let avail = (tab_strip.width - 48.0).max(0.0);
    let pill_w = if tab_count > 0 {
        (avail / tab_count as f32).clamp(64.0, metrics::TAB_PILL_WIDTH)
    } else {
        metrics::TAB_PILL_WIDTH
    };

    for _ in 0..tab_count {
        if x + pill_w > tab_strip.right() - 36.0 {
            break;
        }
        let pill = Rect::new(x, y, pill_w, h);
        let close = Rect::new(
            (pill.right() - metrics::TAB_CLOSE_WIDTH - 2.0).max(pill.x),
            y + 2.0,
            metrics::TAB_CLOSE_WIDTH,
            (h - 4.0).max(0.0),
        );
        pills.push((pill, close));
        x += pill_w + 4.0;
    }

    let new_btn = Rect::new(x, y, metrics::TAB_NEW_BUTTON_WIDTH, h);
    (pills, new_btn)
}

fn compute_content_and_drawers(
    x: f32,
    y: f32,
    total_w: f32,
    h: f32,
    inspector_visible: bool,
    ai_drawer_visible: bool,
) -> (Rect, Rect, Rect) {
    let ai_w = if ai_drawer_visible && total_w > metrics::AI_DRAWER_WIDTH + 200.0 {
        metrics::AI_DRAWER_WIDTH
    } else {
        0.0
    };
    let ai_drawer = if ai_w > 0.0 {
        Rect::new(x + total_w - ai_w, y, ai_w, h)
    } else {
        Rect::ZERO
    };

    let rem_w = (total_w - ai_w).max(0.0);
    let insp_w = if inspector_visible && rem_w > metrics::INSPECTOR_WIDTH + 200.0 {
        metrics::INSPECTOR_WIDTH
    } else {
        0.0
    };
    let inspector = if insp_w > 0.0 {
        Rect::new(x + rem_w - insp_w, y, insp_w, h)
    } else {
        Rect::ZERO
    };

    let content = Rect::new(x, y, (rem_w - insp_w).max(0.0), h);
    (content, inspector, ai_drawer)
}

fn compute_status_bar_items(bar: Rect) -> (Rect, Rect, Rect, Rect, Rect, Rect, Rect) {
    let y = bar.y + 3.0;
    let h = (bar.height - 6.0).max(0.0);
    let data_btn = Rect::new(bar.x + 8.0, y, 54.0, h);
    let structure_btn = Rect::new(data_btn.right() + 2.0, y, 72.0, h);
    let columns_btn = Rect::new(structure_btn.right() + 12.0, y, 68.0, h);
    let filters_btn = Rect::new(columns_btn.right() + 6.0, y, 60.0, h);

    let settings_btn = Rect::new((bar.right() - 32.0).max(bar.x), y, 24.0, h);
    let next_page_btn = Rect::new((settings_btn.x - 32.0).max(bar.x), y, 24.0, h);
    let prev_page_btn = Rect::new((next_page_btn.x - 86.0).max(bar.x), y, 24.0, h);

    (
        data_btn,
        structure_btn,
        columns_btn,
        filters_btn,
        prev_page_btn,
        next_page_btn,
        settings_btn,
    )
}
