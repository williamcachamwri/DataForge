//! Normalized TablePro vector icons for all 11 database engines rendered cleanly
//! via GPUI `canvas` + `PathBuilder` (zero Direct2D dependency).

use dataforge_gfx::glyphs::{EngineKind, PathSegment, parse_svg_path};
use herogpui::gpui::{
    AnyElement, Hsla, IntoElement, Path, PathBuilder, Pixels, Point, Styled, canvas, point, px,
};

/// Canonical normalized extent (`16x16` DIPs) returned by [`parse_svg_path`].
pub const NORMALIZED_ICON_EXTENT: f32 = 16.0;

/// PostgreSQL SVG path (`24x24` viewBox).
pub const POSTGRESQL_SVG_PATH: &str = "\
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

/// MySQL SVG path (`24x24` viewBox).
pub const MYSQL_SVG_PATH: &str = "\
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

/// MariaDB SVG path (`24x24` viewBox).
pub const MARIADB_SVG_PATH: &str = "\
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

/// SQLite document outline SVG path (`24x24` viewBox, hollow frame).
pub const SQLITE_DOC_SVG_PATH: &str = "\
M3.4 1.2H13.6A2.2 2.2 0 0 1 15.8 3.4V19.6A2.2 2.2 0 0 1 13.6 21.8H3.4A2.2 2.2 0 0 1 1.2 19.6V3.4A2.2 2.2 0 0 1 3.4 1.2Z\
M3.7 3.2A0.5 0.5 0 0 0 3.2 3.7V19.3A0.5 0.5 0 0 0 3.7 19.8H13.3A0.5 0.5 0 0 0 13.8 19.3V3.7A0.5 0.5 0 0 0 13.3 3.2H3.7Z";

/// SQLite feather SVG path (`24x24` viewBox).
pub const SQLITE_SVG_PATH: &str = "\
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
s1.945-1.184 3.613-1.72C21.695 7.9 24.195 2.767 21.678.521Z";

/// SQLite SVG paths (`24x24` viewBox): hollow document outline + feather.
pub const SQLITE_SVG_PATHS: &[&str] = &[SQLITE_DOC_SVG_PATH, SQLITE_SVG_PATH];

/// DuckDB SVG path (`24x24` viewBox).
pub const DUCKDB_SVG_PATH: &str = "\
M12 0C5.363 0 0 5.363 0 12s5.363 12 12 12 12-5.363 12-12S18.637 0 12 0z\
M9.502 7.03a4.974 4.974 0 0 1 4.97 4.97 4.974 4.974 0 0 1-4.97 4.97\
A4.974 4.974 0 0 1 4.532 12a4.974 4.974 0 0 1 4.97-4.97z\
m6.563 3.183h2.351c.98 0 1.787.782 1.787 1.762s-.807 1.789-1.787 1.789h-2.351v-3.551z";

/// Microsoft SQL Server SVG paths (`48x48` viewBox).
pub const MSSQL_SVG_PATHS: &[&str] = &[
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

/// ClickHouse SVG path (`24x24` viewBox).
pub const CLICKHOUSE_SVG_PATH: &str = "\
M21.333 10H24v4h-2.667ZM16 1.335h2.667v21.33H16Zm-5.333 0h2.666v21.33h-2.666Z\
M0 22.665V1.335h2.667v21.33zm5.333-21.33H8v21.33H5.333Z";

/// Apache Cassandra SVG paths (`32x32` viewBox).
pub const CASSANDRA_SVG_PATHS: &[&str] = &[
    "M16 2C8.268 2 2 8.268 2 16s6.268 14 14 14 14-6.268 14-14S23.732 2 16 2z\
m0 2c6.627 0 12 5.373 12 12s-5.373 12-12 12S4 22.627 4 16 9.373 4 16 4z",
    "M16 7c-1.1 0-2 .5-2.6 1.3L9.8 13c-.4.5-.6 1.1-.6 1.7v2.6c0 .6.2 1.2.6 1.7l3.6 4.7\
c.6.8 1.5 1.3 2.6 1.3s2-.5 2.6-1.3l3.6-4.7c.4-.5.6-1.1.6-1.7v-2.6c0-.6-.2-1.2-.6-1.7l-3.6-4.7\
C18 7.5 17.1 7 16 7zm0 2c.4 0 .7.2.9.4l3.6 4.7c.1.2.2.4.2.6v2.6c0 .2-.1.4-.2.6L16.9 22.6\
c-.2.3-.5.4-.9.4s-.7-.2-.9-.4l-3.6-4.7c-.1-.2-.2-.4-.2-.6v-2.6c0-.2.1-.4.2-.6L15.1 9.4\
c.2-.3.5-.4.9-.4z",
];

/// ScyllaDB SVG paths (`32x32` viewBox).
pub const SCYLLADB_SVG_PATHS: &[&str] = &[
    "M16 2C8.268 2 2 8.268 2 16s6.268 14 14 14 14-6.268 14-14S23.732 2 16 2z\
m0 2c6.627 0 12 5.373 12 12s-5.373 12-12 12S4 22.627 4 16 9.373 4 16 4z",
    "M16 8c-4.4 0-8 3.6-8 8s3.6 8 8 8 8-3.6 8-8-3.6-8-8-8z\
m0 2c3.3 0 6 2.7 6 6s-2.7 6-6 6-6-2.7-6-6 2.7-6 6-6z",
    "M13 16a3 3 0 1 0 6 0 3 3 0 1 0-6 0z",
];

/// MongoDB SVG path (`24x24` viewBox).
pub const MONGODB_SVG_PATH: &str = "\
M17.193 9.555c-1.264-5.58-4.252-7.414-4.573-8.115-.28-.394-.53-.954-.735-1.44\
-.036.495-.055.685-.523 1.184-.723.566-4.438 3.682-4.74 10.02\
-.282 5.912 4.27 9.435 4.888 9.884l.07.05A73.49 73.49 0 0111.91 24h.481\
c.114-1.032.284-2.056.51-3.07.417-.296.604-.463.85-.693\
a11.342 11.342 0 003.639-8.464c.01-.814-.103-1.662-.197-2.218z\
m-5.336 8.195s0-8.291.275-8.29c.213 0 .49 10.695.49 10.695-.381-.045-.765-1.76-.765-2.405z";

/// Redis SVG path (`24x24` viewBox).
pub const REDIS_SVG_PATH: &str = "\
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

/// Viewbox extent and SVG path strings for a database engine glyph.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EngineSvgSpec {
    pub engine: EngineKind,
    pub viewbox: f32,
    pub paths: &'static [&'static str],
}

/// All 11 TablePro engine SVG icon specs in canonical [`EngineKind::ALL`] order.
pub const ENGINE_SVG_SPECS: [EngineSvgSpec; 11] = [
    EngineSvgSpec {
        engine: EngineKind::Postgresql,
        viewbox: 24.0,
        paths: &[POSTGRESQL_SVG_PATH],
    },
    EngineSvgSpec {
        engine: EngineKind::Mysql,
        viewbox: 24.0,
        paths: &[MYSQL_SVG_PATH],
    },
    EngineSvgSpec {
        engine: EngineKind::Mariadb,
        viewbox: 24.0,
        paths: &[MARIADB_SVG_PATH],
    },
    EngineSvgSpec {
        engine: EngineKind::Sqlite,
        viewbox: 24.0,
        paths: SQLITE_SVG_PATHS,
    },
    EngineSvgSpec {
        engine: EngineKind::Duckdb,
        viewbox: 24.0,
        paths: &[DUCKDB_SVG_PATH],
    },
    EngineSvgSpec {
        engine: EngineKind::Mssql,
        viewbox: 48.0,
        paths: MSSQL_SVG_PATHS,
    },
    EngineSvgSpec {
        engine: EngineKind::Clickhouse,
        viewbox: 24.0,
        paths: &[CLICKHOUSE_SVG_PATH],
    },
    EngineSvgSpec {
        engine: EngineKind::Cassandra,
        viewbox: 32.0,
        paths: CASSANDRA_SVG_PATHS,
    },
    EngineSvgSpec {
        engine: EngineKind::Scylladb,
        viewbox: 32.0,
        paths: SCYLLADB_SVG_PATHS,
    },
    EngineSvgSpec {
        engine: EngineKind::Mongodb,
        viewbox: 24.0,
        paths: &[MONGODB_SVG_PATH],
    },
    EngineSvgSpec {
        engine: EngineKind::Redis,
        viewbox: 24.0,
        paths: &[REDIS_SVG_PATH],
    },
];

/// Returns the [`EngineSvgSpec`] for `engine`.
#[must_use]
pub const fn engine_svg_spec(engine: EngineKind) -> EngineSvgSpec {
    match engine {
        EngineKind::Postgresql => ENGINE_SVG_SPECS[0],
        EngineKind::Mysql => ENGINE_SVG_SPECS[1],
        EngineKind::Mariadb => ENGINE_SVG_SPECS[2],
        EngineKind::Sqlite => ENGINE_SVG_SPECS[3],
        EngineKind::Duckdb => ENGINE_SVG_SPECS[4],
        EngineKind::Mssql => ENGINE_SVG_SPECS[5],
        EngineKind::Clickhouse => ENGINE_SVG_SPECS[6],
        EngineKind::Cassandra => ENGINE_SVG_SPECS[7],
        EngineKind::Scylladb => ENGINE_SVG_SPECS[8],
        EngineKind::Mongodb => ENGINE_SVG_SPECS[9],
        EngineKind::Redis => ENGINE_SVG_SPECS[10],
    }
}

/// Tessellates the vector paths for `engine` scaled to `size_px` at `origin`
/// using GPUI's [`PathBuilder`].
#[must_use]
pub fn build_engine_paths(
    engine: EngineKind,
    size_px: f32,
    origin: Point<Pixels>,
) -> Vec<Path<Pixels>> {
    let spec = engine_svg_spec(engine);
    let scale = size_px.max(1.0) / NORMALIZED_ICON_EXTENT;
    let map_pt = |x: f32, y: f32| -> Point<Pixels> {
        point(
            px(origin.x.as_f32() + x * scale),
            px(origin.y.as_f32() + y * scale),
        )
    };

    let mut out = Vec::with_capacity(spec.paths.len());
    for raw_d in spec.paths {
        let Ok(segments) = parse_svg_path(raw_d, spec.viewbox) else {
            continue;
        };
        if segments.is_empty() {
            continue;
        }
        let mut builder = PathBuilder::fill();
        for seg in segments {
            match seg {
                PathSegment::MoveTo { x, y } => builder.move_to(map_pt(x, y)),
                PathSegment::LineTo { x, y } => builder.line_to(map_pt(x, y)),
                PathSegment::QuadTo { cx, cy, x, y } => {
                    builder.curve_to(map_pt(x, y), map_pt(cx, cy));
                }
                PathSegment::CubicTo {
                    c1x,
                    c1y,
                    c2x,
                    c2y,
                    x,
                    y,
                } => {
                    builder.cubic_bezier_to(map_pt(x, y), map_pt(c1x, c1y), map_pt(c2x, c2y));
                }
                PathSegment::Close => builder.close(),
            }
        }
        if let Ok(path) = builder.build() {
            out.push(path);
        }
    }
    out
}

/// Renders a crisp GPU vector icon for `engine` at `size` using `color`.
#[must_use]
pub fn engine_icon(engine: EngineKind, size: Pixels, color: Hsla) -> AnyElement {
    let min_side = size.as_f32().max(1.0);
    canvas(
        move |bounds, _window, _cx| {
            let side = bounds
                .size
                .width
                .as_f32()
                .min(bounds.size.height.as_f32())
                .max(min_side);
            let origin = point(
                px(bounds.origin.x.as_f32() + (bounds.size.width.as_f32() - side) * 0.5),
                px(bounds.origin.y.as_f32() + (bounds.size.height.as_f32() - side) * 0.5),
            );
            build_engine_paths(engine, side, origin)
        },
        move |_bounds, paths, window, _cx| {
            for path in paths {
                window.paint_path(path, color);
            }
        },
    )
    .w(size)
    .h(size)
    .flex_none()
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_eleven_engines_export_and_tessellate_cleanly() {
        assert_eq!(EngineKind::ALL.len(), 11);
        for engine in EngineKind::ALL {
            let spec = engine_svg_spec(engine);
            assert_eq!(spec.engine, engine);
            assert!(!spec.paths.is_empty());

            let paths = build_engine_paths(engine, 16.0, point(px(0.0), px(0.0)));
            assert_eq!(
                paths.len(),
                spec.paths.len(),
                "expected every SVG subpath for {} to tessellate",
                engine.display_name()
            );
        }
    }
}
