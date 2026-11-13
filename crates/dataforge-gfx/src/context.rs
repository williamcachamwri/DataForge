//! Direct2D and DirectWrite device context, render surface, and resource caches.
//!
//! Designed around a strict `< 60 MB` GPU/CPU cache memory discipline with
//! bounded brush and text-format caches and explicit `D2DERR_RECREATE_TARGET`
//! recovery.

use std::collections::HashMap;

use windows::Win32::Foundation::{D2DERR_RECREATE_TARGET, HWND};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
    D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT,
    D2D1CreateFactory, ID2D1Factory1, ID2D1Geometry, ID2D1HwndRenderTarget, ID2D1PathGeometry,
    ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_ITALIC,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_FONT_WEIGHT_MEDIUM, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TEXT_ALIGNMENT_TRAILING, DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat,
    IDWriteTextLayout,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::core::HSTRING;
use windows_numerics::{Matrix3x2, Vector2};

use crate::error::{GfxError, Result};
use crate::palette::Color;

/// Default memory budget for cached GPU/CPU resources (`60 MB`).
pub const DEFAULT_MEMORY_BUDGET_BYTES: usize = 60 * 1024 * 1024;

/// Maximum number of solid-color brushes retained in [`RenderSurface`].
const MAX_CACHED_BRUSHES: usize = 256;

/// Maximum number of DirectWrite text formats retained in [`RenderSurface`].
const MAX_CACHED_TEXT_FORMATS: usize = 64;

/// Standard baseline DPI on Windows (`96.0`).
pub const BASELINE_DPI: f32 = 96.0;

/// Font weight selector for DirectWrite text formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontWeight {
    /// Normal weight (`400`).
    Normal,
    /// Medium weight (`500`).
    Medium,
    /// Semi-bold weight (`600`).
    SemiBold,
    /// Bold weight (`700`).
    Bold,
}

impl FontWeight {
    const fn to_dwrite(self) -> DWRITE_FONT_WEIGHT {
        match self {
            Self::Normal => DWRITE_FONT_WEIGHT_NORMAL,
            Self::Medium => DWRITE_FONT_WEIGHT_MEDIUM,
            Self::SemiBold => DWRITE_FONT_WEIGHT_SEMI_BOLD,
            Self::Bold => DWRITE_FONT_WEIGHT_BOLD,
        }
    }
}

/// Horizontal text alignment within a layout box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextAlign {
    /// Left-aligned (in LTR).
    Leading,
    /// Horizontally centered.
    Center,
    /// Right-aligned (in LTR).
    Trailing,
}

/// Vertical text alignment within a layout box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VerticalAlign {
    /// Top-aligned.
    Top,
    /// Vertically centered.
    Center,
}

/// Specification for a cached `IDWriteTextFormat`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TextFormatSpec {
    /// Font family name (e.g. `"Segoe UI"` or `"Cascadia Mono"`).
    pub family: String,
    /// Font size in DIPs, quantized to tenths of a DIP (`size * 10.0`).
    pub size_tenths: u32,
    /// Font weight.
    pub weight: FontWeight,
    /// Italic style flag.
    pub italic: bool,
    /// Horizontal alignment.
    pub align: TextAlign,
    /// Vertical alignment.
    pub valign: VerticalAlign,
}

impl TextFormatSpec {
    /// Creates a new [`TextFormatSpec`] with leading/top alignment.
    #[must_use]
    pub fn new(family: impl Into<String>, size_dip: f32) -> Self {
        Self {
            family: family.into(),
            size_tenths: (size_dip.max(1.0) * 10.0).round() as u32,
            weight: FontWeight::Normal,
            italic: false,
            align: TextAlign::Leading,
            valign: VerticalAlign::Top,
        }
    }

    /// Sets the font weight.
    #[must_use]
    pub const fn with_weight(mut self, weight: FontWeight) -> Self {
        self.weight = weight;
        self
    }

    /// Sets italic style.
    #[must_use]
    pub const fn with_italic(mut self, italic: bool) -> Self {
        self.italic = italic;
        self
    }

    /// Sets horizontal alignment.
    #[must_use]
    pub const fn with_align(mut self, align: TextAlign) -> Self {
        self.align = align;
        self
    }

    /// Sets vertical alignment.
    #[must_use]
    pub const fn with_valign(mut self, valign: VerticalAlign) -> Self {
        self.valign = valign;
        self
    }

    /// Returns the font size in DIPs.
    #[must_use]
    pub fn size_dip(&self) -> f32 {
        self.size_tenths as f32 / 10.0
    }
}

/// Estimated memory footprint tracker for `< 60 MB` resource discipline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryBudget {
    /// Maximum allowed bytes.
    pub limit_bytes: usize,
    /// Estimated swapchain / render-target bytes.
    pub surface_bytes: usize,
    /// Estimated cached resource bytes (brushes, formats, line layouts).
    pub cache_bytes: usize,
}

impl MemoryBudget {
    /// Creates a new tracker with [`DEFAULT_MEMORY_BUDGET_BYTES`].
    #[must_use]
    pub const fn new(limit_bytes: usize) -> Self {
        Self {
            limit_bytes,
            surface_bytes: 0,
            cache_bytes: 0,
        }
    }

    /// Total estimated bytes currently accounted for.
    #[must_use]
    pub const fn total_bytes(&self) -> usize {
        self.surface_bytes.saturating_add(self.cache_bytes)
    }

    /// Returns `true` if total estimated bytes exceed `limit_bytes`.
    #[must_use]
    pub const fn is_over_budget(&self) -> bool {
        self.total_bytes() > self.limit_bytes
    }

    /// Updates surface byte estimate for a double-buffered BGRA8 target.
    pub const fn set_surface_dimensions(&mut self, width: u32, height: u32) {
        self.surface_bytes = (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(8);
    }
}

/// Shared Direct2D and DirectWrite factory context.
///
/// Created once per UI thread and used to construct [`RenderSurface`] instances,
/// `ID2D1PathGeometry` glyphs, and `IDWriteTextLayout` objects.
pub struct D2dContext {
    d2d_factory: ID2D1Factory1,
    dwrite_factory: IDWriteFactory,
}

impl D2dContext {
    /// Initializes the Direct2D (`ID2D1Factory1`) and DirectWrite (`IDWriteFactory`) factories.
    ///
    /// # Errors
    /// Returns [`GfxError::Win32`] if either factory fails to initialize.
    pub fn new() -> Result<Self> {
        // SAFETY: `D2D1CreateFactory` with `SINGLE_THREADED` and `None` options
        // initializes a standard COM Direct2D 1.1 factory.
        let d2d_factory: ID2D1Factory1 =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)? };

        // SAFETY: `DWriteCreateFactory` with `SHARED` initializes the DirectWrite factory.
        let dwrite_factory: IDWriteFactory =
            unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };

        Ok(Self {
            d2d_factory,
            dwrite_factory,
        })
    }

    /// Returns a reference to the underlying `ID2D1Factory1`.
    #[must_use]
    pub const fn d2d_factory(&self) -> &ID2D1Factory1 {
        &self.d2d_factory
    }

    /// Returns a reference to the underlying `IDWriteFactory`.
    #[must_use]
    pub const fn dwrite_factory(&self) -> &IDWriteFactory {
        &self.dwrite_factory
    }

    /// Creates a new empty `ID2D1PathGeometry`.
    ///
    /// # Errors
    /// Returns [`GfxError::Win32`] if geometry creation fails.
    pub fn create_path_geometry(&self) -> Result<ID2D1PathGeometry> {
        // SAFETY: `d2d_factory` is a valid initialized `ID2D1Factory1`.
        let geom = unsafe { self.d2d_factory.CreatePathGeometry()? };
        Ok(geom.into())
    }

    /// Creates a DirectWrite `IDWriteTextFormat` from [`TextFormatSpec`].
    ///
    /// # Errors
    /// Returns [`GfxError::Win32`] if DirectWrite rejects the font specification.
    pub fn create_text_format(&self, spec: &TextFormatSpec) -> Result<IDWriteTextFormat> {
        let family = HSTRING::from(spec.family.as_str());
        let locale = HSTRING::from("en-us");
        let style = if spec.italic {
            DWRITE_FONT_STYLE_ITALIC
        } else {
            DWRITE_FONT_STYLE_NORMAL
        };

        // SAFETY: All strings are valid null-terminated HSTRINGs and `dwrite_factory` is valid.
        let format = unsafe {
            self.dwrite_factory.CreateTextFormat(
                &family,
                None,
                spec.weight.to_dwrite(),
                style,
                DWRITE_FONT_STRETCH_NORMAL,
                spec.size_dip(),
                &locale,
            )?
        };

        let halign = match spec.align {
            TextAlign::Leading => DWRITE_TEXT_ALIGNMENT_LEADING,
            TextAlign::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
            TextAlign::Trailing => DWRITE_TEXT_ALIGNMENT_TRAILING,
        };
        let valign = match spec.valign {
            VerticalAlign::Top => DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
            VerticalAlign::Center => DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
        };

        // SAFETY: `format` is a newly created valid `IDWriteTextFormat`.
        unsafe {
            format.SetTextAlignment(halign)?;
            format.SetParagraphAlignment(valign)?;
        }

        Ok(format)
    }

    /// Creates an `IDWriteTextLayout` for a single string within `max_width x max_height` DIPs.
    ///
    /// # Errors
    /// Returns [`GfxError::Win32`] if layout creation fails.
    pub fn create_text_layout(
        &self,
        text: &str,
        format: &IDWriteTextFormat,
        max_width: f32,
        max_height: f32,
    ) -> Result<IDWriteTextLayout> {
        let wide: Vec<u16> = text.encode_utf16().collect();
        // SAFETY: `wide` slice is valid for the duration of `CreateTextLayout`.
        let layout = unsafe {
            self.dwrite_factory.CreateTextLayout(
                &wide,
                format,
                max_width.max(1.0),
                max_height.max(1.0),
            )?
        };
        Ok(layout)
    }

    /// Creates a hardware-accelerated [`RenderSurface`] bound to a Win32 `HWND`.
    ///
    /// # Errors
    /// Returns [`GfxError::InvalidSurfaceMetrics`] if `dpi <= 0.0`, or [`GfxError::Win32`]
    /// if Direct2D fails to create the render target.
    pub fn create_hwnd_surface(
        &self,
        hwnd: HWND,
        width: u32,
        height: u32,
        dpi: f32,
    ) -> Result<RenderSurface> {
        if dpi <= 0.0 || !dpi.is_finite() {
            return Err(GfxError::InvalidSurfaceMetrics { width, height, dpi });
        }

        let safe_w = width.max(1);
        let safe_h = height.max(1);

        let render_props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: dpi,
            dpiY: dpi,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };

        let hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES {
            hwnd,
            pixelSize: D2D_SIZE_U {
                width: safe_w,
                height: safe_h,
            },
            presentOptions: D2D1_PRESENT_OPTIONS_NONE,
        };

        // SAFETY: `hwnd` is provided by the caller's window; `render_props` and `hwnd_props`
        // are valid stack structs.
        let target = unsafe {
            self.d2d_factory
                .CreateHwndRenderTarget(&render_props, &hwnd_props)?
        };

        let mut budget = MemoryBudget::new(DEFAULT_MEMORY_BUDGET_BYTES);
        budget.set_surface_dimensions(safe_w, safe_h);

        Ok(RenderSurface {
            target,
            dwrite_factory: self.dwrite_factory.clone(),
            width_px: safe_w,
            height_px: safe_h,
            dpi,
            brushes: HashMap::new(),
            text_formats: HashMap::new(),
            budget,
        })
    }
}

/// Per-window Direct2D render target with DPI scaling and bounded resource caches.
pub struct RenderSurface {
    target: ID2D1HwndRenderTarget,
    dwrite_factory: IDWriteFactory,
    width_px: u32,
    height_px: u32,
    dpi: f32,
    brushes: HashMap<u32, ID2D1SolidColorBrush>,
    text_formats: HashMap<TextFormatSpec, IDWriteTextFormat>,
    budget: MemoryBudget,
}

impl RenderSurface {
    /// Returns the physical pixel width.
    #[must_use]
    pub const fn width_px(&self) -> u32 {
        self.width_px
    }

    /// Returns the physical pixel height.
    #[must_use]
    pub const fn height_px(&self) -> u32 {
        self.height_px
    }

    /// Returns the current DPI (`96.0` = 100% scale).
    #[must_use]
    pub const fn dpi(&self) -> f32 {
        self.dpi
    }

    /// Returns the DPI scale factor (`dpi / 96.0`).
    #[must_use]
    pub fn scale_factor(&self) -> f32 {
        self.dpi / BASELINE_DPI
    }

    /// Returns the logical width in DIPs.
    #[must_use]
    pub fn width_dip(&self) -> f32 {
        self.width_px as f32 / self.scale_factor()
    }

    /// Returns the logical height in DIPs.
    #[must_use]
    pub fn height_dip(&self) -> f32 {
        self.height_px as f32 / self.scale_factor()
    }

    /// Returns the current memory budget telemetry.
    #[must_use]
    pub const fn memory_budget(&self) -> MemoryBudget {
        self.budget
    }

    /// Updates the surface pixel size on `WM_SIZE`.
    ///
    /// # Errors
    /// Returns [`GfxError::Win32`] if Direct2D fails to resize the swapchain.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        let safe_w = width.max(1);
        let safe_h = height.max(1);
        let size = D2D_SIZE_U {
            width: safe_w,
            height: safe_h,
        };
        // SAFETY: `target` is a valid `ID2D1HwndRenderTarget`.
        unsafe { self.target.Resize(&size)? };
        self.width_px = safe_w;
        self.height_px = safe_h;
        self.budget.set_surface_dimensions(safe_w, safe_h);
        if self.budget.is_over_budget() {
            self.trim_caches();
        }
        Ok(())
    }

    /// Updates the surface DPI on `WM_DPICHANGED`.
    ///
    /// # Errors
    /// Returns [`GfxError::InvalidSurfaceMetrics`] if `dpi <= 0.0`.
    pub fn set_dpi(&mut self, dpi: f32) -> Result<()> {
        if dpi <= 0.0 || !dpi.is_finite() {
            return Err(GfxError::InvalidSurfaceMetrics {
                width: self.width_px,
                height: self.height_px,
                dpi,
            });
        }
        // SAFETY: `target` is a valid `ID2D1HwndRenderTarget`.
        unsafe { self.target.SetDpi(dpi, dpi) };
        self.dpi = dpi;
        Ok(())
    }

    /// Clears cached brushes and text formats to stay within the memory budget.
    pub fn trim_caches(&mut self) {
        self.brushes.clear();
        self.text_formats.clear();
        self.budget.cache_bytes = 0;
    }

    /// Begins a Direct2D drawing frame.
    pub fn begin_draw(&self) {
        // SAFETY: `target` is a valid `ID2D1HwndRenderTarget`.
        unsafe { self.target.BeginDraw() };
    }

    /// Ends a Direct2D drawing frame and presents it.
    ///
    /// # Errors
    /// Returns [`GfxError::TargetLost`] on `D2DERR_RECREATE_TARGET`, or [`GfxError::Win32`]
    /// on other Direct2D errors.
    pub fn end_draw(&mut self) -> Result<()> {
        // SAFETY: Called after `begin_draw`.
        let result = unsafe { self.target.EndDraw(None, None) };
        match result {
            Ok(()) => Ok(()),
            Err(err) if err.code() == D2DERR_RECREATE_TARGET => {
                self.trim_caches();
                Err(GfxError::TargetLost)
            }
            Err(err) => Err(GfxError::Win32(err)),
        }
    }

    /// Clears the entire render target with a solid color.
    pub fn clear(&self, color: Color) {
        let d2d_color: D2D1_COLOR_F = color.to_d2d();
        // SAFETY: Called inside an active `begin_draw`..`end_draw` frame.
        unsafe { self.target.Clear(Some(&d2d_color)) };
    }

    /// Retrieves or creates a cached `ID2D1SolidColorBrush` for `color`.
    ///
    /// # Errors
    /// Returns [`GfxError::Win32`] if Direct2D brush creation fails.
    pub fn brush(&mut self, color: Color) -> Result<ID2D1SolidColorBrush> {
        let key = color.to_packed_rgba();
        if let Some(existing) = self.brushes.get(&key) {
            return Ok(existing.clone());
        }
        if self.brushes.len() >= MAX_CACHED_BRUSHES {
            self.brushes.clear();
        }
        let d2d_color = color.to_d2d();
        // SAFETY: `target` is valid; `d2d_color` is a stack value.
        let created = unsafe { self.target.CreateSolidColorBrush(&d2d_color, None)? };
        self.brushes.insert(key, created.clone());
        self.recompute_cache_bytes();
        Ok(created)
    }

    /// Retrieves or creates a cached `IDWriteTextFormat`.
    ///
    /// # Errors
    /// Returns [`GfxError::Win32`] if DirectWrite fails to create the format.
    pub fn text_format(&mut self, spec: &TextFormatSpec) -> Result<IDWriteTextFormat> {
        if let Some(existing) = self.text_formats.get(spec) {
            return Ok(existing.clone());
        }
        if self.text_formats.len() >= MAX_CACHED_TEXT_FORMATS {
            self.text_formats.clear();
        }

        let family = HSTRING::from(spec.family.as_str());
        let locale = HSTRING::from("en-us");
        let style = if spec.italic {
            DWRITE_FONT_STYLE_ITALIC
        } else {
            DWRITE_FONT_STYLE_NORMAL
        };

        // SAFETY: `dwrite_factory` is valid and strings are valid HSTRINGs.
        let format = unsafe {
            self.dwrite_factory.CreateTextFormat(
                &family,
                None,
                spec.weight.to_dwrite(),
                style,
                DWRITE_FONT_STRETCH_NORMAL,
                spec.size_dip(),
                &locale,
            )?
        };
        let halign = match spec.align {
            TextAlign::Leading => DWRITE_TEXT_ALIGNMENT_LEADING,
            TextAlign::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
            TextAlign::Trailing => DWRITE_TEXT_ALIGNMENT_TRAILING,
        };
        let valign = match spec.valign {
            VerticalAlign::Top => DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
            VerticalAlign::Center => DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
        };
        // SAFETY: `format` is a valid newly created `IDWriteTextFormat`.
        unsafe {
            format.SetTextAlignment(halign)?;
            format.SetParagraphAlignment(valign)?;
        }

        self.text_formats.insert(spec.clone(), format.clone());
        self.recompute_cache_bytes();
        Ok(format)
    }

    /// Creates an `IDWriteTextLayout` using the surface's DirectWrite factory.
    ///
    /// # Errors
    /// Returns [`GfxError::Win32`] if DirectWrite fails to create the layout.
    pub fn create_text_layout(
        &self,
        text: &str,
        format: &IDWriteTextFormat,
        max_width: f32,
        max_height: f32,
    ) -> Result<IDWriteTextLayout> {
        let wide: Vec<u16> = text.encode_utf16().collect();
        // SAFETY: `dwrite_factory` and `format` are valid COM objects.
        let layout = unsafe {
            self.dwrite_factory.CreateTextLayout(
                &wide,
                format,
                max_width.max(1.0),
                max_height.max(1.0),
            )?
        };
        Ok(layout)
    }

    /// Fills an axis-aligned rectangle.
    ///
    /// # Errors
    /// Returns [`GfxError::Win32`] if brush creation fails.
    pub fn fill_rect(&mut self, rect: D2D_RECT_F, color: Color) -> Result<()> {
        let brush = self.brush(color)?;
        // SAFETY: `target` and `brush` are valid within an active draw frame.
        unsafe { self.target.FillRectangle(&rect, &brush) };
        Ok(())
    }

    /// Strokes an axis-aligned rectangle.
    ///
    /// # Errors
    /// Returns [`GfxError::Win32`] if brush creation fails.
    pub fn draw_rect(&mut self, rect: D2D_RECT_F, color: Color, stroke_width: f32) -> Result<()> {
        let brush = self.brush(color)?;
        // SAFETY: `target` and `brush` are valid within an active draw frame.
        unsafe {
            self.target
                .DrawRectangle(&rect, &brush, stroke_width, None);
        };
        Ok(())
    }

    /// Fills a rounded rectangle.
    ///
    /// # Errors
    /// Returns [`GfxError::Win32`] if brush creation fails.
    pub fn fill_rounded_rect(
        &mut self,
        rect: D2D_RECT_F,
        radius: f32,
        color: Color,
    ) -> Result<()> {
        let brush = self.brush(color)?;
        let rounded = D2D1_ROUNDED_RECT {
            rect,
            radiusX: radius,
            radiusY: radius,
        };
        // SAFETY: `target` and `brush` are valid within an active draw frame.
        unsafe { self.target.FillRoundedRectangle(&rounded, &brush) };
        Ok(())
    }

    /// Strokes a rounded rectangle.
    ///
    /// # Errors
    /// Returns [`GfxError::Win32`] if brush creation fails.
    pub fn draw_rounded_rect(
        &mut self,
        rect: D2D_RECT_F,
        radius: f32,
        color: Color,
        stroke_width: f32,
    ) -> Result<()> {
        let brush = self.brush(color)?;
        let rounded = D2D1_ROUNDED_RECT {
            rect,
            radiusX: radius,
            radiusY: radius,
        };
        // SAFETY: `target` and `brush` are valid within an active draw frame.
        unsafe {
            self.target
                .DrawRoundedRectangle(&rounded, &brush, stroke_width, None);
        };
        Ok(())
    }

    /// Draws a straight line between two DIP points.
    ///
    /// # Errors
    /// Returns [`GfxError::Win32`] if brush creation fails.
    pub fn draw_line(
        &mut self,
        p0: Vector2,
        p1: Vector2,
        color: Color,
        stroke_width: f32,
    ) -> Result<()> {
        let brush = self.brush(color)?;
        // SAFETY: `target` and `brush` are valid within an active draw frame.
        unsafe { self.target.DrawLine(p0, p1, &brush, stroke_width, None) };
        Ok(())
    }

    /// Draws text clipped to `rect` using a cached text format.
    ///
    /// # Errors
    /// Returns [`GfxError::Win32`] if brush or format creation fails.
    pub fn draw_text(
        &mut self,
        text: &str,
        rect: D2D_RECT_F,
        spec: &TextFormatSpec,
        color: Color,
    ) -> Result<()> {
        let format = self.text_format(spec)?;
        let brush = self.brush(color)?;
        let wide: Vec<u16> = text.encode_utf16().collect();
        // SAFETY: `wide`, `format`, and `brush` are valid for the call.
        unsafe {
            self.target.DrawText(
                &wide,
                &format,
                &rect,
                &brush,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
        Ok(())
    }

    /// Draws a precomputed `IDWriteTextLayout` at `origin`.
    ///
    /// # Errors
    /// Returns [`GfxError::Win32`] if brush creation fails.
    pub fn draw_text_layout(
        &mut self,
        origin: Vector2,
        layout: &IDWriteTextLayout,
        color: Color,
    ) -> Result<()> {
        let brush = self.brush(color)?;
        // SAFETY: `layout` and `brush` are valid COM objects.
        unsafe {
            self.target
                .DrawTextLayout(origin, layout, &brush, D2D1_DRAW_TEXT_OPTIONS_CLIP);
        }
        Ok(())
    }

    /// Fills a Direct2D geometry (e.g. an adaptive engine glyph).
    ///
    /// # Errors
    /// Returns [`GfxError::Win32`] if brush creation fails.
    pub fn fill_geometry(&mut self, geometry: &ID2D1Geometry, color: Color) -> Result<()> {
        let brush = self.brush(color)?;
        // SAFETY: `geometry` and `brush` are valid COM objects.
        unsafe { self.target.FillGeometry(geometry, &brush, None) };
        Ok(())
    }

    /// Sets the active 2D affine transform on the render target.
    pub fn set_transform(&self, matrix: &Matrix3x2) {
        // SAFETY: `target` is valid and `matrix` is a valid `Matrix3x2`.
        unsafe { self.target.SetTransform(matrix) };
    }

    /// Resets the render target transform to identity.
    pub fn reset_transform(&self) {
        self.set_transform(&Matrix3x2::identity());
    }

    /// Pushes an axis-aligned clip rectangle. Must be paired with [`Self::pop_clip`].
    pub fn push_clip(&self, rect: D2D_RECT_F) {
        // SAFETY: `target` is valid inside an active draw frame.
        unsafe {
            self.target.PushAxisAlignedClip(
                &rect,
                windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            );
        }
    }

    /// Pops the most recent axis-aligned clip rectangle.
    pub fn pop_clip(&self) {
        // SAFETY: Paired with `push_clip`.
        unsafe { self.target.PopAxisAlignedClip() };
    }

    fn recompute_cache_bytes(&mut self) {
        // Conservative per-entry estimate for brush & text format COM objects.
        self.budget.cache_bytes = self
            .brushes
            .len()
            .saturating_mul(256)
            .saturating_add(self.text_formats.len().saturating_mul(2048));
    }
}

/// Helper to construct a [`D2D_RECT_F`] from origin and size.
#[must_use]
pub const fn rect_f(x: f32, y: f32, width: f32, height: f32) -> D2D_RECT_F {
    D2D_RECT_F {
        left: x,
        top: y,
        right: x + width,
        bottom: y + height,
    }
}

/// Helper to construct a [`Vector2`] point.
#[must_use]
pub const fn point_f(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_budget_enforces_60mb_limit() {
        let mut budget = MemoryBudget::new(DEFAULT_MEMORY_BUDGET_BYTES);
        assert_eq!(budget.limit_bytes, 60 * 1024 * 1024);
        budget.set_surface_dimensions(1920, 1080);
        assert!(!budget.is_over_budget());

        budget.set_surface_dimensions(4096, 2160);
        assert!(budget.is_over_budget());
    }

    #[test]
    fn text_format_spec_quantizes_size() {
        let spec = TextFormatSpec::new("Cascadia Mono", 13.25)
            .with_weight(FontWeight::SemiBold)
            .with_italic(true);
        assert_eq!(spec.size_tenths, 133);
        assert!((spec.size_dip() - 13.3).abs() < 1e-4);
    }
}
