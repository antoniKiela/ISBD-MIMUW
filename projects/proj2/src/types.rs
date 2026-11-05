// Reprezentacja danych w pamięci: schemat tabeli oraz kolumnowe porcje wierszy (batche).
//
// Dane są trzymane kolumnowo w ciągłych tablicach:
// - INT64   – jeden `Vec<i64>`,
// - VARCHAR – wszystkie napisy sklejone w jednym `Vec<u8>` (`data`) oraz `Vec<u64>` z pozycjami
//             początków (`offsets`); napis i to `data[offsets[i]..offsets[i + 1]]`.
// Dzięki temu pętle po kolumnie czytają pamięć sekwencyjnie (dobrze dla cache i SIMD),
// a kolumna napisów to dwie alokacje zamiast jednej na każdy napis.

use crate::error::{FormatError, Result};

/// Typ kolumny.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnType {
    Int64,
    Varchar,
}

/// Opis jednej kolumny: nazwa i typ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnDef {
    pub name: String,
    pub ty: ColumnType,
}

/// Schemat tabeli: lista kolumn w ustalonej kolejności.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schema {
    pub columns: Vec<ColumnDef>,
}

impl Schema {
    /// Tworzy schemat; nazwy kolumn muszą być unikatowe.
    pub fn new(columns: Vec<ColumnDef>) -> Result<Schema> {
        for i in 0..columns.len() {
            for j in 0..i {
                if columns[i].name == columns[j].name {
                    return Err(FormatError::InvalidData(format!(
                        "powtórzona nazwa kolumny '{}'",
                        columns[i].name
                    )));
                }
            }
        }
        Ok(Schema { columns })
    }
}

/// Wartości jednej kolumny (dla jednej porcji wierszy).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Column {
    Int64(Vec<i64>),
    /// `offsets.len()` == liczba wierszy + 1, `offsets[0] == 0`, ostatni offset == `data.len()`.
    Varchar { offsets: Vec<u64>, data: Vec<u8> },
}

impl Column {
    /// Buduje kolumnę VARCHAR z listy napisów.
    pub fn varchar_from_strs(values: &[&str]) -> Column {
        let mut offsets = Vec::with_capacity(values.len() + 1);
        let mut data = Vec::new();
        offsets.push(0);
        for v in values {
            data.extend_from_slice(v.as_bytes());
            offsets.push(data.len() as u64);
        }
        Column::Varchar { offsets, data }
    }

    /// Typ tej kolumny.
    pub fn column_type(&self) -> ColumnType {
        match self {
            Column::Int64(_) => ColumnType::Int64,
            Column::Varchar { .. } => ColumnType::Varchar,
        }
    }

    /// Liczba wierszy w kolumnie.
    pub fn len(&self) -> usize {
        match self {
            Column::Int64(values) => values.len(),
            Column::Varchar { offsets, .. } => {
                if offsets.is_empty() {
                    0
                } else {
                    offsets.len() - 1
                }
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Bajty i-tego napisu; `None`, gdy kolumna nie jest VARCHAR albo `i` jest poza zakresem.
    pub fn varchar_get(&self, i: usize) -> Option<&[u8]> {
        match self {
            Column::Varchar { offsets, data } => {
                if i + 1 >= offsets.len() {
                    return None;
                }
                let start = offsets[i] as usize;
                let end = offsets[i + 1] as usize;
                data.get(start..end)
            }
            Column::Int64(_) => None,
        }
    }

    /// Sprawdza wewnętrzną spójność kolumny VARCHAR (dla INT64 zawsze OK).
    fn validate(&self) -> Result<()> {
        if let Column::Varchar { offsets, data } = self {
            if offsets.is_empty() || offsets[0] != 0 {
                return Err(FormatError::InvalidData(
                    "offsety VARCHAR muszą zaczynać się od 0".to_string(),
                ));
            }
            for i in 1..offsets.len() {
                if offsets[i] < offsets[i - 1] {
                    return Err(FormatError::InvalidData(
                        "offsety VARCHAR muszą być niemalejące".to_string(),
                    ));
                }
            }
            if offsets[offsets.len() - 1] != data.len() as u64 {
                return Err(FormatError::InvalidData(
                    "ostatni offset VARCHAR musi być równy długości danych".to_string(),
                ));
            }
        }
        Ok(())
    }
}

/// Porcja wierszy tabeli w układzie kolumnowym; wszystkie kolumny mają `num_rows` wartości.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
    pub num_rows: usize,
    pub columns: Vec<Column>,
}

impl Batch {
    /// Tworzy batch zgodny ze schematem: ta sama liczba i typy kolumn, kolumny równej długości.
    pub fn new(schema: &Schema, columns: Vec<Column>) -> Result<Batch> {
        if columns.len() != schema.columns.len() {
            return Err(FormatError::InvalidData(format!(
                "schemat ma {} kolumn, a podano {}",
                schema.columns.len(),
                columns.len()
            )));
        }

        let mut num_rows = 0;
        for (i, col) in columns.iter().enumerate() {
            let def = &schema.columns[i];
            if col.column_type() != def.ty {
                return Err(FormatError::InvalidData(format!(
                    "kolumna '{}' ma typ {:?}, a podano {:?}",
                    def.name,
                    def.ty,
                    col.column_type()
                )));
            }
            col.validate()?;
            if i == 0 {
                num_rows = col.len();
            } else if col.len() != num_rows {
                return Err(FormatError::InvalidData(format!(
                    "kolumna '{}' ma {} wierszy, a pierwsza kolumna {}",
                    def.name,
                    col.len(),
                    num_rows
                )));
            }
        }

        Ok(Batch { num_rows, columns })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, ty: ColumnType) -> ColumnDef {
        ColumnDef { name: name.to_string(), ty }
    }

    fn schema_2i_2v() -> Schema {
        Schema::new(vec![
            col("a", ColumnType::Int64),
            col("b", ColumnType::Int64),
            col("c", ColumnType::Varchar),
            col("d", ColumnType::Varchar),
        ])
        .unwrap()
    }

    #[test]
    fn varchar_from_strs_builds_offsets() {
        let c = Column::varchar_from_strs(&["ab", "", "żółw"]);
        // "żółw" to 7 bajtów w UTF-8 (ż, ó, ł po 2 bajty + w)
        assert_eq!(
            c,
            Column::Varchar { offsets: vec![0, 2, 2, 9], data: "abżółw".as_bytes().to_vec() }
        );
        assert_eq!(c.len(), 3);
        assert_eq!(c.varchar_get(0), Some("ab".as_bytes()));
        assert_eq!(c.varchar_get(1), Some("".as_bytes()));
        assert_eq!(c.varchar_get(2), Some("żółw".as_bytes()));
        assert_eq!(c.varchar_get(3), None);
    }

    #[test]
    fn empty_varchar_column() {
        let c = Column::varchar_from_strs(&[]);
        assert_eq!(c, Column::Varchar { offsets: vec![0], data: vec![] });
        assert!(c.is_empty());
    }

    #[test]
    fn valid_batch() {
        let b = Batch::new(
            &schema_2i_2v(),
            vec![
                Column::Int64(vec![1, 2, 3]),
                Column::Int64(vec![-1, 0, i64::MAX]),
                Column::varchar_from_strs(&["x", "yy", ""]),
                Column::varchar_from_strs(&["", "", "zzz"]),
            ],
        )
        .unwrap();
        assert_eq!(b.num_rows, 3);
    }

    #[test]
    fn empty_schema_gives_empty_batch() {
        let b = Batch::new(&Schema::new(vec![]).unwrap(), vec![]).unwrap();
        assert_eq!(b.num_rows, 0);
    }

    #[test]
    fn duplicate_column_names_rejected() {
        let r = Schema::new(vec![col("a", ColumnType::Int64), col("a", ColumnType::Varchar)]);
        assert!(matches!(r, Err(FormatError::InvalidData(_))));
    }

    #[test]
    fn different_lengths_rejected() {
        let r = Batch::new(
            &schema_2i_2v(),
            vec![
                Column::Int64(vec![1, 2, 3]),
                Column::Int64(vec![1, 2]),
                Column::varchar_from_strs(&["x", "y", "z"]),
                Column::varchar_from_strs(&["x", "y", "z"]),
            ],
        );
        assert!(matches!(r, Err(FormatError::InvalidData(_))));
    }

    #[test]
    fn type_mismatch_rejected() {
        let r = Batch::new(
            &schema_2i_2v(),
            vec![
                Column::Int64(vec![1]),
                Column::varchar_from_strs(&["x"]),
                Column::varchar_from_strs(&["x"]),
                Column::varchar_from_strs(&["x"]),
            ],
        );
        assert!(matches!(r, Err(FormatError::InvalidData(_))));
    }

    #[test]
    fn wrong_column_count_rejected() {
        let r = Batch::new(&schema_2i_2v(), vec![Column::Int64(vec![1])]);
        assert!(matches!(r, Err(FormatError::InvalidData(_))));
    }

    #[test]
    fn broken_varchar_offsets_rejected() {
        let schema = Schema::new(vec![col("s", ColumnType::Varchar)]).unwrap();
        let broken = vec![
            Column::Varchar { offsets: vec![], data: vec![] },             // brak offsetów
            Column::Varchar { offsets: vec![1, 2], data: vec![0, 0] },     // nie zaczyna się od 0
            Column::Varchar { offsets: vec![0, 2, 1], data: vec![0, 0] },  // malejące
            Column::Varchar { offsets: vec![0, 1], data: vec![0, 0, 0] },  // ostatni != data.len()
        ];
        for c in broken {
            let r = Batch::new(&schema, vec![c]);
            assert!(matches!(r, Err(FormatError::InvalidData(_))));
        }
    }
}
