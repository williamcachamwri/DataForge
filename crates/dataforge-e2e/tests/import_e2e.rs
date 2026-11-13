//! Foreign Import (TablePlus, DBeaver, DataGrip, Navicat, Sequel Ace) and
//! connection URL parser into `ConnectionsLibraryModel` E2E tests.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use dataforge_e2e::E2eResult;
use dataforge_state::library::{
    ConnectionsLibraryModel, connection_from_url, parse_connection_url,
};
use tablepro_foreign_import::{
    ForeignApp, ForeignAppDirs, ForeignImporter, ForeignPlatform, ImportRequest, PlainHost,
};
use tokio_util::sync::CancellationToken;

fn default_request() -> ImportRequest {
    ImportRequest {
        include_passwords: false,
        include_saved_queries: true,
        saved_query_byte_limit: 65_536,
    }
}

fn run_import(
    app: ForeignApp,
    dirs: ForeignAppDirs,
    picked_file: Option<std::path::PathBuf>,
) -> E2eResult<tablepro_foreign_import::CollectedImport> {
    let mut importer = ForeignImporter::new(app, dirs, Arc::new(PlainHost));
    if let Some(path) = picked_file {
        importer = importer.with_file(path);
    }
    assert!(importer.is_available_blocking(), "{app:?} should be available");
    let inv = importer.inventory_blocking();
    assert!(inv.connections >= 1, "{app:?} inventory should find >= 1 connection");
    Ok(importer.collect_blocking(&default_request(), &CancellationToken::new())?)
}

#[test]
fn import_tableplus_into_library() -> E2eResult<()> {
    let dir = tempfile::tempdir()?;
    let dirs = ForeignAppDirs::conventional(ForeignPlatform::Windows, dir.path());
    let data_dir = dirs.data.join("com.tinyapp.TablePlus").join("Data");
    fs::create_dir_all(&data_dir)?;

    let mut entry = plist::Dictionary::new();
    entry.insert("ID".into(), plist::Value::String("tp-pg-1".into()));
    entry.insert("ConnectionName".into(), plist::Value::String("TP Prod PG".into()));
    entry.insert("Driver".into(), plist::Value::String("PostgreSQL".into()));
    entry.insert("DatabaseHost".into(), plist::Value::String("pg.example.com".into()));
    entry.insert("DatabasePort".into(), plist::Value::String("5432".into()));
    entry.insert("DatabaseName".into(), plist::Value::String("app".into()));
    entry.insert("DatabaseUser".into(), plist::Value::String("postgres".into()));
    plist::Value::Array(vec![plist::Value::Dictionary(entry)])
        .to_file_binary(data_dir.join("Connections.plist"))?;

    let collected = run_import(ForeignApp::TablePlus, dirs, None)?;
    assert_eq!(collected.connections.len(), 1);

    let mut library = ConnectionsLibraryModel::new();
    let report = library.import_foreign(&collected);
    assert_eq!(report.imported_count, 1);
    assert!(
        library
            .outline()
            .iter()
            .any(|row| !row.is_group && row.label == "TP Prod PG")
    );
    Ok(())
}

#[test]
fn import_dbeaver_into_library() -> E2eResult<()> {
    let dir = tempfile::tempdir()?;
    let dirs = ForeignAppDirs::conventional(ForeignPlatform::Windows, dir.path());
    let dbeaver_dir = dirs
        .config
        .join("DBeaverData")
        .join("workspace6")
        .join("General")
        .join(".dbeaver");
    fs::create_dir_all(&dbeaver_dir)?;

    let json = serde_json::json!({
        "connections": {
            "mysql-1": {
                "provider": "mysql",
                "driver": "mysql8",
                "name": "DBeaver MySQL",
                "configuration": {
                    "host": "mysql.internal",
                    "port": "3306",
                    "database": "orders",
                    "user": "readonly"
                }
            }
        }
    });
    fs::write(
        dbeaver_dir.join("data-sources.json"),
        serde_json::to_vec_pretty(&json)?,
    )?;

    let collected = run_import(ForeignApp::DBeaver, dirs, None)?;
    assert_eq!(collected.connections.len(), 1);

    let mut library = ConnectionsLibraryModel::new();
    let report = library.import_foreign(&collected);
    assert_eq!(report.imported_count, 1);
    assert!(
        library
            .outline()
            .iter()
            .any(|row| !row.is_group && row.label == "DBeaver MySQL")
    );
    Ok(())
}

#[test]
fn import_datagrip_into_library() -> E2eResult<()> {
    let dir = tempfile::tempdir()?;
    let dirs = ForeignAppDirs::conventional(ForeignPlatform::Windows, dir.path());
    let options_dir = dirs
        .config
        .join("JetBrains")
        .join("DataGrip2024.3")
        .join("options");
    fs::create_dir_all(&options_dir)?;

    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<application>
  <component name="DataSourceManagerImpl" format="xml" multifile-model="true">
    <data-source source="LOCAL" name="DataGrip DuckDB" uuid="dg-1">
      <driver-ref>duckdb</driver-ref>
      <jdbc-driver>org.duckdb.DuckDBDriver</jdbc-driver>
      <jdbc-url>jdbc:duckdb:/tmp/analytics.duckdb</jdbc-url>
    </data-source>
  </component>
</application>"#;
    fs::write(options_dir.join("dataSources.xml"), xml)?;

    let collected = run_import(ForeignApp::DataGrip, dirs, None)?;
    assert_eq!(collected.connections.len(), 1);

    let mut library = ConnectionsLibraryModel::new();
    let report = library.import_foreign(&collected);
    assert_eq!(report.imported_count, 1);
    assert!(
        library
            .outline()
            .iter()
            .any(|row| !row.is_group && row.label == "DataGrip DuckDB")
    );
    Ok(())
}

#[test]
fn import_navicat_ncx_into_library() -> E2eResult<()> {
    let dir = tempfile::tempdir()?;
    let dirs = ForeignAppDirs::conventional(ForeignPlatform::Windows, dir.path());
    let ncx_path = dir.path().join("connections.ncx");

    let ncx = r#"<?xml version="1.0" encoding="UTF-8"?>
<Connections Ver="1.2">
  <Connection ConnectionName="Navicat PG" ConnType="PostgreSQL" Host="db.prod.local" Port="5432" Database="core" UserName="svc_ro" />
</Connections>"#;
    fs::write(&ncx_path, ncx)?;

    let collected = run_import(ForeignApp::Navicat, dirs, Some(ncx_path))?;
    assert_eq!(collected.connections.len(), 1);

    let mut library = ConnectionsLibraryModel::new();
    let report = library.import_foreign(&collected);
    assert_eq!(report.imported_count, 1);
    assert!(
        library
            .outline()
            .iter()
            .any(|row| !row.is_group && row.label == "Navicat PG")
    );
    Ok(())
}

#[test]
fn import_sequel_ace_into_library() -> E2eResult<()> {
    let dir = tempfile::tempdir()?;
    let dirs = ForeignAppDirs::conventional(ForeignPlatform::MacOs, dir.path());
    let fav_dir = Path::new(&dirs.home)
        .join("Library")
        .join("Containers")
        .join("com.sequel-ace.sequel-ace")
        .join("Data")
        .join("Library")
        .join("Application Support")
        .join("Sequel Ace")
        .join("Data");
    fs::create_dir_all(&fav_dir)?;

    let mut conn = plist::Dictionary::new();
    conn.insert("name".into(), plist::Value::String("Sequel MySQL".into()));
    conn.insert("host".into(), plist::Value::String("127.0.0.1".into()));
    conn.insert("port".into(), plist::Value::Integer(3306.into()));
    conn.insert("user".into(), plist::Value::String("root".into()));
    conn.insert("database".into(), plist::Value::String("sakila".into()));
    conn.insert("type".into(), plist::Value::Integer(0.into()));

    let mut fav_root = plist::Dictionary::new();
    fav_root.insert(
        "Children".into(),
        plist::Value::Array(vec![plist::Value::Dictionary(conn)]),
    );
    let mut root = plist::Dictionary::new();
    root.insert("Favorites Root".into(), plist::Value::Dictionary(fav_root));
    plist::Value::Dictionary(root).to_file_xml(fav_dir.join("Favorites.plist"))?;

    let collected = run_import(ForeignApp::SequelAce, dirs, None)?;
    assert_eq!(collected.connections.len(), 1);

    let mut library = ConnectionsLibraryModel::new();
    let report = library.import_foreign(&collected);
    assert_eq!(report.imported_count, 1);
    assert!(
        library
            .outline()
            .iter()
            .any(|row| !row.is_group && row.label == "Sequel MySQL")
    );
    Ok(())
}

#[test]
fn parse_connection_urls_into_library() -> E2eResult<()> {
    let cases = [
        ("postgres://alice@pg.local:5432/analytics", "PG Analytics", "postgres"),
        ("mysql://bob@mysql.local:3306/shop", "MySQL Shop", "mysql"),
        ("redis://redis.local:6379/0", "Redis Cache", "redis"),
        ("mongodb://mongo.local:27017/events", "Mongo Events", "mongodb"),
        ("clickhouse://ch.local:8123/logs", "ClickHouse Logs", "clickhouse"),
    ];

    let mut library = ConnectionsLibraryModel::new();
    for (raw_url, label, expected_driver) in cases {
        let parsed = parse_connection_url(raw_url)?;
        let conn = connection_from_url(&parsed, Some(label));
        assert_eq!(conn.driver_id, expected_driver);
        assert_eq!(conn.name, label);
        library.upsert_connection(conn);
    }

    let outline = library.outline();
    let count = outline.iter().filter(|r| !r.is_group).count();
    assert_eq!(count, 5);

    library.set_search_query("ClickHouse");
    let filtered: Vec<&str> = library
        .outline()
        .iter()
        .filter(|r| !r.is_group)
        .map(|r| r.label.as_str())
        .collect();
    assert_eq!(filtered, vec!["ClickHouse Logs"]);

    Ok(())
}
