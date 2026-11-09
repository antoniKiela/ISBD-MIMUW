// Metryki kolumn liczone strumieniowo, batch po batchu:
// - INT64   – średnia arytmetyczna wartości,
// - VARCHAR – liczba znaków ASCII (bajtów < 0x80) we wszystkich napisach.
//
// Trzymamy tylko akumulatory (sumę i licznik), a nie dane, więc pamięć nie zależy
// od rozmiaru pliku.

use crate::error::{FormatError, Result};
use crate::types::{Batch, Column, ColumnType, Schema};

/// Stan obliczeń dla jednej kolumny.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColumnMetric {
    /// Suma w i128 – suma wielu wartości i64 łatwo przekracza zakres i64.
    Int64Average { sum: i128, count: u64 },
    VarcharAscii { ascii_chars: u64 },
}

pub struct Metrics {
    pub columns: Vec<ColumnMetric>,
}

impl Metrics {
    /// Pusty stan (zero wierszy) dla każdej kolumny schematu.
    pub fn new(schema: &Schema) -> Metrics {
        let mut columns = Vec::with_capacity(schema.columns.len());
        for def in &schema.columns {
            columns.push(match def.ty {
                ColumnType::Int64 => ColumnMetric::Int64Average { sum: 0, count: 0 },
                ColumnType::Varchar => ColumnMetric::VarcharAscii { ascii_chars: 0 },
            });
        }
        Metrics { columns }
    }

    /// Dolicza wartości z kolejnego batcha (kolumny w kolejności schematu).
    pub fn add_batch(&mut self, batch: &Batch) -> Result<()> {
        if batch.columns.len() != self.columns.len() {
            return Err(FormatError::InvalidData(format!(
                "batch ma {} kolumn, a metryki liczymy dla {}",
                batch.columns.len(),
                self.columns.len()
            )));
        }
        for (metric, col) in self.columns.iter_mut().zip(&batch.columns) {
            match (metric, col) {
                (ColumnMetric::Int64Average { sum, count }, Column::Int64(values)) => {
                    // Prosta pętla po ciągłej tablicy – kompilator zamienia ją na instrukcje SIMD.
                    let mut batch_sum: i128 = 0;
                    for &v in values {
                        batch_sum += v as i128;
                    }
                    *sum += batch_sum;
                    *count += values.len() as u64;
                }
                (ColumnMetric::VarcharAscii { ascii_chars }, Column::Varchar { data, .. }) => {
                    // `data` zawiera dokładnie wszystkie napisy kolumny, sklejone.
                    let mut n: u64 = 0;
                    for &b in data {
                        if b < 0x80 {
                            n += 1;
                        }
                    }
                    *ascii_chars += n;
                }
                _ => {
                    return Err(FormatError::InvalidData(
                        "typ kolumny batcha nie zgadza się ze schematem".to_string(),
                    ));
                }
            }
        }
        Ok(())
    }

    /// Wynik w formacie tekstowym, jedna linia na kolumnę:
    ///   <nazwa> INT64 avg=<średnia z 6 miejscami po przecinku albo NULL dla 0 wierszy>
    ///   <nazwa> VARCHAR ascii_chars=<liczba>
    pub fn report(&self, schema: &Schema) -> String {
        let mut out = String::new();
        for (def, metric) in schema.columns.iter().zip(&self.columns) {
            match metric {
                ColumnMetric::Int64Average { sum, count } => {
                    if *count == 0 {
                        out.push_str(&format!("{} INT64 avg=NULL\n", def.name));
                    } else {
                        let avg = *sum as f64 / *count as f64;
                        out.push_str(&format!("{} INT64 avg={:.6}\n", def.name, avg));
                    }
                }
                ColumnMetric::VarcharAscii { ascii_chars } => {
                    out.push_str(&format!("{} VARCHAR ascii_chars={}\n", def.name, ascii_chars));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ColumnDef;

    #[test]
    fn metrics_over_two_batches() {
        let schema = Schema::new(vec![
            ColumnDef { name: "n".to_string(), ty: ColumnType::Int64 },
            ColumnDef { name: "s".to_string(), ty: ColumnType::Varchar },
        ])
        .unwrap();

        let mut m = Metrics::new(&schema);
        // przed jakimikolwiek danymi średnia jest niezdefiniowana
        assert_eq!(m.report(&schema), "n INT64 avg=NULL\ns VARCHAR ascii_chars=0\n");

        let b1 = Batch::new(
            &schema,
            vec![Column::Int64(vec![1, 2, -6]), Column::varchar_from_strs(&["ab", "żółw", ""])],
        )
        .unwrap();
        let b2 = Batch::new(
            &schema,
            vec![Column::Int64(vec![i64::MAX, i64::MAX]), Column::varchar_from_strs(&["xyz", "ą"])],
        )
        .unwrap();
        m.add_batch(&b1).unwrap();
        m.add_batch(&b2).unwrap();

        // średnia: (1 + 2 - 6 + 2 * i64::MAX) / 5 – suma nie mieści się w i64
        let expected_avg = (2.0 * i64::MAX as f64 - 3.0) / 5.0;
        // ASCII: "ab" (2) + "w" z "żółw" (1) + "xyz" (3); ż, ó, ł, ą to bajty ≥ 0x80
        let expected = format!("n INT64 avg={:.6}\ns VARCHAR ascii_chars=6\n", expected_avg);
        assert_eq!(m.report(&schema), expected);
    }
}
