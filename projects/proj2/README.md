# Projekt 2 – format pliku BIGOS (`.bgs`)

Kolumnowy format pliku z serializatorem i deserializatorem (biblioteka `bigos` w Ruście) oraz dwa programy:

- `bgs_metrics <plik.bgs>` – wczytuje plik i wypisuje średnią każdej kolumny INT64, entropię binarną
  każdej kolumny BOOLEAN oraz liczbę znaków ASCII każdej kolumny VARCHAR,
- `bgs_gen <wyjście.bgs> <liczba_wierszy> [ziarno=42]` – generuje plik z deterministycznymi danymi.

**Plik przykładowy:** `sample/sample.bgs` (kolumny `id`: INT64, `flag`: BOOLEAN, `name`, `comment`: VARCHAR;
20 000 wierszy), oczekiwany wynik: `sample/sample_metrics.txt`.

## Budowanie i uruchomienie

Wymagany Rust (`cargo`) i kompilator C (biblioteka `zstd` buduje libzstd ze źródeł).

```
make all      # programy w target/release/
make test     # testy
make sample   # odtwarza sample/sample.bgs i sample/sample_metrics.txt
```

```
$ target/release/bgs_metrics sample/sample.bgs
id INT64 avg=9999.500000
flag BOOLEAN entropy=0.885827
name VARCHAR ascii_chars=113259
comment VARCHAR ascii_chars=598483
```

## Format pliku

```
HEADER : "BIGOS" | wersja (1 B)
grupa wierszy 0: chunk kolumny 0 | chunk kolumny 1 | …
grupa wierszy 1: …
FOOTER : schemat (typ i nazwa kolumn) + dla każdej grupy: liczba wierszy
         oraz offset, rozmiar i rozmiar przed kompresją każdego chunka
TAIL   : długość stopki (u32 LE) | "BIGOS"
```

- Tabela jest podzielona na **grupy wierszy** po maks. `BATCH_SIZE` = 1024 wiersze, więc zapis i odczyt działają
  w stałej pamięci, także dla plików większych niż RAM.
- Każda kolumna grupy to osobny **chunk**, dzięki czemu można czytać tylko wybrane kolumny.
- Metadane są w **stopce**, bo rozmiary chunków po kompresji są znane dopiero po zapisie. Brak magii na końcu
  oznacza niedokończony zapis.

Kodowanie kolumn:

- **INT64** – delta (różnica względem poprzedniej wartości), ZigZag (małe ujemne → małe dodatnie) i VLE
  (7 bitów na bajt): `[100, 101, 99] → [100, 1, −2] → [200, 2, 3] → C8 01 02 03`.
- **BOOLEAN** – 1 bit na wartość, 8 wartości w bajcie (najmłodszy bit pierwszy):
  `[true, false, true, true] → 0x0D`.
- **VARCHAR** – długości napisów (VLE), a po nich sklejone bajty napisów; całość kompresowana ZSTD.

Uszkodzony plik (zła magia, offsety poza plikiem, zła liczba wartości itp.) kończy się błędem, a nie paniką.

## Reprezentacja w pamięci

```rust
pub enum Column {
    Int64(Vec<i64>),
    Bool(Vec<bool>),
    Varchar { offsets: Vec<u64>, data: Vec<u8> },  // napis i = data[offsets[i]..offsets[i+1]]
}
pub struct Batch { pub num_rows: usize, pub columns: Vec<Column> }  // maks. BATCH_SIZE wierszy
```

Dane są trzymane kolumnowo (Struct of Arrays) w ciągłych tablicach: pętle po kolumnie czytają pamięć
sekwencyjnie, a kolumna napisów to dwie alokacje zamiast osobnej alokacji na każdy napis.
Deserializator zwraca plik jako ciąg batchy (`FileReader::read_row_group`), każdy z co najwyżej `BATCH_SIZE`
wierszami.

Dostęp do wierszy: `Column::get_i64(i)`, `Column::get_bool(i)`, `Column::varchar_get(i)`.
Przykładowa operacja: `Column::concat_varchar(&s1, &s2)` tworzy kolumnę, w której wiersz i to `s1[i] ++ s2[i]`.
