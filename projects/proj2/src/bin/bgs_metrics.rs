// Wczytuje plik .bgs i wypisuje metryki kolumn na stdout:
//   plik → deserializator (FileReader) → batche w pamięci → metryki → stdout
// Plik jest czytany grupa po grupie, więc może być większy niż RAM.
//
// Wywołanie: bgs_metrics <plik.bgs>

use std::env;
use std::path::Path;
use std::process;

use bigos::error::Result;
use bigos::metrics::Metrics;
use bigos::reader::FileReader;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: bgs_metrics <plik.bgs>");
        process::exit(1);
    }

    match run(Path::new(&args[1])) {
        Ok(report) => print!("{}", report),
        Err(e) => {
            eprintln!("bgs_metrics: {}", e);
            process::exit(1);
        }
    }
}

fn run(path: &Path) -> Result<String> {
    let reader = FileReader::open(path)?;
    let schema = reader.schema();

    let mut all_columns = Vec::new();
    for c in 0..schema.columns.len() {
        all_columns.push(c);
    }

    let mut metrics = Metrics::new(schema);
    for i in 0..reader.num_row_groups() {
        let batch = reader.read_row_group(i, &all_columns)?;
        metrics.add_batch(&batch)?;
        // batch jest tu zwalniany – w pamięci jest naraz tylko jedna grupa wierszy
    }
    Ok(metrics.report(schema))
}
