// FileWriter – serializator: zapisuje schemat i kolejne batche do pliku BIGOS.
//
// Użycie:
//   let mut w = FileWriter::create(path, schema)?;
//   w.write_batch(&batch1)?;      // każdy batch to jedna grupa wierszy
//   w.write_batch(&batch2)?;
//   w.finish()?;                  // dopisuje stopkę; bez tego plik jest niekompletny
//
// Batch po zapisaniu nie jest już potrzebny – w pamięci trzymamy tylko metadane
// (kilkadziesiąt bajtów na grupę wierszy), więc plik może być większy niż RAM.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use crate::encoding::{encode_column, write_vle};
use crate::error::{FormatError, Result};
use crate::format::{column_type_to_byte, ChunkMeta, RowGroupMeta, HEADER_LEN, MAGIC, VERSION};
use crate::types::{check_columns, Batch, Schema, BATCH_SIZE};

pub struct FileWriter {
    out: BufWriter<File>,
    schema: Schema,
    /// Liczba bajtów zapisanych do tej pory = offset następnego chunka.
    offset: u64,
    /// Metadane zapisanych grup wierszy – trafią do stopki.
    row_groups: Vec<RowGroupMeta>,
}

impl FileWriter {
    /// Tworzy (albo nadpisuje) plik i zapisuje nagłówek.
    pub fn create(path: &Path, schema: Schema) -> Result<FileWriter> {
        let mut out = BufWriter::new(File::create(path)?);
        out.write_all(MAGIC)?;
        out.write_all(&[VERSION])?;
        Ok(FileWriter { out, schema, offset: HEADER_LEN, row_groups: Vec::new() })
    }

    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// Zapisuje batch (maks. `BATCH_SIZE` wierszy) jako jedną grupę wierszy. Pusty batch jest pomijany.
    pub fn write_batch(&mut self, batch: &Batch) -> Result<()> {
        let num_rows = check_columns(&self.schema, &batch.columns)?;
        if num_rows != batch.num_rows {
            return Err(FormatError::InvalidData(format!(
                "batch deklaruje {} wierszy, a kolumny mają {}",
                batch.num_rows, num_rows
            )));
        }
        if num_rows > BATCH_SIZE {
            return Err(FormatError::InvalidData(format!(
                "batch ma {} wierszy, a maksimum to {}",
                num_rows, BATCH_SIZE
            )));
        }
        if num_rows == 0 {
            return Ok(());
        }

        // Najpierw kodujemy wszystkie kolumny – jeśli któraś zawiedzie,
        // do pliku nie trafi połowa grupy wierszy.
        let mut encoded = Vec::with_capacity(batch.columns.len());
        for col in &batch.columns {
            encoded.push(encode_column(col)?);
        }

        let mut chunks = Vec::with_capacity(encoded.len());
        for chunk in &encoded {
            self.out.write_all(&chunk.bytes)?;
            let size = chunk.bytes.len() as u64;
            chunks.push(ChunkMeta { offset: self.offset, size, raw_size: chunk.raw_size });
            self.offset += size;
        }
        self.row_groups.push(RowGroupMeta { row_count: num_rows as u64, chunks });
        Ok(())
    }

    /// Dopisuje stopkę i końcówkę pliku. Przyjmuje `self` na własność,
    /// więc po `finish()` nie da się już nic zapisać tym writerem.
    pub fn finish(mut self) -> Result<()> {
        let footer = encode_footer(&self.schema, &self.row_groups);
        if footer.len() > u32::MAX as usize {
            return Err(FormatError::InvalidData("stopka większa niż 4 GiB".to_string()));
        }
        self.out.write_all(&footer)?;
        self.out.write_all(&(footer.len() as u32).to_le_bytes())?;
        self.out.write_all(MAGIC)?;
        self.out.flush()?;
        Ok(())
    }
}

/// Stopka:
///   num_columns : VLE
///   dla każdej kolumny:   typ: u8 (0 = INT64, 1 = VARCHAR) | name_len: VLE | name: bajty UTF-8
///   num_row_groups : VLE
///   dla każdej grupy:     row_count: VLE
///     dla każdej kolumny: offset: u64 LE | size: u64 LE | raw_size: VLE
fn encode_footer(schema: &Schema, row_groups: &[RowGroupMeta]) -> Vec<u8> {
    let mut out = Vec::new();

    write_vle(&mut out, schema.columns.len() as u64);
    for col in &schema.columns {
        out.push(column_type_to_byte(col.ty));
        write_vle(&mut out, col.name.len() as u64);
        out.extend_from_slice(col.name.as_bytes());
    }

    write_vle(&mut out, row_groups.len() as u64);
    for rg in row_groups {
        write_vle(&mut out, rg.row_count);
        for chunk in &rg.chunks {
            out.extend_from_slice(&chunk.offset.to_le_bytes());
            out.extend_from_slice(&chunk.size.to_le_bytes());
            write_vle(&mut out, chunk.raw_size);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Column, ColumnDef, ColumnType};

    fn int_schema() -> Schema {
        Schema::new(vec![ColumnDef { name: "a".to_string(), ty: ColumnType::Int64 }]).unwrap()
    }

    #[test]
    fn writes_expected_bytes() {
        let path = std::env::temp_dir().join(format!("bigos_writer_{}.bgs", std::process::id()));
        let schema = int_schema();
        let batch = Batch::new(&schema, vec![Column::Int64(vec![100, 101, 99])]).unwrap();

        let mut w = FileWriter::create(&path, schema).unwrap();
        w.write_batch(&batch).unwrap();
        w.finish().unwrap();
        let bytes = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).unwrap();

        let mut expected = Vec::new();
        // nagłówek: "BIGOS" + wersja 1
        expected.extend_from_slice(&[0x42, 0x49, 0x47, 0x4F, 0x53, 0x01]);
        // chunk kolumny "a" (offset 6): [100, 101, 99] → C8 01 02 03
        expected.extend_from_slice(&[0xC8, 0x01, 0x02, 0x03]);
        // stopka (23 B): 1 kolumna, typ INT64, nazwa "a"
        expected.extend_from_slice(&[0x01, 0x00, 0x01, 0x61]);
        // 1 grupa wierszy, 3 wiersze
        expected.extend_from_slice(&[0x01, 0x03]);
        // chunk: offset = 6, size = 4 (u64 LE), raw_size = 4 (VLE)
        expected.extend_from_slice(&[0x06, 0, 0, 0, 0, 0, 0, 0]);
        expected.extend_from_slice(&[0x04, 0, 0, 0, 0, 0, 0, 0]);
        expected.push(0x04);
        // tail: długość stopki = 23 (u32 LE) + "BIGOS"
        expected.extend_from_slice(&[0x17, 0x00, 0x00, 0x00, 0x42, 0x49, 0x47, 0x4F, 0x53]);

        assert_eq!(bytes, expected);
    }

    #[test]
    fn rejects_batch_not_matching_schema() {
        let path = std::env::temp_dir().join(format!("bigos_writer_bad_{}.bgs", std::process::id()));
        let mut w = FileWriter::create(&path, int_schema()).unwrap();

        let wrong_type = Batch { num_rows: 1, columns: vec![Column::varchar_from_strs(&["x"])] };
        let wrong_rows = Batch { num_rows: 5, columns: vec![Column::Int64(vec![1])] };
        assert!(matches!(w.write_batch(&wrong_type), Err(FormatError::InvalidData(_))));
        assert!(matches!(w.write_batch(&wrong_rows), Err(FormatError::InvalidData(_))));

        std::fs::remove_file(&path).unwrap();
    }
}
