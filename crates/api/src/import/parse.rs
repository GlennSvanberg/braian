//! Header normalization, CSV dialect sniffing, and streaming spreadsheet reads.
//!
//! Parsed data rows are appended to `rows.bin` as length-prefixed bincode
//! `Vec<Option<String>>` records. `rows.idx` stores a `u64` byte offset every
//! 1,000 rows. `rows.nos` stores the 1-based sheet row number of each record.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;

use calamine::{Data, DataRef, Reader, Sheet, SheetType, SheetVisible, Xlsx, open_workbook};
use encoding_rs::{UTF_8, UTF_16BE, UTF_16LE, WINDOWS_1252};
use encoding_rs_io::DecodeReaderBytesBuilder;
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

pub const MAX_COLUMNS: usize = 2_000;
const SAMPLE_ROWS: usize = 20;
const EMPTY_SCAN: usize = 1_000;
const CELL_CLIP: usize = 200;
const INDEX_EVERY: u64 = 1_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeaderCol {
    pub index: usize,
    pub raw: String,
    pub name: String,
    pub empty_ratio: f64,
    pub duplicate_of: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SheetInfo {
    pub index: usize,
    pub name: String,
    pub hidden: bool,
    pub empty: bool,
    pub rows_estimate: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CsvDialect {
    pub delimiter: String,
    pub quote: String,
    pub encoding: String,
    pub bom: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParseOutcome {
    pub headers: Vec<HeaderCol>,
    pub sample: Vec<Vec<String>>,
    pub sheets: Vec<SheetInfo>,
    pub sheet_index: usize,
    pub dialect: Option<CsvDialect>,
    pub data_rows: u64,
    pub header_row: u32,
    pub skipped_above: u32,
}

#[derive(Debug)]
pub struct ParseError {
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ParseError {}

fn fail(message: impl Into<String>) -> ParseError {
    ParseError {
        message: message.into(),
    }
}

/// NFKC, lowercase, fold Swedish and a few other letters, then snake_case.
pub fn normalize_header(raw: &str) -> String {
    let folded: String = raw.nfkc().flat_map(fold_letter).collect();
    let lower = folded.to_lowercase();
    let mut out = String::new();
    let mut pending = false;
    for character in lower.chars() {
        if character.is_ascii_alphanumeric() {
            if pending && !out.is_empty() {
                out.push('_');
            }
            pending = false;
            out.push(character);
        } else {
            pending = true;
        }
    }
    out
}

fn fold_letter(character: char) -> Vec<char> {
    match character {
        'å' | 'ä' | 'Å' | 'Ä' => vec!['a'],
        'ö' | 'ø' | 'Ö' | 'Ø' => vec!['o'],
        'æ' | 'Æ' => vec!['a', 'e'],
        'ü' | 'Ü' => vec!['u'],
        'é' | 'É' => vec!['e'],
        other => vec![other],
    }
}

pub fn column_letter(index: usize) -> String {
    let mut n = index + 1;
    let mut chars = Vec::new();
    while n > 0 {
        n -= 1;
        chars.push(b'A' + (n % 26) as u8);
        n /= 26;
    }
    chars.reverse();
    String::from_utf8(chars).unwrap_or_else(|_| "A".into())
}

pub fn inspect_headers(raws: &[String], empty_counts: &[usize], seen: usize) -> Vec<HeaderCol> {
    let mut names = Vec::with_capacity(raws.len());
    let mut first: Vec<(String, usize)> = Vec::new();
    for (index, raw) in raws.iter().enumerate() {
        let trimmed = raw.trim();
        let (raw_label, mut name) = if trimmed.is_empty() {
            let letter = column_letter(index);
            (
                format!("Column {letter}"),
                format!("column_{}", letter.to_lowercase()),
            )
        } else {
            (trimmed.to_owned(), normalize_header(trimmed))
        };
        if name.is_empty() {
            let letter = column_letter(index);
            name = format!("column_{}", letter.to_lowercase());
        }
        let duplicate_of = first
            .iter()
            .find(|(existing, _)| existing == &name)
            .map(|(_, first_index)| *first_index);
        if let Some(first_index) = duplicate_of {
            let nth = names
                .iter()
                .filter(|header: &&HeaderCol| {
                    header.name == name || header.name.starts_with(&format!("{name}__"))
                })
                .count()
                + 1;
            let _ = first_index;
            name = format!("{name}__{nth}");
        } else {
            first.push((name.clone(), index));
        }
        let empties = empty_counts.get(index).copied().unwrap_or(seen);
        let empty_ratio = if seen == 0 {
            1.0
        } else {
            empties as f64 / seen as f64
        };
        names.push(HeaderCol {
            index,
            raw: raw_label,
            name,
            empty_ratio,
            duplicate_of,
        });
    }
    names
}

pub struct RowSpool {
    bin: std::io::BufWriter<File>,
    idx: std::io::BufWriter<File>,
    nos: std::io::BufWriter<File>,
    offset: u64,
    count: u64,
}

impl RowSpool {
    pub fn create(dir: &Path) -> Result<Self, ParseError> {
        std::fs::create_dir_all(dir).map_err(|error| fail(error.to_string()))?;
        let bin = File::create(dir.join("rows.bin")).map_err(|error| fail(error.to_string()))?;
        let idx = File::create(dir.join("rows.idx")).map_err(|error| fail(error.to_string()))?;
        let nos = File::create(dir.join("rows.nos")).map_err(|error| fail(error.to_string()))?;
        Ok(Self {
            bin: std::io::BufWriter::new(bin),
            idx: std::io::BufWriter::new(idx),
            nos: std::io::BufWriter::new(nos),
            offset: 0,
            count: 0,
        })
    }

    pub fn push(&mut self, row_no: u32, cells: &[Option<String>]) -> Result<(), ParseError> {
        if self.count.is_multiple_of(INDEX_EVERY) {
            self.idx
                .write_all(&self.offset.to_le_bytes())
                .map_err(|error| fail(error.to_string()))?;
        }
        let bytes = bincode::serialize(cells).map_err(|error| fail(error.to_string()))?;
        let len = u32::try_from(bytes.len()).map_err(|_| fail("a row is too large"))?;
        self.bin
            .write_all(&len.to_le_bytes())
            .map_err(|error| fail(error.to_string()))?;
        self.bin
            .write_all(&bytes)
            .map_err(|error| fail(error.to_string()))?;
        self.nos
            .write_all(&row_no.to_le_bytes())
            .map_err(|error| fail(error.to_string()))?;
        self.offset += 4 + bytes.len() as u64;
        self.count += 1;
        Ok(())
    }

    pub fn finish(mut self) -> Result<u64, ParseError> {
        self.bin.flush().map_err(|error| fail(error.to_string()))?;
        self.idx.flush().map_err(|error| fail(error.to_string()))?;
        self.nos.flush().map_err(|error| fail(error.to_string()))?;
        Ok(self.count)
    }
}

pub type SpoolRow = (u32, Vec<Option<String>>);

pub struct RowCursor {
    file: File,
    nos: Vec<u32>,
    pos: usize,
}

impl RowCursor {
    #[allow(dead_code)]
    pub fn open(dir: &Path) -> Result<Self, ParseError> {
        let file = File::open(dir.join("rows.bin")).map_err(|error| fail(error.to_string()))?;
        let bytes = std::fs::read(dir.join("rows.nos")).map_err(|error| fail(error.to_string()))?;
        let mut nos = Vec::with_capacity(bytes.len() / 4);
        for chunk in bytes.chunks_exact(4) {
            nos.push(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }
        Ok(Self { file, nos, pos: 0 })
    }

    pub fn next_row(&mut self) -> Result<Option<SpoolRow>, ParseError> {
        if self.pos >= self.nos.len() {
            return Ok(None);
        }
        let row = read_record(&mut self.file)?;
        let row_no = self.nos[self.pos];
        self.pos += 1;
        Ok(Some((row_no, row)))
    }

    pub fn len(&self) -> usize {
        self.nos.len()
    }

    #[allow(dead_code)]
    pub fn row_no(&self, index: usize) -> Option<u32> {
        self.nos.get(index).copied()
    }

    #[allow(dead_code)]
    pub fn read_at(&mut self, index: usize) -> Result<SpoolRow, ParseError> {
        let offset = offset_of(&self.file, index)?;
        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(|error| fail(error.to_string()))?;
        let row = read_record(&mut self.file)?;
        let row_no = self.nos.get(index).copied().unwrap_or(0);
        Ok((row_no, row))
    }

    #[allow(dead_code)]
    pub fn for_each<F>(&mut self, mut visit: F) -> Result<(), ParseError>
    where
        F: FnMut(usize, u32, Vec<Option<String>>) -> Result<(), ParseError>,
    {
        self.file
            .seek(SeekFrom::Start(0))
            .map_err(|error| fail(error.to_string()))?;
        for index in 0..self.nos.len() {
            let row = read_record(&mut self.file)?;
            visit(index, self.nos[index], row)?;
        }
        Ok(())
    }
}

#[allow(dead_code)]
fn offset_of(file: &File, index: usize) -> Result<u64, ParseError> {
    if index == 0 {
        return Ok(0);
    }
    let mut reader = BufReader::new(file.try_clone().map_err(|error| fail(error.to_string()))?);
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|error| fail(error.to_string()))?;
    for _ in 0..index {
        let mut len_buf = [0u8; 4];
        reader
            .read_exact(&mut len_buf)
            .map_err(|error| fail(error.to_string()))?;
        let len = u32::from_le_bytes(len_buf) as u64;
        reader
            .seek(SeekFrom::Current(len as i64))
            .map_err(|error| fail(error.to_string()))?;
    }
    reader
        .stream_position()
        .map_err(|error| fail(error.to_string()))
}

fn read_record(file: &mut File) -> Result<Vec<Option<String>>, ParseError> {
    let mut len_buf = [0u8; 4];
    file.read_exact(&mut len_buf)
        .map_err(|error| fail(error.to_string()))?;
    let len = u32::from_le_bytes(len_buf) as usize;
    let mut bytes = vec![0u8; len];
    file.read_exact(&mut bytes)
        .map_err(|error| fail(error.to_string()))?;
    bincode::deserialize(&bytes).map_err(|error| fail(error.to_string()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Csv,
    Xlsx,
    Xls,
}

impl FileKind {
    pub fn parse(name: &str) -> Option<Self> {
        let lower = name.to_lowercase();
        if lower.ends_with(".xlsx") {
            Some(Self::Xlsx)
        } else if lower.ends_with(".xls") {
            Some(Self::Xls)
        } else if lower.ends_with(".csv") || lower.ends_with(".tsv") || lower.ends_with(".txt") {
            Some(Self::Csv)
        } else {
            None
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Xlsx => "xlsx",
            Self::Xls => "xls",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Xlsx => "xlsx",
            Self::Xls => "xls",
        }
    }
}

pub fn parse_upload(
    path: &Path,
    kind: FileKind,
    sheet_index: Option<usize>,
    spool_dir: &Path,
) -> Result<ParseOutcome, ParseError> {
    match kind {
        FileKind::Csv => parse_csv(path, spool_dir),
        FileKind::Xlsx => parse_xlsx(path, sheet_index, spool_dir),
        FileKind::Xls => parse_xls(path, sheet_index, spool_dir),
    }
}

fn parse_csv(path: &Path, spool_dir: &Path) -> Result<ParseOutcome, ParseError> {
    let (encoding, bom, name) = sniff_encoding(path)?;
    let decoded = decode_file(path, encoding)?;
    let delimiter = sniff_delimiter(&decoded);
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .from_reader(decoded.as_bytes());
    let mut records = reader.records();
    let header;
    let mut header_row = 0u32;
    let mut skipped = 0u32;
    loop {
        let Some(record) = records.next() else {
            return Err(fail("This file has no rows."));
        };
        let record =
            record.map_err(|error| fail(format!("We could not read this file. {error}")))?;
        header_row += 1;
        let cells: Vec<String> = record.iter().map(|cell| cell.trim().to_owned()).collect();
        if cells.iter().all(|cell| cell.is_empty()) {
            skipped += 1;
            continue;
        }
        header = cells;
        break;
    }
    if header.len() > MAX_COLUMNS {
        return Err(fail(format!("Too many columns (max {MAX_COLUMNS})")));
    }
    let width = header.len();
    let mut spool = RowSpool::create(spool_dir)?;
    let mut empty = vec![0usize; width];
    let mut seen = 0usize;
    let mut sample = Vec::new();
    let mut data_rows = 0u64;
    for record in records {
        let record =
            record.map_err(|error| fail(format!("We could not read this file. {error}")))?;
        header_row += 1;
        let mut cells = Vec::with_capacity(width);
        for index in 0..width {
            let text = record.get(index).unwrap_or("").trim();
            if text.is_empty() {
                cells.push(None);
            } else {
                cells.push(Some(text.to_owned()));
            }
        }
        if seen < EMPTY_SCAN {
            for (index, cell) in cells.iter().enumerate() {
                if cell.is_none() {
                    empty[index] += 1;
                }
            }
            seen += 1;
        }
        if sample.len() < SAMPLE_ROWS {
            sample.push(
                cells
                    .iter()
                    .map(|cell| clip_cell(cell.as_deref().unwrap_or("")))
                    .collect(),
            );
        }
        spool.push(header_row, &cells)?;
        data_rows += 1;
    }
    if data_rows == 0 {
        return Err(fail("This file has no rows."));
    }
    spool.finish()?;
    let headers = inspect_headers(&header, &empty, seen);
    let delimiter_label = match delimiter {
        b'\t' => "\\t".to_owned(),
        byte => (byte as char).to_string(),
    };
    Ok(ParseOutcome {
        headers,
        sample,
        sheets: vec![SheetInfo {
            index: 0,
            name: "CSV".into(),
            hidden: false,
            empty: false,
            rows_estimate: data_rows,
        }],
        sheet_index: 0,
        dialect: Some(CsvDialect {
            delimiter: delimiter_label,
            quote: "\"".into(),
            encoding: name,
            bom,
        }),
        data_rows,
        header_row: header_row - data_rows as u32,
        skipped_above: skipped,
    })
}

fn sniff_encoding(
    path: &Path,
) -> Result<(&'static encoding_rs::Encoding, bool, String), ParseError> {
    let mut file = File::open(path).map_err(|_| fail("We could not read this file."))?;
    let mut buf = vec![0u8; 65_536];
    let n = file
        .read(&mut buf)
        .map_err(|_| fail("We could not read this file."))?;
    let bytes = &buf[..n];
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return Ok((UTF_16LE, true, "utf-16le".into()));
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return Ok((UTF_16BE, true, "utf-16be".into()));
    }
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Ok((UTF_8, true, "utf-8".into()));
    }
    if utf8_prefix(bytes) {
        Ok((UTF_8, false, "utf-8".into()))
    } else {
        Ok((WINDOWS_1252, false, "windows-1252".into()))
    }
}

fn utf8_prefix(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return true;
    }
    match std::str::from_utf8(bytes) {
        Ok(_) => true,
        Err(error) => error.error_len().is_none() && bytes.len() - error.valid_up_to() <= 3,
    }
}

fn decode_file(
    path: &Path,
    encoding: &'static encoding_rs::Encoding,
) -> Result<String, ParseError> {
    let file = File::open(path).map_err(|_| fail("We could not read this file."))?;
    let mut reader = DecodeReaderBytesBuilder::new()
        .encoding(Some(encoding))
        .build(file);
    let mut text = String::new();
    reader
        .read_to_string(&mut text)
        .map_err(|_| fail("We could not read this file."))?;
    Ok(text)
}

pub fn sniff_delimiter(text: &str) -> u8 {
    let lines: Vec<&str> = text
        .lines()
        .take(20)
        .filter(|line| !line.is_empty())
        .collect();
    let candidates = [b';', b',', b'\t', b'|'];
    let mut best: Option<(i32, u8)> = None;
    for delimiter in candidates {
        let counts: Vec<usize> = lines
            .iter()
            .map(|line| split_unquoted(line, delimiter).len())
            .filter(|count| *count > 1)
            .collect();
        if counts.is_empty() {
            continue;
        }
        let mut tally = std::collections::BTreeMap::new();
        for count in &counts {
            *tally.entry(*count).or_insert(0) += 1;
        }
        let mode = tally.values().copied().max().unwrap_or(0);
        let score = mode * 100 + counts.len() as i32;
        let rank = match delimiter {
            b';' => 4,
            b',' => 3,
            b'\t' => 2,
            _ => 1,
        };
        let ranked = score * 10 + rank;
        if best.map(|(current, _)| ranked > current).unwrap_or(true) {
            best = Some((ranked, delimiter));
        }
    }
    best.map(|(_, delimiter)| delimiter).unwrap_or(b',')
}

fn split_unquoted(line: &str, delimiter: u8) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    let bytes = line.as_bytes();
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'"' {
            quoted = !quoted;
        } else if *byte == delimiter && !quoted {
            parts.push(&line[start..index]);
            start = index + 1;
        }
    }
    parts.push(&line[start..]);
    parts
}

fn parse_xlsx(
    path: &Path,
    sheet_index: Option<usize>,
    spool_dir: &Path,
) -> Result<ParseOutcome, ParseError> {
    let mut book: Xlsx<_> = open_workbook(path)
        .map_err(|_| fail("We could not read this file. Is it a real Excel file?"))?;
    let meta: Vec<Sheet> = book.sheets_metadata().to_vec();
    parse_excel_sheets(&mut book, &meta, sheet_index, spool_dir, true)
}

fn parse_xls(
    path: &Path,
    sheet_index: Option<usize>,
    spool_dir: &Path,
) -> Result<ParseOutcome, ParseError> {
    let len = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    if len > 200 * 1024 * 1024 {
        return Err(fail("This .xls file is larger than 200 MB."));
    }
    let mut book = calamine::open_workbook_auto(path)
        .map_err(|_| fail("We could not read this file. Is it a real Excel file?"))?;
    let meta = book.sheets_metadata().to_vec();
    let names: Vec<(usize, String, bool)> = meta
        .iter()
        .enumerate()
        .map(|(index, sheet)| {
            (
                index,
                sheet.name.clone(),
                sheet.visible != SheetVisible::Visible || sheet.typ != SheetType::WorkSheet,
            )
        })
        .collect();
    let chosen = choose_sheet_by_range(&mut book, &names, sheet_index)?;
    let mut sheets = Vec::new();
    for (index, name, hidden) in &names {
        let empty = book
            .worksheet_range(name)
            .ok()
            .map(|range| {
                range
                    .rows()
                    .all(|row| row.iter().all(|cell| cell_text(cell).is_none()))
            })
            .unwrap_or(true);
        sheets.push(SheetInfo {
            index: *index,
            name: name.clone(),
            hidden: *hidden,
            empty,
            rows_estimate: 0,
        });
    }
    let range = book
        .worksheet_range(&names[chosen].1)
        .map_err(|_| fail("We could not read this file. Is it a real Excel file?"))?;
    let rows: Vec<Vec<Option<String>>> = range
        .rows()
        .map(|row| row.iter().map(cell_text).collect())
        .collect();
    let outcome = spool_matrix(&rows, chosen, spool_dir)?;
    let mut outcome = outcome;
    for sheet in &mut sheets {
        if sheet.index == chosen {
            sheet.empty = false;
            sheet.rows_estimate = outcome.data_rows;
        }
    }
    outcome.sheets = sheets;
    Ok(outcome)
}

fn choose_sheet_by_range<R: Reader<std::io::BufReader<File>>>(
    book: &mut R,
    names: &[(usize, String, bool)],
    sheet_index: Option<usize>,
) -> Result<usize, ParseError> {
    if let Some(index) = sheet_index {
        return names
            .iter()
            .position(|(candidate, _, _)| *candidate == index)
            .ok_or_else(|| fail("That sheet does not exist."));
    }
    for (index, name, hidden) in names {
        if *hidden {
            continue;
        }
        let Ok(range) = book.worksheet_range(name) else {
            continue;
        };
        let mut nonempty = 0;
        for row in range.rows() {
            if row.iter().any(|cell| cell_text(cell).is_some()) {
                nonempty += 1;
                if nonempty >= 2 {
                    return Ok(*index);
                }
            }
        }
    }
    Err(fail("This file has no rows."))
}

fn parse_excel_sheets(
    book: &mut Xlsx<std::io::BufReader<File>>,
    meta: &[Sheet],
    sheet_index: Option<usize>,
    spool_dir: &Path,
    streaming: bool,
) -> Result<ParseOutcome, ParseError> {
    let _ = streaming;
    let names: Vec<(usize, String, bool)> = meta
        .iter()
        .enumerate()
        .map(|(index, sheet)| {
            (
                index,
                sheet.name.clone(),
                sheet.visible != SheetVisible::Visible || sheet.typ != SheetType::WorkSheet,
            )
        })
        .collect();
    let chosen = if let Some(index) = sheet_index {
        names
            .iter()
            .position(|(candidate, _, _)| *candidate == index)
            .ok_or_else(|| fail("That sheet does not exist."))?
    } else {
        choose_xlsx_sheet(book, &names)?
    };
    let mut sheets = Vec::new();
    for (index, name, hidden) in &names {
        sheets.push(SheetInfo {
            index: *index,
            name: name.clone(),
            hidden: *hidden,
            empty: *index != names[chosen].0,
            rows_estimate: 0,
        });
    }
    let name = names[chosen].1.clone();
    let mut reader = book
        .worksheet_cells_reader(&name)
        .map_err(|_| fail("We could not read this file. Is it a real Excel file?"))?;
    let mut rows: Vec<(u32, Vec<Option<String>>)> = Vec::new();
    let mut current_row: Option<u32> = None;
    let mut current: Vec<Option<String>> = Vec::new();
    loop {
        let cell = reader
            .next_cell()
            .map_err(|_| fail("We could not read this file. Is it a real Excel file?"))?;
        let Some(cell) = cell else { break };
        let (row, col) = cell.get_position();
        if current_row != Some(row) {
            if let Some(finished) = current_row.take() {
                rows.push((finished, std::mem::take(&mut current)));
            }
            current_row = Some(row);
        }
        let col = col as usize;
        if col > MAX_COLUMNS {
            return Err(fail(format!("Too many columns (max {MAX_COLUMNS})")));
        }
        if current.len() <= col {
            current.resize(col + 1, None);
        }
        current[col] = dataref_text(cell.get_value());
    }
    if let Some(finished) = current_row {
        rows.push((finished, current));
    }
    let matrix = expand_rows(&rows);
    let mut outcome = spool_matrix(&matrix, names[chosen].0, spool_dir)?;
    for sheet in &mut sheets {
        if sheet.index == names[chosen].0 {
            sheet.empty = outcome.data_rows == 0;
            sheet.rows_estimate = outcome.data_rows;
        }
    }
    if outcome.data_rows == 0 {
        return Err(fail("This file has no rows."));
    }
    outcome.sheets = sheets;
    Ok(outcome)
}

fn choose_xlsx_sheet(
    book: &mut Xlsx<std::io::BufReader<File>>,
    names: &[(usize, String, bool)],
) -> Result<usize, ParseError> {
    for (index, name, hidden) in names {
        if *hidden {
            continue;
        }
        let mut reader = match book.worksheet_cells_reader(name) {
            Ok(reader) => reader,
            Err(_) => continue,
        };
        let mut seen_rows = std::collections::BTreeSet::new();
        while let Ok(Some(cell)) = reader.next_cell() {
            if dataref_text(cell.get_value()).is_some() {
                seen_rows.insert(cell.get_position().0);
                if seen_rows.len() >= 2 {
                    return Ok(*index);
                }
            }
        }
    }
    Err(fail("This file has no rows."))
}

fn expand_rows(rows: &[(u32, Vec<Option<String>>)]) -> Vec<Vec<Option<String>>> {
    if rows.is_empty() {
        return Vec::new();
    }
    let start = rows[0].0;
    let end = rows.last().map(|row| row.0).unwrap_or(start);
    let mut matrix = vec![Vec::new(); (end - start + 1) as usize];
    for (row, cells) in rows {
        let index = (*row - start) as usize;
        if index < matrix.len() {
            matrix[index] = cells.clone();
        }
    }
    matrix
}

fn spool_matrix(
    rows: &[Vec<Option<String>>],
    sheet_index: usize,
    spool_dir: &Path,
) -> Result<ParseOutcome, ParseError> {
    let mut header_index = None;
    for (index, row) in rows.iter().enumerate() {
        if row
            .iter()
            .any(|cell| cell.as_ref().is_some_and(|text| !text.trim().is_empty()))
        {
            header_index = Some(index);
            break;
        }
    }
    let Some(header_index) = header_index else {
        return Err(fail("This file has no rows."));
    };
    let header: Vec<String> = rows[header_index]
        .iter()
        .map(|cell| cell.clone().unwrap_or_default())
        .collect();
    if header.len() > MAX_COLUMNS {
        return Err(fail(format!("Too many columns (max {MAX_COLUMNS})")));
    }
    let width = header.len().max(1);
    let mut spool = RowSpool::create(spool_dir)?;
    let mut empty = vec![0usize; width];
    let mut seen = 0usize;
    let mut sample = Vec::new();
    let mut data_rows = 0u64;
    for (offset, row) in rows.iter().enumerate().skip(header_index + 1) {
        let mut cells = Vec::with_capacity(width);
        for index in 0..width {
            let text = row
                .get(index)
                .and_then(|cell| cell.as_deref())
                .unwrap_or("")
                .trim();
            if text.is_empty() {
                cells.push(None);
            } else {
                cells.push(Some(text.to_owned()));
            }
        }
        if seen < EMPTY_SCAN {
            for (index, cell) in cells.iter().enumerate() {
                if cell.is_none() {
                    empty[index] += 1;
                }
            }
            seen += 1;
        }
        if sample.len() < SAMPLE_ROWS {
            sample.push(
                cells
                    .iter()
                    .map(|cell| clip_cell(cell.as_deref().unwrap_or("")))
                    .collect(),
            );
        }
        let row_no = (offset as u32) + 1;
        spool.push(row_no, &cells)?;
        data_rows += 1;
    }
    if data_rows == 0 {
        return Err(fail("This file has no rows."));
    }
    spool.finish()?;
    Ok(ParseOutcome {
        headers: inspect_headers(&header, &empty, seen),
        sample,
        sheets: vec![SheetInfo {
            index: sheet_index,
            name: format!("Sheet{}", sheet_index + 1),
            hidden: false,
            empty: false,
            rows_estimate: data_rows,
        }],
        sheet_index,
        dialect: None,
        data_rows,
        header_row: header_index as u32 + 1,
        skipped_above: header_index as u32,
    })
}

fn clip_cell(text: &str) -> String {
    text.chars().take(CELL_CLIP).collect()
}

fn cell_text(data: &Data) -> Option<String> {
    match data {
        Data::Empty | Data::Error(_) => None,
        Data::String(text) => nonempty(text),
        Data::Int(value) => Some(value.to_string()),
        Data::Float(value) => Some(format_number(*value)),
        Data::Bool(value) => Some(if *value { "true" } else { "false" }.into()),
        Data::DateTime(value) => value.as_datetime().map(format_excel_date),
        Data::DateTimeIso(text) => Some(text.chars().take(10).collect()),
        Data::DurationIso(text) => Some(text.clone()),
    }
}

fn dataref_text(data: &DataRef<'_>) -> Option<String> {
    match data {
        DataRef::Empty | DataRef::Error(_) => None,
        DataRef::String(text) => nonempty(text),
        DataRef::SharedString(text) => nonempty(text),
        DataRef::Int(value) => Some(value.to_string()),
        DataRef::Float(value) => Some(format_number(*value)),
        DataRef::Bool(value) => Some(if *value { "true" } else { "false" }.into()),
        DataRef::DateTime(value) => value.as_datetime().map(format_excel_date),
        DataRef::DateTimeIso(text) => Some(text.chars().take(10).collect()),
        DataRef::DurationIso(text) => Some(text.clone()),
    }
}

fn nonempty(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

pub fn format_number(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        value.to_string()
    }
}

fn format_excel_date(value: chrono::NaiveDateTime) -> String {
    if value.time() == chrono::NaiveTime::MIN {
        value.format("%Y-%m-%d").to_string()
    } else {
        value.format("%Y-%m-%dT%H:%M:%S").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_swedish_headers_and_duplicates() {
        assert_eq!(normalize_header("Åtgärd"), "atgard");
        assert_eq!(normalize_header("  Vikt (kg) "), "vikt_kg");
        // ß is not in the fold table, so it becomes a separator.
        assert_eq!(normalize_header("Größe"), "gro_e");
        let headers = inspect_headers(
            &["Price".into(), "".into(), "Price".into(), "Åäö".into()],
            &[0, 2, 0, 1],
            2,
        );
        assert_eq!(headers[0].name, "price");
        assert_eq!(headers[1].raw, "Column B");
        assert_eq!(headers[1].name, "column_b");
        assert_eq!(headers[2].duplicate_of, Some(0));
        assert_eq!(headers[2].name, "price__2");
        assert_eq!(headers[3].name, "aao");
        assert!((headers[1].empty_ratio - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn column_letters_pass_z() {
        assert_eq!(column_letter(0), "A");
        assert_eq!(column_letter(25), "Z");
        assert_eq!(column_letter(26), "AA");
    }

    #[test]
    fn delimiter_ties_prefer_semicolon_then_comma() {
        assert_eq!(sniff_delimiter("a;b\nc;d\n"), b';');
        assert_eq!(sniff_delimiter("a,b\nc,d\n"), b',');
        assert_eq!(sniff_delimiter("a\tb\nc\td\n"), b'\t');
        assert_eq!(sniff_delimiter("a|b\nc|d\n"), b'|');
    }

    #[test]
    fn utf16_and_cp1252_round_trip_headers() {
        let dir = std::env::temp_dir().join(format!("braian-csv-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let utf16 = dir.join("utf16.csv");
        let mut bytes = vec![0xFF, 0xFE];
        for unit in "sku;name\nA-1;Stol\n".encode_utf16() {
            bytes.extend(unit.to_le_bytes());
        }
        std::fs::write(&utf16, bytes).unwrap();
        let outcome = parse_csv(&utf16, &dir.join("spool-utf16")).unwrap();
        assert_eq!(outcome.dialect.unwrap().encoding, "utf-16le");
        assert_eq!(outcome.headers[0].name, "sku");
        assert_eq!(outcome.sample[0][1], "Stol");

        let cp = dir.join("cp.csv");
        let mut raw = b"sku;namn\n".to_vec();
        raw.extend(b"A-1;");
        raw.push(0xE5); // å in windows-1252
        raw.extend(b"\n");
        std::fs::write(&cp, raw).unwrap();
        let outcome = parse_csv(&cp, &dir.join("spool-cp")).unwrap();
        assert_eq!(outcome.dialect.unwrap().encoding, "windows-1252");
        assert_eq!(outcome.sample[0][1], "å");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn streams_a_wide_csv_without_keeping_every_row() {
        let dir = std::env::temp_dir().join(format!("braian-csv-big-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("big.csv");
        let mut file = File::create(&path).unwrap();
        writeln!(file, "sku,name").unwrap();
        let rows = 5_000u32;
        for index in 0..rows {
            writeln!(file, "K-{index},Name {index}").unwrap();
        }
        drop(file);
        let spool = dir.join("spool");
        let outcome = parse_csv(&path, &spool).unwrap();
        assert_eq!(outcome.data_rows, u64::from(rows));
        assert_eq!(outcome.sample.len(), 20);
        let mut cursor = RowCursor::open(&spool).unwrap();
        assert_eq!(cursor.len(), rows as usize);
        let (row_no, cells) = cursor.read_at(rows as usize - 1).unwrap();
        assert_eq!(row_no, rows + 1);
        assert_eq!(
            cells[0].as_deref(),
            Some(format!("K-{}", rows - 1)).as_deref()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn xlsx_picks_the_first_nonempty_visible_sheet() {
        let dir = std::env::temp_dir().join(format!("braian-xlsx-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("book.xlsx");
        let mut workbook = rust_xlsxwriter::Workbook::new();
        let hidden = workbook.add_worksheet();
        hidden.set_name("Hidden").unwrap();
        hidden.write_string(0, 0, "sku").unwrap();
        hidden.write_string(1, 0, "NO").unwrap();
        hidden.set_hidden(true);
        let empty = workbook.add_worksheet();
        empty.set_name("Empty").unwrap();
        let data = workbook.add_worksheet();
        data.set_name("Prices").unwrap();
        data.write_string(0, 0, "Artnr").unwrap();
        data.write_string(0, 1, "Vikt").unwrap();
        data.write_number(1, 0, 100).unwrap();
        data.write_number(1, 1, 1.5).unwrap();
        data.write_number(2, 0, 101).unwrap();
        data.write_number(2, 1, 2.0).unwrap();
        data.set_active(true);
        workbook.save(&path).unwrap();
        let outcome = parse_xlsx(&path, None, &dir.join("spool")).unwrap();
        assert_eq!(outcome.sheets.len(), 3);
        assert!(outcome.sheets[0].hidden);
        assert_eq!(outcome.sheets[outcome.sheet_index].name, "Prices");
        assert_eq!(outcome.headers[0].name, "artnr");
        assert_eq!(outcome.data_rows, 2);
        assert_eq!(outcome.sample[0][1], "1.5");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
