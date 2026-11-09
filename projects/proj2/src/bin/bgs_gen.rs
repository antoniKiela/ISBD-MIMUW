// Generuje plik .bgs z deterministycznymi danymi testowymi o schemacie:
//   id      INT64   – 0, 1, 2, … (stała delta = 1, koduje się na 1 bajcie)
//   flag    BOOLEAN – losowe, true z prawdopodobieństwem 0,3
//   name    VARCHAR – losowe słowo ze słownika (część słów z polskimi znakami)
//   comment VARCHAR – losowy napis ASCII długości 0..=60 (także pusty)
//
// Dane są zapisywane w grupach po BATCH_SIZE wierszy.
//
// Wywołanie: bgs_gen <wyjście.bgs> <liczba_wierszy> [ziarno=42]

use std::env;
use std::path::Path;
use std::process;

use bigos::error::Result;
use bigos::rng::Rng;
use bigos::types::{Batch, Column, ColumnDef, ColumnType, Schema, BATCH_SIZE};
use bigos::writer::FileWriter;

const WORDS: [&str; 24] = [
    "baza", "danych", "tabela", "kolumna", "wiersz", "indeks", "zapytanie", "plik", "dysk", "pamięć",
    "procesor", "wątek", "kompresja", "format", "bigos", "kapusta", "kiełbasa", "grzyby", "śliwki",
    "żurek", "pierogi", "gołąbki", "ćwikła", "źródło",
];

const COMMENT_CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 .,!?";

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 || args.len() > 4 {
        usage();
    }
    let rows = parse_arg(&args, 2, 0);
    let seed = parse_arg(&args, 3, 42);

    if let Err(e) = run(Path::new(&args[1]), rows, seed) {
        eprintln!("bgs_gen: {}", e);
        process::exit(1);
    }
}

fn usage() -> ! {
    eprintln!("Usage: bgs_gen <wyjście.bgs> <liczba_wierszy> [ziarno=42]");
    process::exit(1);
}

/// Argument nr `i` jako liczba; `default`, gdy go nie podano.
fn parse_arg(args: &[String], i: usize, default: u64) -> u64 {
    if i >= args.len() {
        return default;
    }
    match args[i].parse::<u64>() {
        Ok(v) => v,
        Err(_) => usage(),
    }
}

fn run(path: &Path, rows: u64, seed: u64) -> Result<()> {
    let schema = Schema::new(vec![
        ColumnDef { name: "id".to_string(), ty: ColumnType::Int64 },
        ColumnDef { name: "flag".to_string(), ty: ColumnType::Bool },
        ColumnDef { name: "name".to_string(), ty: ColumnType::Varchar },
        ColumnDef { name: "comment".to_string(), ty: ColumnType::Varchar },
    ])?;
    let mut rng = Rng::new(seed);
    let mut writer = FileWriter::create(path, schema.clone())?;

    let mut written: u64 = 0;
    while written < rows {
        let n = (rows - written).min(BATCH_SIZE as u64) as usize;

        let mut ids = Vec::with_capacity(n);
        let mut flags = Vec::with_capacity(n);
        let mut names = VarcharBuilder::new();
        let mut comments = VarcharBuilder::new();
        for j in 0..n {
            ids.push((written + j as u64) as i64);
            flags.push(rng.below(10) < 3);
            names.push(WORDS[rng.below(WORDS.len() as u64) as usize].as_bytes());

            let len = rng.below(61) as usize;
            let mut comment = Vec::with_capacity(len);
            for _ in 0..len {
                comment.push(COMMENT_CHARS[rng.below(COMMENT_CHARS.len() as u64) as usize]);
            }
            comments.push(&comment);
        }

        let batch = Batch::new(
            &schema,
            vec![Column::Int64(ids), Column::Bool(flags), names.finish(), comments.finish()],
        )?;
        writer.write_batch(&batch)?;
        written += n as u64;
    }
    writer.finish()?;

    let size = std::fs::metadata(path)?.len();
    eprintln!("bgs_gen: zapisano {} wierszy do {} ({} B)", rows, path.display(), size);
    Ok(())
}

/// Buduje kolumnę VARCHAR napis po napisie, od razu w układzie offsets + data.
struct VarcharBuilder {
    offsets: Vec<u64>,
    data: Vec<u8>,
}

impl VarcharBuilder {
    fn new() -> VarcharBuilder {
        VarcharBuilder { offsets: vec![0], data: Vec::new() }
    }

    fn push(&mut self, s: &[u8]) {
        self.data.extend_from_slice(s);
        self.offsets.push(self.data.len() as u64);
    }

    fn finish(self) -> Column {
        Column::Varchar { offsets: self.offsets, data: self.data }
    }
}
