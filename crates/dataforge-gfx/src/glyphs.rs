//! Adaptive vector glyphs (`16x16` normalized viewbox) for all 11 database engines
//! and the DataForge application icon, backed by a pure-Rust SVG path parser and
//! Direct2D `ID2D1PathGeometry` builder.

use std::f32::consts::TAU;

use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_BEZIER_SEGMENT, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN,
    D2D1_FILL_MODE_WINDING,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_QUADRATIC_BEZIER_SEGMENT, ID2D1Factory1, ID2D1PathGeometry,
};
use windows_numerics::Vector2;

use crate::error::{GfxError, Result};

/// Normalized viewbox extent (`16.0 x 16.0` DIPs) for all adaptive glyphs.
pub const NORMALIZED_VIEWBOX: f32 = 16.0;

/// Supported database engines with adaptive vector glyphs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EngineKind {
    /// PostgreSQL.
    Postgresql,
    /// MySQL.
    Mysql,
    /// MariaDB.
    Mariadb,
    /// SQLite.
    Sqlite,
    /// DuckDB.
    Duckdb,
    /// Microsoft SQL Server.
    Mssql,
    /// ClickHouse.
    Clickhouse,
    /// Apache Cassandra.
    Cassandra,
    /// ScyllaDB.
    Scylladb,
    /// MongoDB.
    Mongodb,
    /// Redis.
    Redis,
}

impl EngineKind {
    /// All 11 supported database engines in canonical display order.
    pub const ALL: [Self; 11] = [
        Self::Postgresql,
        Self::Mysql,
        Self::Mariadb,
        Self::Sqlite,
        Self::Duckdb,
        Self::Mssql,
        Self::Clickhouse,
        Self::Cassandra,
        Self::Scylladb,
        Self::Mongodb,
        Self::Redis,
    ];

    /// Human-readable engine name.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Postgresql => "PostgreSQL",
            Self::Mysql => "MySQL",
            Self::Mariadb => "MariaDB",
            Self::Sqlite => "SQLite",
            Self::Duckdb => "DuckDB",
            Self::Mssql => "SQL Server",
            Self::Clickhouse => "ClickHouse",
            Self::Cassandra => "Cassandra",
            Self::Scylladb => "ScyllaDB",
            Self::Mongodb => "MongoDB",
            Self::Redis => "Redis",
        }
    }

    /// Returns the parsed `16x16` normalized [`GlyphDefinition`] for this engine.
    ///
    /// # Errors
    /// Returns [`GfxError::InvalidSvgPath`] if an embedded SVG path is malformed.
    pub fn glyph(self) -> Result<GlyphDefinition> {
        engine_glyph(self)
    }
}

/// A single normalized vector path segment in `16x16` viewbox coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathSegment {
    /// Starts a new subpath at `(x, y)`.
    MoveTo {
        /// Target X coordinate.
        x: f32,
        /// Target Y coordinate.
        y: f32,
    },
    /// Straight line to `(x, y)`.
    LineTo {
        /// Target X coordinate.
        x: f32,
        /// Target Y coordinate.
        y: f32,
    },
    /// Cubic Bezier curve to `(x, y)` with control points `(c1x, c1y)` and `(c2x, c2y)`.
    CubicTo {
        /// First control point X.
        c1x: f32,
        /// First control point Y.
        c1y: f32,
        /// Second control point X.
        c2x: f32,
        /// Second control point Y.
        c2y: f32,
        /// End point X.
        x: f32,
        /// End point Y.
        y: f32,
    },
    /// Quadratic Bezier curve to `(x, y)` with control point `(cx, cy)`.
    QuadTo {
        /// Control point X.
        cx: f32,
        /// Control point Y.
        cy: f32,
        /// End point X.
        x: f32,
        /// End point Y.
        y: f32,
    },
    /// Closes the current subpath back to its initial point.
    Close,
}

/// Adaptive vector glyph normalized to a `16x16` viewbox.
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphDefinition {
    /// Human-readable glyph label.
    pub name: &'static str,
    /// Normalized path segments in `[0.0, 16.0]` coordinates.
    pub segments: Vec<PathSegment>,
}

impl GlyphDefinition {
    /// Builds a Direct2D `ID2D1PathGeometry` scaled to `target_size` DIPs.
    ///
    /// # Errors
    /// Returns [`GfxError::Win32`] if Direct2D geometry construction fails.
    pub fn to_d2d_geometry(
        &self,
        factory: &ID2D1Factory1,
        target_size: f32,
    ) -> Result<ID2D1PathGeometry> {
        let scale = target_size / NORMALIZED_VIEWBOX;
        build_path_geometry(factory, &self.segments, scale)
    }
}

/// Builds an `ID2D1PathGeometry` from normalized [`PathSegment`]s scaled by `scale`.
///
/// # Errors
/// Returns [`GfxError::Win32`] if Direct2D fails to create or populate the geometry.
pub fn build_path_geometry(
    factory: &ID2D1Factory1,
    segments: &[PathSegment],
    scale: f32,
) -> Result<ID2D1PathGeometry> {
    // SAFETY: `factory` is a valid `ID2D1Factory1`.
    let geometry: ID2D1PathGeometry = unsafe { factory.CreatePathGeometry()?.into() };
    // SAFETY: `geometry` was just created and is not yet populated.
    let sink = unsafe { geometry.Open()? };
    // SAFETY: `sink` is open and valid until `Close()`.
    unsafe { sink.SetFillMode(D2D1_FILL_MODE_WINDING) };

    let mut figure_open = false;
    for seg in segments {
        match *seg {
            PathSegment::MoveTo { x, y } => {
                if figure_open {
                    // SAFETY: Closing previous open figure before starting a new one.
                    unsafe { sink.EndFigure(D2D1_FIGURE_END_OPEN) };
                }
                let pt = Vector2 {
                    X: x * scale,
                    Y: y * scale,
                };
                // SAFETY: `sink` is open.
                unsafe { sink.BeginFigure(pt, D2D1_FIGURE_BEGIN_FILLED) };
                figure_open = true;
            }
            PathSegment::LineTo { x, y } if figure_open => {
                let pt = Vector2 {
                    X: x * scale,
                    Y: y * scale,
                };
                // SAFETY: Figure is open.
                unsafe { sink.AddLine(pt) };
            }
            PathSegment::CubicTo {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
            } if figure_open => {
                let bez = D2D1_BEZIER_SEGMENT {
                    point1: Vector2 {
                        X: c1x * scale,
                        Y: c1y * scale,
                    },
                    point2: Vector2 {
                        X: c2x * scale,
                        Y: c2y * scale,
                    },
                    point3: Vector2 {
                        X: x * scale,
                        Y: y * scale,
                    },
                };
                // SAFETY: Figure is open.
                unsafe { sink.AddBezier(&bez) };
            }
            PathSegment::QuadTo { cx, cy, x, y } if figure_open => {
                let qbez = D2D1_QUADRATIC_BEZIER_SEGMENT {
                    point1: Vector2 {
                        X: cx * scale,
                        Y: cy * scale,
                    },
                    point2: Vector2 {
                        X: x * scale,
                        Y: y * scale,
                    },
                };
                // SAFETY: Figure is open.
                unsafe { sink.AddQuadraticBezier(&qbez) };
            }
            PathSegment::Close if figure_open => {
                // SAFETY: Figure is open.
                unsafe { sink.EndFigure(D2D1_FIGURE_END_CLOSED) };
                figure_open = false;
            }
            _ => {}
        }
    }

    if figure_open {
        // SAFETY: Close trailing open figure before closing sink.
        unsafe { sink.EndFigure(D2D1_FIGURE_END_OPEN) };
    }
    // SAFETY: All figures are ended.
    unsafe { sink.Close()? };
    Ok(geometry)
}

/// Parses an SVG `d` path string and normalizes coordinates from `viewbox_size` to `16x16`.
///
/// # Errors
/// Returns [`GfxError::InvalidSvgPath`] if `viewbox_size <= 0.0` or the path contains
/// unsupported commands or malformed numbers.
pub fn parse_svg_path(d: &str, viewbox_size: f32) -> Result<Vec<PathSegment>> {
    if viewbox_size <= 0.0 || !viewbox_size.is_finite() {
        return Err(GfxError::InvalidSvgPath {
            offset: 0,
            reason: "viewbox_size must be positive and finite".to_owned(),
        });
    }
    let scale = NORMALIZED_VIEWBOX / viewbox_size;
    let mut parser = SvgPathParser::new(d, scale);
    parser.parse_all()
}

struct SvgPathParser<'a> {
    bytes: &'a [u8],
    pos: usize,
    scale: f32,
    cx: f32,
    cy: f32,
    sx: f32,
    sy: f32,
    prev_ctrl_x: f32,
    prev_ctrl_y: f32,
    last_cmd: u8,
    segments: Vec<PathSegment>,
}

impl<'a> SvgPathParser<'a> {
    fn new(input: &'a str, scale: f32) -> Self {
        Self {
            bytes: input.as_bytes(),
            pos: 0,
            scale,
            cx: 0.0,
            cy: 0.0,
            sx: 0.0,
            sy: 0.0,
            prev_ctrl_x: 0.0,
            prev_ctrl_y: 0.0,
            last_cmd: 0,
            segments: Vec::new(),
        }
    }

    fn parse_all(&mut self) -> Result<Vec<PathSegment>> {
        let mut active_cmd: Option<u8> = None;
        loop {
            self.skip_ws_and_commas();
            if self.pos >= self.bytes.len() {
                break;
            }
            let b = self.bytes[self.pos];
            let cmd = if b.is_ascii_alphabetic() {
                self.pos += 1;
                b
            } else if let Some(prev) = active_cmd {
                match prev {
                    b'M' => b'L',
                    b'm' => b'l',
                    other => other,
                }
            } else {
                return Err(self.err("expected SVG path command"));
            };

            self.dispatch_command(cmd)?;
            active_cmd = if matches!(cmd, b'Z' | b'z') {
                None
            } else {
                Some(cmd)
            };
        }
        Ok(std::mem::take(&mut self.segments))
    }

    fn dispatch_command(&mut self, cmd: u8) -> Result<()> {
        match cmd {
            b'M' | b'm' => self.parse_move(cmd == b'm'),
            b'L' | b'l' => self.parse_line(cmd == b'l'),
            b'H' | b'h' => self.parse_horiz(cmd == b'h'),
            b'V' | b'v' => self.parse_vert(cmd == b'v'),
            b'C' | b'c' => self.parse_cubic(cmd == b'c'),
            b'S' | b's' => self.parse_smooth_cubic(cmd == b's'),
            b'Q' | b'q' => self.parse_quad(cmd == b'q'),
            b'T' | b't' => self.parse_smooth_quad(cmd == b't'),
            b'A' | b'a' => self.parse_arc(cmd == b'a'),
            b'Z' | b'z' => {
                self.cx = self.sx;
                self.cy = self.sy;
                self.segments.push(PathSegment::Close);
                self.last_cmd = cmd;
                Ok(())
            }
            _ => Err(self.err(format!("unsupported SVG command '{}'", cmd as char))),
        }
    }

    fn parse_move(&mut self, rel: bool) -> Result<()> {
        let (x, y) = self.read_point(rel)?;
        self.cx = x;
        self.cy = y;
        self.sx = x;
        self.sy = y;
        self.segments.push(PathSegment::MoveTo {
            x: x * self.scale,
            y: y * self.scale,
        });
        self.last_cmd = if rel { b'm' } else { b'M' };
        Ok(())
    }

    fn parse_line(&mut self, rel: bool) -> Result<()> {
        let (x, y) = self.read_point(rel)?;
        self.cx = x;
        self.cy = y;
        self.segments.push(PathSegment::LineTo {
            x: x * self.scale,
            y: y * self.scale,
        });
        self.last_cmd = if rel { b'l' } else { b'L' };
        Ok(())
    }

    fn parse_horiz(&mut self, rel: bool) -> Result<()> {
        let x = self.read_number()?;
        self.cx = if rel { self.cx + x } else { x };
        self.segments.push(PathSegment::LineTo {
            x: self.cx * self.scale,
            y: self.cy * self.scale,
        });
        self.last_cmd = if rel { b'h' } else { b'H' };
        Ok(())
    }

    fn parse_vert(&mut self, rel: bool) -> Result<()> {
        let y = self.read_number()?;
        self.cy = if rel { self.cy + y } else { y };
        self.segments.push(PathSegment::LineTo {
            x: self.cx * self.scale,
            y: self.cy * self.scale,
        });
        self.last_cmd = if rel { b'v' } else { b'V' };
        Ok(())
    }

    fn parse_cubic(&mut self, rel: bool) -> Result<()> {
        let (c1x, c1y) = self.read_point(rel)?;
        let (c2x, c2y) = self.read_point(rel)?;
        let (x, y) = self.read_point(rel)?;
        self.prev_ctrl_x = c2x;
        self.prev_ctrl_y = c2y;
        self.cx = x;
        self.cy = y;
        self.push_cubic(c1x, c1y, c2x, c2y, x, y);
        self.last_cmd = if rel { b'c' } else { b'C' };
        Ok(())
    }

    fn parse_smooth_cubic(&mut self, rel: bool) -> Result<()> {
        let (c1x, c1y) = if matches!(self.last_cmd, b'C' | b'c' | b'S' | b's') {
            (2.0 * self.cx - self.prev_ctrl_x, 2.0 * self.cy - self.prev_ctrl_y)
        } else {
            (self.cx, self.cy)
        };
        let (c2x, c2y) = self.read_point(rel)?;
        let (x, y) = self.read_point(rel)?;
        self.prev_ctrl_x = c2x;
        self.prev_ctrl_y = c2y;
        self.cx = x;
        self.cy = y;
        self.push_cubic(c1x, c1y, c2x, c2y, x, y);
        self.last_cmd = if rel { b's' } else { b'S' };
        Ok(())
    }

    fn parse_quad(&mut self, rel: bool) -> Result<()> {
        let (qx, qy) = self.read_point(rel)?;
        let (x, y) = self.read_point(rel)?;
        self.prev_ctrl_x = qx;
        self.prev_ctrl_y = qy;
        self.cx = x;
        self.cy = y;
        self.segments.push(PathSegment::QuadTo {
            cx: qx * self.scale,
            cy: qy * self.scale,
            x: x * self.scale,
            y: y * self.scale,
        });
        self.last_cmd = if rel { b'q' } else { b'Q' };
        Ok(())
    }

    fn parse_smooth_quad(&mut self, rel: bool) -> Result<()> {
        let (qx, qy) = if matches!(self.last_cmd, b'Q' | b'q' | b'T' | b't') {
            (2.0 * self.cx - self.prev_ctrl_x, 2.0 * self.cy - self.prev_ctrl_y)
        } else {
            (self.cx, self.cy)
        };
        let (x, y) = self.read_point(rel)?;
        self.prev_ctrl_x = qx;
        self.prev_ctrl_y = qy;
        self.cx = x;
        self.cy = y;
        self.segments.push(PathSegment::QuadTo {
            cx: qx * self.scale,
            cy: qy * self.scale,
            x: x * self.scale,
            y: y * self.scale,
        });
        self.last_cmd = if rel { b't' } else { b'T' };
        Ok(())
    }

    fn parse_arc(&mut self, rel: bool) -> Result<()> {
        let rx = self.read_number()?.abs();
        let ry = self.read_number()?.abs();
        let x_rot = self.read_number()?;
        let large_arc = self.read_flag()?;
        let sweep = self.read_flag()?;
        let (x, y) = self.read_point(rel)?;
        let start = (self.cx, self.cy);
        self.cx = x;
        self.cy = y;
        self.last_cmd = if rel { b'a' } else { b'A' };

        if (x - start.0).abs() < 1e-6 && (y - start.1).abs() < 1e-6 {
            return Ok(());
        }
        if rx < 1e-6 || ry < 1e-6 {
            self.segments.push(PathSegment::LineTo {
                x: x * self.scale,
                y: y * self.scale,
            });
            return Ok(());
        }
        self.arc_to_cubics(start, (x, y), (rx, ry), x_rot, large_arc, sweep);
        Ok(())
    }

    fn push_cubic(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        self.segments.push(PathSegment::CubicTo {
            c1x: c1x * self.scale,
            c1y: c1y * self.scale,
            c2x: c2x * self.scale,
            c2y: c2y * self.scale,
            x: x * self.scale,
            y: y * self.scale,
        });
    }

    fn arc_to_cubics(
        &mut self,
        start: (f32, f32),
        end: (f32, f32),
        radii: (f32, f32),
        x_rot_deg: f32,
        large_arc: bool,
        sweep: bool,
    ) {
        let phi = x_rot_deg.to_radians();
        let (sin_p, cos_p) = phi.sin_cos();
        let dx2 = (start.0 - end.0) * 0.5;
        let dy2 = (start.1 - end.1) * 0.5;
        let x1p = cos_p * dx2 + sin_p * dy2;
        let y1p = -sin_p * dx2 + cos_p * dy2;

        let (mut rx, mut ry) = radii;
        let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
        if lambda > 1.0 {
            let s = lambda.sqrt();
            rx *= s;
            ry *= s;
        }

        let rx2 = rx * rx;
        let ry2 = ry * ry;
        let num = (rx2 * ry2 - rx2 * y1p * y1p - ry2 * x1p * x1p).max(0.0);
        let den = rx2 * y1p * y1p + ry2 * x1p * x1p;
        let factor = if den > 1e-12 { (num / den).sqrt() } else { 0.0 };
        let sign = if large_arc == sweep { -1.0 } else { 1.0 };
        let cxp = sign * factor * (rx * y1p / ry);
        let cyp = sign * factor * (-ry * x1p / rx);

        let cx = cos_p * cxp - sin_p * cyp + (start.0 + end.0) * 0.5;
        let cy = sin_p * cxp + cos_p * cyp + (start.1 + end.1) * 0.5;

        let ux = (x1p - cxp) / rx;
        let uy = (y1p - cyp) / ry;
        let vx = (-x1p - cxp) / rx;
        let vy = (-y1p - cyp) / ry;

        let theta1 = vec_angle(1.0, 0.0, ux, uy);
        let mut dtheta = vec_angle(ux, uy, vx, vy);
        if !sweep && dtheta > 0.0 {
            dtheta -= TAU;
        } else if sweep && dtheta < 0.0 {
            dtheta += TAU;
        }

        let segs = ((dtheta.abs() / (TAU * 0.25)).ceil() as usize).max(1);
        let delta = dtheta / segs as f32;
        let alpha = (4.0 / 3.0) * (delta * 0.25).tan();

        for i in 0..segs {
            let t0 = theta1 + delta * i as f32;
            let t1 = t0 + delta;
            let (s0, c0) = t0.sin_cos();
            let (s1, c1) = t1.sin_cos();

            let p1x = c0 - alpha * s0;
            let p1y = s0 + alpha * c0;
            let p2x = c1 + alpha * s1;
            let p2y = s1 - alpha * c1;

            let map = |px: f32, py: f32| -> (f32, f32) {
                let sx = rx * px;
                let sy = ry * py;
                (cx + cos_p * sx - sin_p * sy, cy + sin_p * sx + cos_p * sy)
            };

            let (c1x, c1y) = map(p1x, p1y);
            let (c2x, c2y) = map(p2x, p2y);
            let (ex, ey) = if i + 1 == segs {
                end
            } else {
                map(c1, s1)
            };
            self.push_cubic(c1x, c1y, c2x, c2y, ex, ey);
        }
    }

    fn read_point(&mut self, rel: bool) -> Result<(f32, f32)> {
        let x = self.read_number()?;
        let y = self.read_number()?;
        if rel {
            Ok((self.cx + x, self.cy + y))
        } else {
            Ok((x, y))
        }
    }

    fn read_flag(&mut self) -> Result<bool> {
        self.skip_ws_and_commas();
        if self.pos >= self.bytes.len() {
            return Err(self.err("unexpected end of SVG path while reading arc flag"));
        }
        let val = match self.bytes[self.pos] {
            b'0' => false,
            b'1' => true,
            other => {
                return Err(self.err(format!("invalid arc flag '{}'", other as char)));
            }
        };
        self.pos += 1;
        Ok(val)
    }

    fn read_number(&mut self) -> Result<f32> {
        self.skip_ws_and_commas();
        let start = self.pos;
        if self.pos < self.bytes.len() && matches!(self.bytes[self.pos], b'+' | b'-') {
            self.pos += 1;
        }
        let mut seen_digit = false;
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
            seen_digit = true;
            self.pos += 1;
        }
        if self.pos < self.bytes.len() && self.bytes[self.pos] == b'.' {
            self.pos += 1;
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                seen_digit = true;
                self.pos += 1;
            }
        }
        if !seen_digit {
            return Err(self.err("expected number in SVG path"));
        }
        if self.pos < self.bytes.len() && matches!(self.bytes[self.pos], b'e' | b'E') {
            self.pos += 1;
            if self.pos < self.bytes.len() && matches!(self.bytes[self.pos], b'+' | b'-') {
                self.pos += 1;
            }
            let exp_start = self.pos;
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                self.pos += 1;
            }
            if self.pos == exp_start {
                return Err(self.err("expected exponent digits in SVG path"));
            }
        }
        let slice = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| self.err("non-UTF8 number in SVG path"))?;
        slice
            .parse::<f32>()
            .map_err(|_| self.err(format!("invalid float '{slice}'")))
    }

    fn skip_ws_and_commas(&mut self) {
        while self.pos < self.bytes.len()
            && (self.bytes[self.pos].is_ascii_whitespace() || self.bytes[self.pos] == b',')
        {
            self.pos += 1;
        }
    }

    fn err(&self, reason: impl Into<String>) -> GfxError {
        GfxError::InvalidSvgPath {
            offset: self.pos,
            reason: reason.into(),
        }
    }
}

fn vec_angle(ux: f32, uy: f32, vx: f32, vy: f32) -> f32 {
    let dot = ux * vx + uy * vy;
    let len = (ux * ux + uy * uy).sqrt() * (vx * vx + vy * vy).sqrt();
    let ratio = if len > 1e-12 {
        (dot / len).clamp(-1.0, 1.0)
    } else {
        1.0
    };
    let sign = if (ux * vy - uy * vx) < 0.0 { -1.0 } else { 1.0 };
    sign * ratio.acos()
}

/// Returns the `16x16` normalized [`GlyphDefinition`] for a database engine.
///
/// # Errors
/// Returns [`GfxError::InvalidSvgPath`] if an embedded path fails to parse.
pub fn engine_glyph(kind: EngineKind) -> Result<GlyphDefinition> {
    let (viewbox, paths): (f32, &[&str]) = match kind {
        EngineKind::Postgresql => (24.0, &[POSTGRESQL_PATH]),
        EngineKind::Mysql => (24.0, &[MYSQL_PATH]),
        EngineKind::Mariadb => (24.0, &[MARIADB_PATH]),
        EngineKind::Sqlite => (24.0, &[SQLITE_PATH]),
        EngineKind::Duckdb => (24.0, &[DUCKDB_PATH]),
        EngineKind::Mssql => (48.0, MSSQL_PATHS),
        EngineKind::Clickhouse => (24.0, &[CLICKHOUSE_PATH]),
        EngineKind::Cassandra => (32.0, CASSANDRA_PATHS),
        EngineKind::Scylladb => (32.0, SCYLLADB_PATHS),
        EngineKind::Mongodb => (24.0, &[MONGODB_PATH]),
        EngineKind::Redis => (24.0, &[REDIS_PATH]),
    };

    let mut segments = Vec::new();
    for path_d in paths {
        segments.extend(parse_svg_path(path_d, viewbox)?);
    }
    Ok(GlyphDefinition {
        name: kind.display_name(),
        segments,
    })
}

/// Returns the `16x16` normalized DataForge application icon glyph.
///
/// # Errors
/// Returns [`GfxError::InvalidSvgPath`] if the embedded path fails to parse.
pub fn app_icon_glyph() -> Result<GlyphDefinition> {
    Ok(GlyphDefinition {
        name: "DataForge",
        segments: parse_svg_path(DATAFORGE_APP_ICON_PATH, 16.0)?,
    })
}

const DATAFORGE_APP_ICON_PATH: &str = "\
M8 1.5C4.4 1.5 2 2.6 2 4v2c0 1.4 2.4 2.5 6 2.5s6-1.1 6-2.5V4c0-1.4-2.4-2.5-6-2.5z\
M2 7.6v2.2c0 1.4 2.4 2.5 6 2.5s6-1.1 6-2.5V7.6c-1.3.9-3.5 1.4-6 1.4s-4.7-.5-6-1.4z\
M2 11.4v1.8c0 1.4 2.4 2.3 6 2.3s6-.9 6-2.3v-1.8c-1.3.9-3.5 1.4-6 1.4s-4.7-.5-6-1.4z";

const POSTGRESQL_PATH: &str = "\
M23.5594 14.7228a.5269.5269 0 0 0-.0563-.1191c-.139-.2632-.4768-.3418-1.0074-.2321\
-1.6533.3411-2.2935.1312-2.5256-.0191 1.342-2.0482 2.445-4.522 3.0411-6.8297\
.2714-1.0507.7982-3.5237.1222-4.7316a1.5641 1.5641 0 0 0-.1509-.235C21.6931.9086 19.8007.0248 17.5099.0005\
c-1.4947-.0158-2.7705.3461-3.1161.4794a9.449 9.449 0 0 0-.5159-.0816 8.044 8.044 0 0 0-1.3114-.1278\
c-1.1822-.0184-2.2038.2642-3.0498.8406-.8573-.3211-4.7888-1.645-7.2219.0788\
C.9359 2.1526.3086 3.8733.4302 6.3043c.0409.818.5069 3.334 1.2423 5.7436\
.4598 1.5065.9387 2.7019 1.4334 3.582.553.9942 1.1259 1.5933 1.7143 1.7895\
.4474.1491 1.1327.1441 1.8581-.7279.8012-.9635 1.5903-1.8258 1.9446-2.2069\
.4351.2355.9064.3625 1.39.3772a.0569.0569 0 0 0 .0004.0041 11.0312 11.0312 0 0 0-.2472.3054\
c-.3389.4302-.4094.5197-1.5002.7443-.3102.064-1.1344.2339-1.1464.8115\
-.0025.1224.0329.2309.0919.3268.2269.4231.9216.6097 1.015.6331 \
1.3345.3335 2.5044.092 3.3714-.6787-.017 2.231.0775 4.4174.3454 5.0874\
.2212.5529.7618 1.9045 2.4692 1.9043.2505 0 .5263-.0291.8296-.0941 \
1.7819-.3821 2.5557-1.1696 2.855-2.9059.1503-.8707.4016-2.8753.5388-4.1012\
.0169-.0703.0357-.1207.057-.1362.0007-.0005.0697-.0471.4272.0307\
a.3673.3673 0 0 0 .0443.0068l.2539.0223.0149.001c.8468.0384 1.9114-.1426 2.5312-.4308\
.6438-.2988 1.8057-1.0323 1.5951-1.6698z";

const MYSQL_PATH: &str = "\
M23.224 11.311c-.535-.014-.95.04-1.297.188-.1.04-.26.04-.274.167.055.053.063.14.11.214\
.08.134.218.313.346.407.14.11.28.216.427.31.26.16.555.255.81.416.145.094.293.213.44.313\
.073.05.12.14.214.172v-.02c-.046-.06-.06-.147-.105-.214-.067-.067-.134-.127-.2-.193\
a3.223 3.223 0 00-.695-.675c-.214-.146-.682-.35-.77-.595l-.013-.014\
c.146-.013.32-.066.46-.106.227-.06.435-.047.67-.106.106-.027.213-.06.32-.094v-.06\
c-.12-.12-.21-.283-.334-.395a8.867 8.867 0 00-1.104-.823c-.21-.134-.476-.22-.697-.334\
-.08-.04-.214-.06-.26-.127-.12-.146-.19-.34-.275-.514a17.69 17.69 0 01-.547-1.163\
c-.12-.262-.193-.523-.34-.763-.69-1.137-1.437-1.826-2.586-2.5-.247-.14-.543-.2-.856-.274\
-.167-.008-.334-.02-.5-.027-.11-.047-.216-.174-.31-.235-.38-.24-1.364-.76-1.644-.072\
-.18.434.267.862.422 1.082.115.153.26.328.34.5.047.116.06.235.107.356\
.106.294.207.622.347.897.073.14.153.287.247.413.054.073.146.107.167.227\
-.094.136-.1.334-.154.5-.24.757-.146 1.693.194 2.25.107.166.362.534.703.393\
.3-.12.234-.5.32-.835.02-.08.007-.133.048-.187v.015c.094.188.188.367.274.555\
.206.328.566.668.867.895.16.12.287.328.487.402v-.02h-.015c-.043-.058-.1-.086-.154-.133\
a3.445 3.445 0 01-.35-.4 8.76 8.76 0 01-.747-1.218c-.11-.21-.202-.436-.29-.643\
-.04-.08-.04-.2-.107-.24-.1.146-.247.273-.32.453-.127.288-.14.642-.188 1.01\
-.027.007-.014 0-.027.014-.214-.052-.287-.274-.367-.46-.2-.475-.233-1.238-.06-1.785\
.047-.14.247-.582.167-.716-.042-.127-.174-.2-.247-.303a2.478 2.478 0 01-.24-.427\
c-.16-.374-.24-.788-.414-1.162-.08-.173-.22-.354-.334-.513-.127-.18-.267-.307-.368-.52\
-.033-.073-.08-.194-.027-.274.014-.054.042-.075.094-.09.088-.072.335.022.422.062\
.247.1.455.194.662.334.094.066.195.193.315.226h.14c.214.047.455.014.655.073\
.355.114.675.28.962.46a5.953 5.953 0 012.085 2.286c.08.154.115.295.188.455\
.14.33.313.663.455.982.14.315.275.636.476.897.1.14.502.213.682.286\
.133.06.34.115.46.188.23.14.454.3.67.454.11.076.443.243.463.378z";

const MARIADB_PATH: &str = "\
M23.157 4.412c-.676.284-.79.31-1.673.372-.65.045-.757.057-1.212.209-.75.246-1.395.75-2.02 1.59\
-.296.398-1.249 1.913-1.249 1.988 0 .057-.65.998-.915 1.32-.574.713-1.08 1.079-2.14 1.59\
-.77.36-1.224.524-4.102 1.477-1.073.353-2.133.738-2.367.864-.852.449-1.515 1.036-2.203 1.938\
-1.003 1.32-.972 1.313-3.042.947a12.264 12.264 0 00-.675-.063c-.644-.05-1.023.044-1.332.334\
L0 17.193l.177.088c.094.05.353.234.561.398.215.17.461.347.55.391.088.044.17.088.183.101\
.012.013-.089.17-.228.353-.435.581-.593.871-.574 1.048.019.164.032.17.43.17\
.517-.006.826-.056 1.261-.208.65-.233 2.058-.94 2.784-1.4.776-.5 1.717-.998 1.956-1.042\
.082-.02.354-.07.594-.114.58-.107 1.464-.095 2.587.05.108.013.373.045.6.064\
.227.025.43.057.454.076.026.012.474.037.998.056.934.026 1.104.007 1.3-.189\
.126-.133.385-.631.498-.985.209-.643.417-.921.366-.492-.113.966-.322 1.692-.713 2.411\
-.259.499-.663 1.092-.934 1.395-.322.347-.315.36.088.315.619-.063 1.471-.397 2.096-.82\
.827-.562 1.647-1.691 2.19-3.03.107-.27.22-.22.183.083-.013.094-.038.315-.057.498\
l-.031.328.353-.202c.833-.48 1.414-1.262 2.127-2.884.227-.518.877-2.922 1.073-3.976\
a9.64 9.64 0 01.271-1.042c.127-.429.196-.555.48-.858.183-.19.625-.555.978-.808\
.72-.505.953-.75 1.187-1.205.208-.417.284-1.13.132-1.357-.132-.202-.284-.196-.763.006Z";

const SQLITE_PATH: &str = "\
M21.678.521c-1.032-.92-2.28-.55-3.513.544a8.71 8.71 0 0 0-.547.535\
c-2.109 2.237-4.066 6.38-4.674 9.544.237.48.422 1.093.544 1.561a13.044 13.044 0 0 1 .164.703\
s-.019-.071-.096-.296l-.05-.146a1.689 1.689 0 0 0-.033-.08c-.138-.32-.518-.995-.686-1.289\
-.143.423-.27.818-.376 1.176.484.884.778 2.4.778 2.4s-.025-.099-.147-.442\
c-.107-.303-.644-1.244-.772-1.464-.217.804-.304 1.346-.226 1.478.152.256.296.698.422 1.186\
.286 1.1.485 2.44.485 2.44l.017.224a22.41 22.41 0 0 0 .056 2.748c.095 1.146.273 2.13.5 2.657\
l.155-.084c-.334-1.038-.47-2.399-.41-3.967.09-2.398.642-5.29 1.661-8.304 \
1.723-4.55 4.113-8.201 6.3-9.945-1.993 1.8-4.692 7.63-5.5 9.788\
-.904 2.416-1.545 4.684-1.931 6.857.666-2.037 2.821-2.912 2.821-2.912\
s1.057-1.304 2.292-3.166c-.74.169-1.955.458-2.362.629-.6.251-.762.337-.762.337\
s1.945-1.184 3.613-1.72C21.695 7.9 24.195 2.767 21.678.521\
m-18.573.543A1.842 1.842 0 0 0 1.27 2.9v16.608a1.84 1.84 0 0 0 1.835 1.834h9.418\
a22.953 22.953 0 0 1-.052-2.707c-.006-.062-.011-.141-.016-.2a27.01 27.01 0 0 0-.473-2.378\
c-.121-.47-.275-.898-.369-1.057-.116-.197-.098-.31-.097-.432 0-.12.015-.245.037-.386\
a9.98 9.98 0 0 1 .234-1.045l.217-.028c-.017-.035-.014-.065-.031-.097l-.041-.381\
a32.8 32.8 0 0 1 .382-1.194l.2-.019c-.008-.016-.01-.038-.018-.053l-.043-.316\
c.63-3.28 2.587-7.443 4.8-9.791.066-.069.133-.128.198-.194Z";

const DUCKDB_PATH: &str = "\
M12 0C5.363 0 0 5.363 0 12s5.363 12 12 12 12-5.363 12-12S18.637 0 12 0z\
M9.502 7.03a4.974 4.974 0 0 1 4.97 4.97 4.974 4.974 0 0 1-4.97 4.97\
A4.974 4.974 0 0 1 4.532 12a4.974 4.974 0 0 1 4.97-4.97z\
m6.563 3.183h2.351c.98 0 1.787.782 1.787 1.762s-.807 1.789-1.787 1.789h-2.351v-3.551z";

const MSSQL_PATHS: &[&str] = &[
    "M24.751 43H24.5c-8.192 0-17.309-2.573-18.386-6.879-.657-2.63 1.492-5.536 6.214-8.401l.52.854\
c-4.249 2.579-6.296 5.172-5.763 7.305.935 3.738 9.575 6.068 17.153 6.12.901-1.347 5.742-9.26 2.979-19.873\
l.967-.252c3.149 12.092-3.218 20.837-3.282 20.924L24.751 43z",
    "M21.843 24.4c-.068 0-.137-.014-.201-.042-.199-.088-.319-.294-.296-.51\
.292-2.749-3.926-3.852-3.969-3.862-.174-.044-.312-.179-.359-.352s.002-.359.129-.486\
c.207-.207 5.139-5.098 11.327-7.784.173-.075.369-.047.515.07.145.118.212.307.174.489\
-1.186 5.744-6.71 12.044-6.944 12.309-.1.109-.238.168-.377.168z",
    "M42.935 19.794c0 0-.605.086-.775.106-8.76.97-17.8 3.49-22.97 5.56-1.87.75-3.81 1.66-5.58 2.68\
-.01.01-.02.01-.04.02-1.04.6-3.57 1.84-5.62 2.93 3-3.19 8.62-5.65 10.86-6.55 \
5.07-2.03 13.78-4.48 22.35-5.53-1.01-1.18-3.48-3.68-8.34-5.54-2.84-1.1-7.16-1.72-10.97-2.27\
-6.06-.87-9.51-1.45-9.84-3.1-.07-.33-.02-.66.13-.98.33.54.8.92 1.11 1.14.15.1.26.16.3.18l.01.01\
c1.42.75 5.25 1.3 8.44 1.76 3.86.56 8.23 1.19 11.18 2.32 6.87 2.65 9.24 6.44 9.34 6.6\
.19.15.515.664.515.664z",
];

const CLICKHOUSE_PATH: &str = "\
M21.333 10H24v4h-2.667ZM16 1.335h2.667v21.33H16Zm-5.333 0h2.666v21.33h-2.666Z\
M0 22.665V1.335h2.667v21.33zm5.333-21.33H8v21.33H5.333Z";

const CASSANDRA_PATHS: &[&str] = &[
    "M16 2C8.268 2 2 8.268 2 16s6.268 14 14 14 14-6.268 14-14S23.732 2 16 2z\
m0 2c6.627 0 12 5.373 12 12s-5.373 12-12 12S4 22.627 4 16 9.373 4 16 4z",
    "M16 7c-1.1 0-2 .5-2.6 1.3L9.8 13c-.4.5-.6 1.1-.6 1.7v2.6c0 .6.2 1.2.6 1.7l3.6 4.7\
c.6.8 1.5 1.3 2.6 1.3s2-.5 2.6-1.3l3.6-4.7c.4-.5.6-1.1.6-1.7v-2.6c0-.6-.2-1.2-.6-1.7l-3.6-4.7\
C18 7.5 17.1 7 16 7zm0 2c.4 0 .7.2.9.4l3.6 4.7c.1.2.2.4.2.6v2.6c0 .2-.1.4-.2.6L16.9 22.6\
c-.2.3-.5.4-.9.4s-.7-.2-.9-.4l-3.6-4.7c-.1-.2-.2-.4-.2-.6v-2.6c0-.2.1-.4.2-.6L15.1 9.4\
c.2-.3.5-.4.9-.4z",
];

const SCYLLADB_PATHS: &[&str] = &[
    "M16 2C8.268 2 2 8.268 2 16s6.268 14 14 14 14-6.268 14-14S23.732 2 16 2z\
m0 2c6.627 0 12 5.373 12 12s-5.373 12-12 12S4 22.627 4 16 9.373 4 16 4z",
    "M16 8c-4.4 0-8 3.6-8 8s3.6 8 8 8 8-3.6 8-8-3.6-8-8-8z\
m0 2c3.3 0 6 2.7 6 6s-2.7 6-6 6-6-2.7-6-6 2.7-6 6-6z",
    "M13 16a3 3 0 1 0 6 0 3 3 0 1 0-6 0z",
];

const MONGODB_PATH: &str = "\
M17.193 9.555c-1.264-5.58-4.252-7.414-4.573-8.115-.28-.394-.53-.954-.735-1.44\
-.036.495-.055.685-.523 1.184-.723.566-4.438 3.682-4.74 10.02\
-.282 5.912 4.27 9.435 4.888 9.884l.07.05A73.49 73.49 0 0111.91 24h.481\
c.114-1.032.284-2.056.51-3.07.417-.296.604-.463.85-.693\
a11.342 11.342 0 003.639-8.464c.01-.814-.103-1.662-.197-2.218z\
m-5.336 8.195s0-8.291.275-8.29c.213 0 .49 10.695.49 10.695-.381-.045-.765-1.76-.765-2.405z";

const REDIS_PATH: &str = "\
M22.71 13.145c-1.66 2.092-3.452 4.483-7.038 4.483-3.203 0-4.397-2.825-4.48-5.12\
.701 1.484 2.073 2.685 4.214 2.63 4.117-.133 6.94-3.852 6.94-7.239 \
0-4.05-3.022-6.972-8.268-6.972-3.752 0-8.4 1.428-11.455 3.685\
C2.59 6.937 3.885 9.958 4.35 9.626c2.648-1.904 4.748-3.13 6.784-3.744\
C8.12 9.244.886 17.05 0 18.425c.1 1.261 1.66 4.648 2.424 4.648\
.232 0 .431-.133.664-.365a100.49 100.49 0 0 0 5.54-6.765\
c.222 3.104 1.748 6.898 6.014 6.898 3.819 0 7.604-2.756 9.33-8.965\
.2-.764-.73-1.361-1.261-.73zm-4.349-5.013c0 1.959-1.926 2.922-3.685 2.922\
-.941 0-1.664-.247-2.235-.568 1.051-1.592 2.092-3.225 3.21-4.973 \
1.972.334 2.71 1.43 2.71 2.619z";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_and_implicit_commands() -> Result<()> {
        let segs = parse_svg_path("M0 0 24 0V24H0z", 24.0)?;
        assert_eq!(segs.len(), 5);
        assert_eq!(segs[0], PathSegment::MoveTo { x: 0.0, y: 0.0 });
        assert_eq!(segs[1], PathSegment::LineTo { x: 16.0, y: 0.0 });
        assert_eq!(segs[2], PathSegment::LineTo { x: 16.0, y: 16.0 });
        assert_eq!(segs[3], PathSegment::LineTo { x: 0.0, y: 16.0 });
        assert_eq!(segs[4], PathSegment::Close);
        Ok(())
    }

    #[test]
    fn parse_compact_arc_flags_and_beziers() -> Result<()> {
        let segs = parse_svg_path("M4 12a4 4 0 018 0q2-4 4 0t4 0", 16.0)?;
        assert!(matches!(segs[0], PathSegment::MoveTo { .. }));
        assert!(segs.iter().any(|s| matches!(s, PathSegment::CubicTo { .. })));
        assert!(segs.iter().any(|s| matches!(s, PathSegment::QuadTo { .. })));
        Ok(())
    }

    #[test]
    fn invalid_svg_path_returns_error() {
        assert!(parse_svg_path("M0 0", 0.0).is_err());
        assert!(parse_svg_path("X10 10", 16.0).is_err());
        assert!(parse_svg_path("10 10", 16.0).is_err());
    }

    #[test]
    fn all_eleven_engines_and_app_icon_parse_to_16x16_bounds() -> Result<()> {
        assert_eq!(EngineKind::ALL.len(), 11);
        for engine in EngineKind::ALL {
            let glyph = match engine.glyph() {
                Ok(g) => g,
                Err(e) => panic_test_err(engine, &e),
            };
            assert!(!glyph.segments.is_empty());
            assert!(matches!(glyph.segments[0], PathSegment::MoveTo { .. }));
            for seg in &glyph.segments {
                match *seg {
                    PathSegment::MoveTo { x, y } | PathSegment::LineTo { x, y } => {
                        assert!((-1.0..=17.0).contains(&x), "{} x={x}", glyph.name);
                        assert!((-1.0..=17.0).contains(&y), "{} y={y}", glyph.name);
                    }
                    _ => {}
                }
            }
        }
        let icon = app_icon_glyph()?;
        assert_eq!(icon.name, "DataForge");
        assert!(!icon.segments.is_empty());
        Ok(())
    }

    fn panic_test_err(engine: EngineKind, err: &GfxError) -> ! {
        assert_eq!(engine.display_name(), "", "failed to parse glyph: {err}");
        unreachable!()
    }
}
