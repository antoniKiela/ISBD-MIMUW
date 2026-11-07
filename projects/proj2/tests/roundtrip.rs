// Testy end-to-end: dane w pamięci → FileWriter → plik → FileReader → dane w pamięci.
// Oczekujemy, że dane po odczycie są identyczne z zapisanymi.
//
// Ten plik jest osobnym programem testowym: widzi bibliotekę `bigos` tak jak każdy
// zewnętrzny użytkownik – tylko jej publiczne API.

use std::path::PathBuf;

use bigos::error::FormatError;
use bigos::reader::FileReader;
use bigos::types::{Batch, Column, ColumnDef, ColumnType, Schema};
use bigos::writer::FileWriter;

fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("bigos_roundtrip_{}_{}.bgs", name, std::process::id()))
}

/// Zapisuje batche do pliku, odczytuje go w całości i usuwa plik.
fn write_and_read(name: &str, schema: &Schema, batches: &[Batch]) -> (Schema, Vec<Batch>) {
    let path = temp_path(name);
    let mut w = FileWriter::create(&path, schema.clone()).unwrap();
    for b in batches {
        w.write_batch(b).unwrap();
    }
    w.finish().unwrap();

    let r = FileReader::open(&path).unwrap();
    let result = (r.schema().clone(), r.read_all().unwrap());
    std::fs::remove_file(&path).unwrap();
    result
}

/// Schemat jak w pliku przykładowym: 2 × INT64 + 2 × VARCHAR.
fn schema_2i_2v() -> Schema {
    Schema::new(vec![
        ColumnDef { name: "id".to_string(), ty: ColumnType::Int64 },
        ColumnDef { name: "value".to_string(), ty: ColumnType::Int64 },
        ColumnDef { name: "name".to_string(), ty: ColumnType::Varchar },
        ColumnDef { name: "comment".to_string(), ty: ColumnType::Varchar },
    ])
    .unwrap()
}

fn batch(schema: &Schema, ids: Vec<i64>, values: Vec<i64>, names: &[&str], comments: &[&str]) -> Batch {
    Batch::new(
        schema,
        vec![
            Column::Int64(ids),
            Column::Int64(values),
            Column::varchar_from_strs(names),
            Column::varchar_from_strs(comments),
        ],
    )
    .unwrap()
}

#[test]
fn edge_cases_in_multiple_row_groups() {
    let schema = schema_2i_2v();
    let long = "x".repeat(100_000);
    let batches = vec![
        // skrajne INT64 na przemian (delty przepełniają się i zawijają), puste napisy, UTF-8
        batch(
            &schema,
            vec![1, 2, 3, 4],
            vec![i64::MAX, i64::MIN, i64::MAX, -1],
            &["", "żółw", "", "ASCII"],
            &["a", "", "zażółć gęślą jaźń", ""],
        ),
        // bardzo długi napis
        batch(&schema, vec![5, 6, 7, 8], vec![0, 0, 0, i64::MIN], &[&long, "b", "c", "d"], &["", "", "", ""]),
        // ostatnia grupa niepełna
        batch(&schema, vec![9, 10], vec![-5, 5], &["koniec", ""], &["", "!"]),
    ];

    // pusty batch po drodze writer pomija – nie powinien pojawić się w pliku
    let empty = batch(&schema, vec![], vec![], &[], &[]);
    let to_write = vec![batches[0].clone(), empty, batches[1].clone(), batches[2].clone()];

    let (read_schema, read_batches) = write_and_read("edge", &schema, &to_write);
    assert_eq!(read_schema, schema);
    assert_eq!(read_batches, batches);
}

#[test]
fn empty_tables() {
    // kolumny są, ale brak wierszy: schemat się zachowuje, grup wierszy brak
    let schema = schema_2i_2v();
    let (read_schema, read_batches) = write_and_read("no_rows", &schema, &[]);
    assert_eq!(read_schema, schema);
    assert!(read_batches.is_empty());

    // tabela bez kolumn
    let schema = Schema::new(vec![]).unwrap();
    let (read_schema, read_batches) = write_and_read("no_columns", &schema, &[]);
    assert_eq!(read_schema, schema);
    assert!(read_batches.is_empty());
}

#[test]
fn projection_reads_only_selected_columns() {
    let schema = schema_2i_2v();
    let path = temp_path("projection");
    let mut w = FileWriter::create(&path, schema.clone()).unwrap();
    w.write_batch(&batch(&schema, vec![1, 2], vec![10, 20], &["a", "b"], &["x", "y"])).unwrap();
    w.write_batch(&batch(&schema, vec![3], vec![30], &["c"], &["z"])).unwrap();
    w.finish().unwrap();

    let r = FileReader::open(&path).unwrap();
    assert_eq!(r.num_row_groups(), 2);
    assert_eq!(r.row_group_rows(0), Some(2));
    assert_eq!(r.row_group_rows(2), None);

    // kolumny 3 i 0, w tej kolejności
    let b = r.read_row_group(1, &[3, 0]).unwrap();
    assert_eq!(
        b,
        Batch { num_rows: 1, columns: vec![Column::varchar_from_strs(&["z"]), Column::Int64(vec![3])] }
    );

    // nieistniejąca kolumna albo grupa to błąd wywołującego, a nie uszkodzony plik
    assert!(matches!(r.read_row_group(0, &[4]), Err(FormatError::InvalidData(_))));
    assert!(matches!(r.read_row_group(2, &[0]), Err(FormatError::InvalidData(_))));

    std::fs::remove_file(&path).unwrap();
}
