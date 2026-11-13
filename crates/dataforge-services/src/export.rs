//! Streaming CSV, JSON, and SQL export with `.part` atomic write and rename.

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tablepro_core::column::ResultColumn;
use tablepro_core::dialect::SqlDialect;
use tablepro_core::export::{
    CsvEncoder, CsvOptions, JsonArrayEncoder, JsonLinesEncoder, RowEncoder, SqlInsertEncoder,
};
use tablepro_core::meta::TableRef;
use tablepro_core::value::Value;
use tablepro_core::{ReadStatement, ResultEvent, ResultSink, SinkFlow, StreamSummary};
use tablepro_session::Session;
use tablepro_session::operation::{OperationError, OperationKind};
use tokio_util::sync::CancellationToken;

use crate::error::ServiceError;

/// Supported streaming export formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Csv,
    JsonArray,
    JsonLines,
    SqlInsert,
}

/// Options controlling how rows are formatted during export.
#[derive(Clone)]
pub struct ExportOptions {
    pub format: ExportFormat,
    pub csv: CsvOptions,
    pub table: Option<TableRef>,
    pub dialect: Option<Arc<dyn SqlDialect>>,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            format: ExportFormat::Csv,
            csv: CsvOptions::default(),
            table: None,
            dialect: None,
        }
    }
}

/// Computes the `.part` staging path alongside `target`.
#[must_use]
pub fn part_path_for(target: &Path) -> PathBuf {
    let mut os = target.as_os_str().to_os_string();
    os.push(".part");
    PathBuf::from(os)
}

/// Writes `columns` and `rows` atomically to `target` via a `.part` staging file.
///
/// # Errors
///
/// Returns [`ServiceError::Export`] or [`ServiceError::Io`] if encoding, writing, or renaming fails.
pub fn export_rows_atomic(
    target: &Path,
    columns: &[ResultColumn],
    rows: &[Vec<Value>],
    options: &ExportOptions,
) -> Result<u64, ServiceError> {
    if let Some(parent) = target.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    let part_path = part_path_for(target);
    let result = write_rows_to_part(&part_path, columns, rows, options);
    match result {
        Ok(count) => {
            fs::rename(&part_path, target)?;
            Ok(count)
        }
        Err(err) => {
            let _ = fs::remove_file(&part_path);
            Err(err)
        }
    }
}

fn write_rows_to_part(
    part_path: &Path,
    columns: &[ResultColumn],
    rows: &[Vec<Value>],
    options: &ExportOptions,
) -> Result<u64, ServiceError> {
    let file = File::create(part_path)?;
    let mut writer = BufWriter::new(file);
    let mut encoder = build_encoder(columns, options)?;
    let mut buf = Vec::with_capacity(4096);
    let mut count = 0_u64;

    encoder.begin(&mut buf);
    if !buf.is_empty() {
        writer.write_all(&buf)?;
        buf.clear();
    }
    for row in rows {
        encoder
            .row(&mut buf, &mut row.iter())
            .map_err(|err| ServiceError::Export(err.to_string()))?;
        writer.write_all(&buf)?;
        buf.clear();
        count = count.saturating_add(1);
    }
    encoder.finish(&mut buf);
    if !buf.is_empty() {
        writer.write_all(&buf)?;
    }
    writer.flush()?;
    writer.get_ref().sync_all()?;
    Ok(count)
}

fn build_encoder(
    columns: &[ResultColumn],
    options: &ExportOptions,
) -> Result<Box<dyn RowEncoder>, ServiceError> {
    let cols: Arc<[ResultColumn]> = columns.into();
    match options.format {
        ExportFormat::Csv => Ok(Box::new(CsvEncoder::new(cols, options.csv.clone()))),
        ExportFormat::JsonArray => Ok(Box::new(JsonArrayEncoder::new(cols))),
        ExportFormat::JsonLines => Ok(Box::new(JsonLinesEncoder::new(cols))),
        ExportFormat::SqlInsert => {
            let table = options.table.as_ref().ok_or_else(|| {
                ServiceError::Export("SQL export requires a target TableRef".to_owned())
            })?;
            let dialect = options.dialect.as_ref().ok_or_else(|| {
                ServiceError::Export("SQL export requires a SqlDialect".to_owned())
            })?;
            let enc = SqlInsertEncoder::new(dialect.clone(), table, cols, &[], &[]);
            Ok(Box::new(enc))
        }
    }
}

struct StreamingExportState {
    writer: BufWriter<File>,
    encoder: Option<Box<dyn RowEncoder>>,
    buf: Vec<u8>,
    options: ExportOptions,
    rows_written: u64,
    error: Option<ServiceError>,
}

impl StreamingExportState {
    fn handle_event(&mut self, event: ResultEvent) -> SinkFlow {
        if self.error.is_some() {
            return SinkFlow::ConsumerGone;
        }
        match event {
            ResultEvent::ResultSetStarted { columns } => self.start_encoder(&columns),
            ResultEvent::ColumnsResolved { columns, .. } => self.start_encoder(&columns),
            ResultEvent::Rows(batch) => self.write_batch(&batch),
            ResultEvent::ResultSetFinished { .. } => self.finish_encoder(),
            _ => SinkFlow::Continue,
        }
    }

    fn flush_buf(&mut self) -> SinkFlow {
        if self.buf.is_empty() {
            return SinkFlow::Continue;
        }
        if let Err(err) = self.writer.write_all(&self.buf) {
            self.error = Some(ServiceError::Io(err));
            return SinkFlow::ConsumerGone;
        }
        self.buf.clear();
        SinkFlow::Continue
    }

    fn start_encoder(&mut self, columns: &[ResultColumn]) -> SinkFlow {
        if self.encoder.is_some() {
            return SinkFlow::Continue;
        }
        match build_encoder(columns, &self.options) {
            Ok(mut enc) => {
                self.buf.clear();
                enc.begin(&mut self.buf);
                self.encoder = Some(enc);
                self.flush_buf()
            }
            Err(err) => {
                self.error = Some(err);
                SinkFlow::ConsumerGone
            }
        }
    }

    fn write_batch(&mut self, batch: &[Vec<Value>]) -> SinkFlow {
        let Some(enc) = self.encoder.as_mut() else {
            return SinkFlow::Continue;
        };
        for row in batch {
            self.buf.clear();
            if let Err(err) = enc.row(&mut self.buf, &mut row.iter()) {
                self.error = Some(ServiceError::Export(err.to_string()));
                return SinkFlow::ConsumerGone;
            }
            if let Err(err) = self.writer.write_all(&self.buf) {
                self.error = Some(ServiceError::Io(err));
                return SinkFlow::ConsumerGone;
            }
            self.rows_written = self.rows_written.saturating_add(1);
        }
        self.buf.clear();
        SinkFlow::Continue
    }

    fn finish_encoder(&mut self) -> SinkFlow {
        let Some(mut enc) = self.encoder.take() else {
            return SinkFlow::Continue;
        };
        self.buf.clear();
        enc.finish(&mut self.buf);
        self.flush_buf()
    }
}

struct StreamingExportSink {
    state: Arc<Mutex<StreamingExportState>>,
}

#[async_trait]
impl ResultSink for StreamingExportSink {
    async fn accept(&mut self, event: ResultEvent) -> SinkFlow {
        let mut guard = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.handle_event(event)
    }
}

/// Streams a read statement from `session` into `target` via `.part` atomic write and rename.
///
/// # Errors
///
/// Returns [`ServiceError::Export`] or [`ServiceError::Io`] if streaming, encoding, or renaming fails.
pub async fn stream_export_atomic(
    session: &Session,
    read: &ReadStatement,
    target: &Path,
    options: ExportOptions,
    cancel: &CancellationToken,
) -> Result<(u64, StreamSummary), ServiceError> {
    if let Some(parent) = target.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    let part_path = part_path_for(target);
    let file = File::create(&part_path)?;
    let state = Arc::new(Mutex::new(StreamingExportState {
        writer: BufWriter::new(file),
        encoder: None,
        buf: Vec::with_capacity(4096),
        options,
        rows_written: 0,
        error: None,
    }));

    let read_owned = read.clone();
    let state_for_run = state.clone();
    let stream_res = session
        .run_cancellable(OperationKind::Export, cancel, |lease, call| {
            let read = read_owned.clone();
            let sink = Box::new(StreamingExportSink {
                state: state_for_run.clone(),
            });
            async move {
                lease
                    .stream_read(&read, sink, call)
                    .await
                    .map_err(OperationError::from)
            }
        })
        .await;

    let (rows_written, sink_err) = {
        let mut guard = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = guard.writer.flush();
        let _ = guard.writer.get_ref().sync_all();
        (guard.rows_written, guard.error.take())
    };

    if let Some(err) = sink_err {
        let _ = fs::remove_file(&part_path);
        return Err(err);
    }

    match stream_res {
        Ok(summary) => {
            fs::rename(&part_path, target)?;
            Ok((rows_written, summary))
        }
        Err(err) => {
            let _ = fs::remove_file(&part_path);
            Err(ServiceError::Export(err.to_string()))
        }
    }
}
