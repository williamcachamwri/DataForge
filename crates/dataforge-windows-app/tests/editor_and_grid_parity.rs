//! Pure-Rust parity tests mirroring `TablePro.Editor.Model.Tests` & `TablePro.DataGrid.Tests`:
//! - SQL Find & Replace (case-sensitive, whole-word, replace-one, replace-all, literals & regex chars)
//! - SQL Formatter (keyword uppercasing, indentation, idempotence, comments, literals, `$dollar$` blocks)
//! - DataGrid Aggregates (`Count`, `NonNull`, `NumericCount`, `Sum`, `Avg`, `Min`, `Max`)
//! - Multi-row TSV copy (`format_rows_as_tsv`) & Pinned Columns (`toggle_pin_column`, `is_column_pinned`)

use dataforge_windows_app::app::AppController;
use dataforge_windows_app::controller_types::GridAggregateStats;
use dataforge_windows_app::i18n::Locale;
use dataforge_windows_platform::{AppSettings, WindowsTheme};
use tablepro_core::column::{ColumnType, ResultColumn};
use tablepro_core::value::Value;

fn load_sample_grid(app: &mut AppController) {
    let cols = vec![
        ResultColumn::new("id", ColumnType::untyped_text("INTEGER")),
        ResultColumn::new("score", ColumnType::untyped_text("REAL")),
        ResultColumn::new("note", ColumnType::untyped_text("TEXT")),
    ];
    let rows = vec![
        vec![
            Value::Int(10),
            Value::Float64(2.5),
            Value::Text("alpha".to_owned()),
        ],
        vec![
            Value::Int(20),
            Value::Null,
            Value::Text("beta\tgamma".to_owned()),
        ],
        vec![
            Value::Null,
            Value::Float64(7.5),
            Value::Text("3.0".to_owned()),
        ],
        vec![Value::Int(30), Value::Float64(5.0), Value::Null],
    ];
    let total = Some(rows.len() as u64);
    app.grid_raw_columns = cols.clone();
    app.grid_raw_rows = rows.clone();
    app.grid_model.reload_rows(cols, rows, total);
    app.sync_grid_view_from_model();
}

#[test]
fn editor_find_replace_case_whole_word_and_special_chars() {
    let mut app = AppController::new(AppSettings::default(), WindowsTheme::Dark, Locale::EnUs);

    // Case-insensitive single replacement.
    app.set_editor_sql("SELECT Users.id FROM users WHERE users_count > 0;");
    let single = app.find_replace_in_editor("users", "accounts", false, false, false);
    assert_eq!(single, 1);
    assert_eq!(
        app.editor_sql_text(),
        "SELECT accounts.id FROM users WHERE users_count > 0;"
    );

    // Case-sensitive + whole-word replace-all skips `a_col` and `'a'`.
    app.set_editor_sql("SELECT a, A, a_col, 'keep' FROM a;");
    let whole_count = app.find_replace_in_editor("a", "b", true, true, true);
    assert_eq!(whole_count, 2);
    assert_eq!(app.editor_sql_text(), "SELECT b, A, a_col, 'keep' FROM b;");

    // Preserves string literals untouched when searching outside literal content,
    // and handles regex metacharacters literally (`.+*?[]()`).
    app.set_editor_sql("SELECT price * (1.0 + tax) AS total, 'price * (1.0 + tax)' AS label;");
    let regex_chars =
        app.find_replace_in_editor("price * (1.0 + tax)", "net_amt", true, false, false);
    assert_eq!(regex_chars, 1);
    assert_eq!(
        app.editor_sql_text(),
        "SELECT net_amt AS total, 'price * (1.0 + tax)' AS label;"
    );

    // Empty find needle is a clean no-op.
    assert_eq!(app.find_replace_in_editor("", "x", false, false, true), 0);
}

#[test]
fn editor_sql_formatter_keywords_indentation_idempotence_and_dollar_blocks() {
    let mut app = AppController::new(AppSettings::default(), WindowsTheme::Dark, Locale::EnUs);

    let raw = "select u.id, 'select from where' as txt -- keep lowercase comment\nfrom users u left join orders o on o.user_id = u.id where u.active = true order by u.id desc limit 10;";
    app.set_editor_sql(raw);
    app.format_editor_sql();
    let once = app.editor_sql_text();

    assert!(once.contains("SELECT"));
    assert!(once.contains("FROM"));
    assert!(once.contains("LEFT JOIN"));
    assert!(once.contains("WHERE"));
    assert!(once.contains("ORDER BY"));
    assert!(once.contains("LIMIT"));
    assert!(once.contains("'select from where'"));
    assert!(once.contains("-- keep lowercase comment"));

    // Idempotence: second format pass leaves formatted SQL unchanged.
    app.format_editor_sql();
    assert_eq!(app.editor_sql_text(), once);

    // Dollar-quoted PL/pgSQL block preservation.
    let plpgsql = "create function greet() returns text as $fn$\nbegin return 'select from'; end;\n$fn$ language plpgsql;";
    app.set_editor_sql(plpgsql);
    app.format_editor_sql();
    let formatted_fn = app.editor_sql_text();
    assert!(formatted_fn.contains("CREATE"));
    assert!(formatted_fn.contains("$fn$\nbegin return 'select from'; end;\n$fn$"));
}

#[test]
fn datagrid_aggregates_handle_mixed_types_nulls_and_empty_selection() {
    let mut app = AppController::new(AppSettings::default(), WindowsTheme::Dark, Locale::EnUs);
    load_sample_grid(&mut app);

    // Empty selection returns zeroed stats.
    assert_eq!(
        app.compute_grid_aggregates(&[], None),
        GridAggregateStats::default()
    );

    // Single-row, single-column selection (`id` = 20).
    let single = app.compute_grid_aggregates(&[1], Some(0));
    assert_eq!(single.count, 1);
    assert_eq!(single.non_null_count, 1);
    assert_eq!(single.numeric_count, 1);
    assert!((single.sum - 20.0).abs() < 1e-9);
    assert!((single.avg - 20.0).abs() < 1e-9);
    assert!((single.min - 20.0).abs() < 1e-9);
    assert!((single.max - 20.0).abs() < 1e-9);

    // Multi-row column `score` (2.5, NULL, 7.5, 5.0).
    let score_stats = app.compute_grid_aggregates(&[0, 1, 2, 3], Some(1));
    assert_eq!(score_stats.count, 4);
    assert_eq!(score_stats.non_null_count, 3);
    assert_eq!(score_stats.numeric_count, 3);
    assert!((score_stats.sum - 15.0).abs() < 1e-9);
    assert!((score_stats.avg - 5.0).abs() < 1e-9);
    assert!((score_stats.min - 2.5).abs() < 1e-9);
    assert!((score_stats.max - 7.5).abs() < 1e-9);

    // Multi-row across ALL columns: ints (10,20,30) + floats (2.5,7.5,5.0) + numeric text ("3.0").
    // Total cells = 12, nulls = 3 (`id` row 2, `score` row 1, `note` row 3), non_null = 9, numeric = 7.
    let all_stats = app.compute_grid_aggregates(&[0, 1, 2, 3], None);
    assert_eq!(all_stats.count, 12);
    assert_eq!(all_stats.non_null_count, 9);
    assert_eq!(all_stats.numeric_count, 7);
    assert!((all_stats.sum - 78.0).abs() < 1e-9);
    assert!((all_stats.min - 2.5).abs() < 1e-9);
    assert!((all_stats.max - 30.0).abs() < 1e-9);
}

#[test]
fn datagrid_multi_row_tsv_copy_and_pinned_columns() {
    let mut app = AppController::new(AppSettings::default(), WindowsTheme::Light, Locale::EnUs);
    load_sample_grid(&mut app);

    // Pinned columns toggle & lookup.
    assert!(!app.is_column_pinned("id"));
    app.toggle_pin_column("id");
    app.toggle_pin_column("score");
    assert!(app.is_column_pinned("id"));
    assert!(app.is_column_pinned("score"));
    app.toggle_pin_column("id");
    assert!(!app.is_column_pinned("id"));
    assert!(app.is_column_pinned("score"));

    // Empty row selection produces empty TSV.
    assert_eq!(app.format_rows_as_tsv(&[]), "");

    // Multi-row TSV copy reflects pending inline edits and escapes tabs cleanly.
    app.edit_grid_cell(0, 2, Value::Text("edited_alpha".to_owned()));
    let tsv = app.format_rows_as_tsv(&[0, 1, 3]);
    let lines: Vec<&str> = tsv.lines().collect();
    assert_eq!(lines.len(), 3);
    assert!(lines[0].contains("10\t2.5\tedited_alpha"));
    assert!(lines[1].starts_with("20\t"));
    assert!(lines[2].starts_with("30\t5"));
}
