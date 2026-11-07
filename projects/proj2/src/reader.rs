// FileReader – deserializator: otwiera plik BIGOS, wczytuje stopkę i odczytuje grupy wierszy.
//
// Użycie:
//   let r = FileReader::open(path)?;          // czyta i sprawdza nagłówek, końcówkę i stopkę
//   for i in 0..r.num_row_groups() {
//       let batch = r.read_row_group(i, &[0, 2])?;   // tylko kolumny 0 i 2
//   }
//
// W pamięci trzymamy tylko schemat i metadane chunków; dane czytamy grupa po grupie,
// więc plik może być większy niż RAM. Chunki czytamy przez `read_exact_at` (pread),
// który nie zmienia pozycji w pliku – metody przyjmują `&self`, więc wiele wątków może
// czytać ten sam plik jednocześnie.

use std::fs::File;
use std::os::unix::fs::FileExt;
use std::path::Path;

use crate::encoding::{decode_column, read_vle};
use crate::error::{FormatError, Result};
use crate::format::{
    column_type_from_byte, ChunkMeta, RowGroupMeta, HEADER_LEN, MAGIC, TAIL_LEN, VERSION,
};
use crate::types::{Batch, ColumnDef, Schema};

pub struct FileReader {
    file: File,
    schema: Schema,
    row_groups: Vec<RowGroupMeta>,
}

impl FileReader {
    /// Otwiera plik i sprawdza jego strukturę (bez czytania danych kolumn).
    pub fn open(path: &Path) -> Result<FileReader> {
        let file = File::open(path)?;
        let file_len = file.metadata()?.len();
        if file_len < HEADER_LEN + TAIL_LEN {
            return Err(FormatError::Corrupted("plik za krótki".to_string()));
        }

        // Nagłówek: magia + wersja.
        let mut header = [0u8; HEADER_LEN as usize];
        file.read_exact_at(&mut header, 0)?;
        if &header[..5] != MAGIC {
            return Err(FormatError::Corrupted("brak magii BIGOS na początku pliku".to_string()));
        }
        if header[5] != VERSION {
            return Err(FormatError::Corrupted(format!("nieobsługiwana wersja {}", header[5])));
        }

        // Końcówka: długość stopki (u32 LE) + magia.
        let mut tail = [0u8; TAIL_LEN as usize];
        file.read_exact_at(&mut tail, file_len - TAIL_LEN)?;
        if &tail[4..] != MAGIC {
            return Err(FormatError::Corrupted(
                "brak magii BIGOS na końcu pliku (niedokończony zapis?)".to_string(),
            ));
        }
        let footer_len = u32::from_le_bytes([tail[0], tail[1], tail[2], tail[3]]) as u64;
        if footer_len > file_len - HEADER_LEN - TAIL_LEN {
            return Err(FormatError::Corrupted("długość stopki większa niż plik".to_string()));
        }

        // Stopka leży tuż przed końcówką; dane chunków – między nagłówkiem a stopką.
        let footer_start = file_len - TAIL_LEN - footer_len;
        let mut footer = vec![0u8; footer_len as usize];
        file.read_exact_at(&mut footer, footer_start)?;
        let (schema, row_groups) = parse_footer(&footer, footer_start)?;

        Ok(FileReader { file, schema, row_groups })
    }

    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    pub fn num_row_groups(&self) -> usize {
        self.row_groups.len()
    }

    /// Liczba wierszy w grupie `i`; `None`, gdy takiej grupy nie ma.
    pub fn row_group_rows(&self, i: usize) -> Option<u64> {
        self.row_groups.get(i).map(|rg| rg.row_count)
    }

    /// Odczytuje grupę wierszy `i`, ale tylko kolumny o numerach z `columns`
    /// (w podanej kolejności). Zwrócony batch ma kolumny w tej samej kolejności co `columns`.
    pub fn read_row_group(&self, i: usize, columns: &[usize]) -> Result<Batch> {
        let rg = match self.row_groups.get(i) {
            Some(rg) => rg,
            None => {
                return Err(FormatError::InvalidData(format!("brak grupy wierszy nr {}", i)));
            }
        };

        let mut result = Vec::with_capacity(columns.len());
        for &c in columns {
            let def = match self.schema.columns.get(c) {
                Some(def) => def,
                None => return Err(FormatError::InvalidData(format!("brak kolumny nr {}", c))),
            };
            let chunk = &rg.chunks[c];
            let mut bytes = vec![0u8; chunk.size as usize];
            self.file.read_exact_at(&mut bytes, chunk.offset)?;
            result.push(decode_column(def.ty, &bytes, rg.row_count as usize, chunk.raw_size)?);
        }

        Ok(Batch { num_rows: rg.row_count as usize, columns: result })
    }

    /// Odczytuje cały plik (wszystkie grupy, wszystkie kolumny).
    pub fn read_all(&self) -> Result<Vec<Batch>> {
        let mut all_columns = Vec::new();
        for c in 0..self.schema.columns.len() {
            all_columns.push(c);
        }
        let mut batches = Vec::with_capacity(self.row_groups.len());
        for i in 0..self.row_groups.len() {
            batches.push(self.read_row_group(i, &all_columns)?);
        }
        Ok(batches)
    }
}

/// Parsuje stopkę (format opisany w `writer::encode_footer`) i sprawdza, że każdy chunk
/// leży w obszarze danych: `HEADER_LEN ≤ offset` oraz `offset + size ≤ footer_start`.
fn parse_footer(buf: &[u8], footer_start: u64) -> Result<(Schema, Vec<RowGroupMeta>)> {
    let mut pos = 0;

    // Schemat. Nie rezerwujemy pamięci z góry według liczb z pliku – każda iteracja
    // zużywa bajty stopki, więc uszkodzona liczba kończy się błędem „ucięta stopka”.
    let num_columns = read_vle(buf, &mut pos)?;
    let mut columns = Vec::new();
    for _ in 0..num_columns {
        let ty = column_type_from_byte(read_u8(buf, &mut pos)?)?;
        let name_len = read_vle(buf, &mut pos)? as usize;
        let name_bytes = read_bytes(buf, &mut pos, name_len)?;
        let name = match String::from_utf8(name_bytes.to_vec()) {
            Ok(name) => name,
            Err(_) => return Err(FormatError::Corrupted("nazwa kolumny to nie UTF-8".to_string())),
        };
        columns.push(ColumnDef { name, ty });
    }
    let schema = match Schema::new(columns) {
        Ok(schema) => schema,
        Err(e) => return Err(FormatError::Corrupted(format!("niepoprawny schemat: {}", e))),
    };

    // Grupy wierszy.
    let num_row_groups = read_vle(buf, &mut pos)?;
    let mut row_groups = Vec::new();
    for _ in 0..num_row_groups {
        let row_count = read_vle(buf, &mut pos)?;
        let mut chunks = Vec::with_capacity(schema.columns.len());
        for _ in 0..schema.columns.len() {
            let offset = read_u64_le(buf, &mut pos)?;
            let size = read_u64_le(buf, &mut pos)?;
            let raw_size = read_vle(buf, &mut pos)?;
            let in_data_area = match offset.checked_add(size) {
                Some(end) => offset >= HEADER_LEN && end <= footer_start,
                None => false, // offset + size przekracza u64
            };
            if !in_data_area {
                return Err(FormatError::Corrupted(format!(
                    "chunk (offset {}, rozmiar {}) poza obszarem danych",
                    offset, size
                )));
            }
            chunks.push(ChunkMeta { offset, size, raw_size });
        }
        row_groups.push(RowGroupMeta { row_count, chunks });
    }

    if pos != buf.len() {
        return Err(FormatError::Corrupted("nadmiarowe bajty w stopce".to_string()));
    }
    Ok((schema, row_groups))
}

fn read_u8(buf: &[u8], pos: &mut usize) -> Result<u8> {
    let bytes = read_bytes(buf, pos, 1)?;
    Ok(bytes[0])
}

/// Liczba u64 zapisana na 8 bajtach little-endian (najmłodszy bajt pierwszy).
fn read_u64_le(buf: &[u8], pos: &mut usize) -> Result<u64> {
    let b = read_bytes(buf, pos, 8)?;
    Ok(u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}

/// Zwraca `len` bajtów od `*pos` i przesuwa `*pos`; błąd, gdy stopka się kończy.
fn read_bytes<'a>(buf: &'a [u8], pos: &mut usize, len: usize) -> Result<&'a [u8]> {
    match buf.get(*pos..pos.saturating_add(len)) {
        Some(bytes) => {
            *pos += len;
            Ok(bytes)
        }
        None => Err(FormatError::Corrupted("ucięta stopka".to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Column, ColumnType};

    /// Ręcznie rozpisany plik: kolumna `a INT64` z wartościami [100, 101, 99].
    const FILE: [u8; 42] = [
        0x42, 0x49, 0x47, 0x4F, 0x53, 0x01, //                 [0..6]   nagłówek "BIGOS" + wersja
        0xC8, 0x01, 0x02, 0x03, //                             [6..10]  chunk kolumny a
        0x01, 0x00, 0x01, 0x61, //                             [10..14] 1 kolumna: INT64, "a"
        0x01, 0x03, //                                         [14..16] 1 grupa, 3 wiersze
        0x06, 0, 0, 0, 0, 0, 0, 0, //                          [16..24] offset = 6
        0x04, 0, 0, 0, 0, 0, 0, 0, //                          [24..32] size = 4
        0x04, //                                               [32]     raw_size = 4
        0x17, 0x00, 0x00, 0x00, 0x42, 0x49, 0x47, 0x4F, 0x53, // [33..42] tail
    ];

    /// Zapisuje bajty do pliku tymczasowego, otwiera go i czyta całość.
    fn read_bytes_as_file(name: &str, bytes: &[u8]) -> Result<(Schema, Vec<Batch>)> {
        let path = std::env::temp_dir().join(format!("bigos_reader_{}_{}.bgs", name, std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        let result = match FileReader::open(&path) {
            Ok(r) => match r.read_all() {
                Ok(batches) => Ok((r.schema().clone(), batches)),
                Err(e) => Err(e),
            },
            Err(e) => Err(e),
        };
        std::fs::remove_file(&path).unwrap();
        result
    }

    #[test]
    fn reads_expected_structure() {
        let (schema, batches) = read_bytes_as_file("ok", &FILE).unwrap();
        assert_eq!(
            schema.columns,
            vec![ColumnDef { name: "a".to_string(), ty: ColumnType::Int64 }]
        );
        assert_eq!(batches, vec![Batch { num_rows: 3, columns: vec![Column::Int64(vec![100, 101, 99])] }]);
    }

    #[test]
    fn rejects_corrupted_files() {
        // (pozycja bajtu, nowa wartość, co psujemy)
        let changes = [
            (0, b'X', "magia na początku"),
            (5, 2, "wersja"),
            (41, b'X', "magia na końcu"),
            (33, 0xFF, "długość stopki większa niż plik"),
            (11, 7, "nieznany typ kolumny"),
            (16, 0xF0, "offset chunka poza obszarem danych"),
            (15, 4, "row_count niezgodny z liczbą wartości w chunku"),
        ];
        for (i, value, what) in changes {
            let mut bytes = FILE;
            bytes[i] = value;
            let r = read_bytes_as_file("bad", &bytes);
            assert!(matches!(r, Err(FormatError::Corrupted(_))), "nie wykryto: {}", what);
        }

        // ucięty plik
        let r = read_bytes_as_file("short", &FILE[..20]);
        assert!(matches!(r, Err(FormatError::Corrupted(_))));
    }
}
