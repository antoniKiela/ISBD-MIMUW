//! Biblioteka formatu BIGOS (`.bgs`)

pub mod encoding; // zigzag, VLE, delta, kodowanie chunków INT64 i VARCHAR
pub mod error; // FormatError – wspólny typ błędu biblioteki
pub mod format; // stałe formatu pliku i metadane chunków
pub mod metrics; // średnie kolumn INT64, liczba znaków ASCII kolumn VARCHAR
pub mod reader; // FileReader – deserializator
pub mod rng; // deterministyczny generator liczb pseudolosowych (dane testowe)
pub mod types; // reprezentacja danych w pamięci: Schema, Column, Batch
pub mod writer; // FileWriter – serializator
