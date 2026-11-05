// Wspólny typ błędu biblioteki formatu BIGOS.

use std::fmt;

/// Błąd zwracany przez funkcje biblioteki.
#[derive(Debug)]
pub enum FormatError {
    /// Błąd systemu plików (open, read, write…).
    Io(std::io::Error),
    /// Plik nie jest poprawnym plikiem BIGOS albo jest uszkodzony.
    Corrupted(String),
    /// Dane w pamięci są niespójne (np. kolumny różnej długości, typ niezgodny ze schematem).
    InvalidData(String),
}

/// Skrót: `Result<T>` zamiast `std::result::Result<T, FormatError>`.
pub type Result<T> = std::result::Result<T, FormatError>;

// Czytelny opis błędu – używany przez `{}` w `println!`/`eprintln!`.
impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            FormatError::Io(e) => write!(f, "błąd wejścia/wyjścia: {}", e),
            FormatError::Corrupted(msg) => write!(f, "uszkodzony plik: {}", msg),
            FormatError::InvalidData(msg) => write!(f, "niepoprawne dane: {}", msg),
        }
    }
}

// Pozwala traktować FormatError jak każdy inny błąd w Ruście.
impl std::error::Error for FormatError {}

// Automatyczna zamiana błędu IO na FormatError – dzięki temu działa operator `?`
// na wynikach funkcji z std::fs / std::io.
impl From<std::io::Error> for FormatError {
    fn from(e: std::io::Error) -> FormatError {
        FormatError::Io(e)
    }
}
