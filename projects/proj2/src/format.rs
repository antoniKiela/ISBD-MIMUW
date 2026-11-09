// Stałe i metadane wspólne dla zapisu i odczytu pliku BIGOS.
//
// Układ pliku:
//   HEADER : "BIGOS" (5 B) | wersja: u8                      (6 B)
//   dane   : grupa wierszy 0: chunk kol. 0 | chunk kol. 1 | …
//            grupa wierszy 1: …
//   FOOTER : schemat + położenie każdego chunka (opis w `writer::encode_footer`)
//   TAIL   : długość stopki: u32 LE | "BIGOS"                 (9 B)

use crate::error::{FormatError, Result};
use crate::types::ColumnType;

/// Magia na początku i na końcu pliku.
pub const MAGIC: &[u8; 5] = b"BIGOS";
/// Wersja formatu zapisywana w nagłówku.
pub const VERSION: u8 = 1;
/// Rozmiar nagłówka: magia + wersja; pierwszy chunk zaczyna się od tego offsetu.
pub const HEADER_LEN: u64 = 6;
/// Rozmiar końcówki pliku: długość stopki (u32) + magia.
pub const TAIL_LEN: u64 = 9;

/// Bajt opisujący typ kolumny w stopce.
pub fn column_type_to_byte(ty: ColumnType) -> u8 {
    match ty {
        ColumnType::Int64 => 0,
        ColumnType::Varchar => 1,
        ColumnType::Bool => 2,
    }
}

/// Odwrotność `column_type_to_byte`; nieznany bajt oznacza uszkodzony plik.
pub fn column_type_from_byte(b: u8) -> Result<ColumnType> {
    match b {
        0 => Ok(ColumnType::Int64),
        1 => Ok(ColumnType::Varchar),
        2 => Ok(ColumnType::Bool),
        _ => Err(FormatError::Corrupted(format!("nieznany typ kolumny {}", b))),
    }
}

/// Położenie jednego chunka (jednej kolumny jednej grupy wierszy) w pliku.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkMeta {
    /// Pozycja pierwszego bajtu chunka od początku pliku.
    pub offset: u64,
    /// Liczba bajtów chunka w pliku.
    pub size: u64,
    /// Rozmiar danych przed kompresją (dla INT64 równy `size`).
    pub raw_size: u64,
}

/// Opis jednej grupy wierszy: liczba wierszy i położenie chunka każdej kolumny.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowGroupMeta {
    pub row_count: u64,
    /// `chunks[i]` to chunk i-tej kolumny schematu.
    pub chunks: Vec<ChunkMeta>,
}
