# Projekt 2 – format pliku BIGOS (`.bgs`)

Kolumnowy format pliku z serializatorem i deserializatorem (biblioteka `bigos` w Ruście) oraz dwa programy:

- `bgs_metrics <plik.bgs>` – wczytuje plik i wypisuje średnią każdej kolumny INT64
  oraz liczbę znaków ASCII każdej kolumny VARCHAR,
- `bgs_gen <wyjście.bgs> <liczba_wierszy> [ziarno=42] [rozmiar_grupy=65536]` – generuje plik
  z deterministycznymi danymi (2 × INT64 + 2 × VARCHAR).

**Plik przykładowy:** `sample/sample.bgs` (kolumny `id`, `value`: INT64, `name`, `comment`: VARCHAR;
20 000 wierszy), oczekiwany wynik: `sample/sample_metrics.txt`.

## Budowanie i uruchomienie

```
make all      # programy w target/release/
make test     # testy
make sample   # odtwarza sample/sample.bgs i sample/sample_metrics.txt
```

```
$ target/release/bgs_metrics sample/sample.bgs
id INT64 avg=9999.500000
value INT64 avg=-202.289550
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

- Tabela jest podzielona na **grupy wierszy**, więc zapis i odczyt działają w stałej pamięci, także dla
  plików większych niż RAM.
- Każda kolumna grupy to osobny **chunk**, dzięki czemu można czytać tylko wybrane kolumny.
- Metadane są w **stopce**, bo rozmiary chunków po kompresji są znane dopiero po zapisie. Brak magii na końcu
  oznacza niedokończony zapis.

Kodowanie kolumn:

- **INT64** – delta (różnica względem poprzedniej wartości), ZigZag (małe ujemne → małe dodatnie) i VLE
  (7 bitów na bajt): `[100, 101, 99] → [100, 1, −2] → [200, 2, 3] → C8 01 02 03`.
- **VARCHAR** – długości napisów (VLE), a po nich sklejone bajty napisów; całość kompresowana ZSTD.

Uszkodzony plik (zła magia, offsety poza plikiem, zła liczba wartości itp.) kończy się błędem, a nie paniką.

## Reprezentacja w pamięci

```rust
pub enum Column {
    Int64(Vec<i64>),
    Varchar { offsets: Vec<u64>, data: Vec<u8> },  // napis i = data[offsets[i]..offsets[i+1]]
}
pub struct Batch { pub num_rows: usize, pub columns: Vec<Column> }  // jedna grupa wierszy
```

Dane są trzymane kolumnowo w ciągłych tablicach: pętle po kolumnie czytają pamięć sekwencyjnie,
a kolumna napisów to dwie alokacje zamiast osobnej alokacji na każdy napis.
