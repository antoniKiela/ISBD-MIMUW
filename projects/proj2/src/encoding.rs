// Kodowania używane w pliku BIGOS:
// - ZigZag  – zamiana i64 na u64 tak, by liczby bliskie zera (także ujemne) były małe,
// - VLE     – liczba u64 zapisana na 1–10 bajtach (małe liczby zajmują mało miejsca),
// - delta   – zamiast wartości zapisujemy różnicę względem poprzedniej,
// - ZSTD    – ogólna kompresja bajtów, używana dla kolumn VARCHAR.
//
// Chunk INT64   = VLE(zigzag(delta[0])) … VLE(zigzag(delta[n-1]))
// Chunk BOOLEAN = 1 bit na wartość, 8 wartości na bajt (najmłodszy bit pierwszy)
// Chunk VARCHAR = ZSTD( VLE(len[0]) … VLE(len[n-1]) ++ sklejone bajty wszystkich napisów )

use crate::error::{FormatError, Result};
use crate::types::{Column, ColumnType};

/// Poziom kompresji ZSTD (3 = domyślny, dobry kompromis szybkość/rozmiar).
const ZSTD_LEVEL: i32 = 3;

/// Maksymalna długość VLE dla u64: ceil(64 / 7) = 10 bajtów.
const MAX_VLE_BYTES: usize = 10;

// ---------------------------------------------------------------------------
// ZigZag
// ---------------------------------------------------------------------------

/// 0 → 0, -1 → 1, 1 → 2, -2 → 3, 2 → 4, …
pub fn zigzag_encode(n: i64) -> u64 {
    // `n >> 63` (przesunięcie arytmetyczne) daje 0 dla n ≥ 0 i same jedynki dla n < 0.
    ((n << 1) ^ (n >> 63)) as u64
}

/// Odwrotność `zigzag_encode`.
pub fn zigzag_decode(u: u64) -> i64 {
    // Najmłodszy bit mówi o znaku; `-(…)` daje 0 albo same jedynki.
    ((u >> 1) as i64) ^ -((u & 1) as i64)
}

// ---------------------------------------------------------------------------
// VLE (unsigned LEB128)
// ---------------------------------------------------------------------------

/// Dopisuje `value` na koniec `out`: po 7 bitów od najmłodszych,
/// najstarszy bit bajtu (0x80) oznacza „jest kolejny bajt”.
pub fn write_vle(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value as u8 & 0x7F) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

/// Czyta jedną liczbę VLE z `buf` od pozycji `*pos` i przesuwa `*pos` za nią.
pub fn read_vle(buf: &[u8], pos: &mut usize) -> Result<u64> {
    let mut value: u64 = 0;
    for i in 0..MAX_VLE_BYTES {
        if *pos >= buf.len() {
            return Err(FormatError::Corrupted("ucięta liczba VLE".to_string()));
        }
        let byte = buf[*pos];
        *pos += 1;

        // 10. bajt może wnieść już tylko 1 bit (9 * 7 = 63 bity są zajęte).
        if i == MAX_VLE_BYTES - 1 && byte > 1 {
            return Err(FormatError::Corrupted("liczba VLE nie mieści się w u64".to_string()));
        }
        value |= ((byte & 0x7F) as u64) << (7 * i);
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(FormatError::Corrupted("liczba VLE dłuższa niż 10 bajtów".to_string()))
}

// ---------------------------------------------------------------------------
// Chunki kolumn
// ---------------------------------------------------------------------------

/// Zakodowany chunk kolumny gotowy do zapisu w pliku.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedChunk {
    /// Bajty zapisywane do pliku.
    pub bytes: Vec<u8>,
    /// Rozmiar danych przed kompresją ZSTD (dla INT64 równy `bytes.len()`).
    pub raw_size: u64,
}

/// Koduje kolumnę (INT64: delta + zigzag + VLE, BOOLEAN: 1 bit na wartość,
/// VARCHAR: długości VLE + bajty, całość ZSTD).
pub fn encode_column(col: &Column) -> Result<EncodedChunk> {
    col.validate()?;
    match col {
        Column::Int64(values) => {
            let bytes = encode_int64(values);
            let raw_size = bytes.len() as u64;
            Ok(EncodedChunk { bytes, raw_size })
        }
        Column::Bool(values) => {
            let bytes = encode_bool(values);
            let raw_size = bytes.len() as u64;
            Ok(EncodedChunk { bytes, raw_size })
        }
        Column::Varchar { offsets, data } => encode_varchar(offsets, data),
    }
}

/// Dekoduje chunk o typie `ty`, który powinien zawierać dokładnie `count` wartości.
pub fn decode_column(ty: ColumnType, bytes: &[u8], count: usize, raw_size: u64) -> Result<Column> {
    // INT64 i BOOLEAN nie są kompresowane, więc rozmiar przed kompresją = rozmiar chunka.
    if ty != ColumnType::Varchar && raw_size != bytes.len() as u64 {
        return Err(FormatError::Corrupted(
            "dla INT64 i BOOLEAN raw_size musi być równy rozmiarowi chunka".to_string(),
        ));
    }
    match ty {
        ColumnType::Int64 => Ok(Column::Int64(decode_int64(bytes, count)?)),
        ColumnType::Bool => Ok(Column::Bool(decode_bool(bytes, count)?)),
        ColumnType::Varchar => decode_varchar(bytes, count, raw_size),
    }
}

fn encode_int64(values: &[i64]) -> Vec<u8> {
    // Większość delt mieści się w 1–2 bajtach – rezerwujemy tyle na start.
    let mut out = Vec::with_capacity(values.len() * 2);
    let mut prev: i64 = 0;
    for &v in values {
        // Pierwsza delta to sama wartość (prev = 0). Odejmowanie modulo 2^64 nie może
        // się przepełnić, a `wrapping_add` przy dekodowaniu odtwarza wartość dokładnie.
        let delta = v.wrapping_sub(prev);
        write_vle(&mut out, zigzag_encode(delta));
        prev = v;
    }
    out
}

fn decode_int64(bytes: &[u8], count: usize) -> Result<Vec<i64>> {
    // Każda wartość zajmuje co najmniej 1 bajt – chroni przed ogromną alokacją
    // przy uszkodzonym `count`.
    if count > bytes.len() {
        return Err(FormatError::Corrupted(format!(
            "chunk INT64 ma {} B, a ma zawierać {} wartości",
            bytes.len(),
            count
        )));
    }
    let mut values = Vec::with_capacity(count);
    let mut pos = 0;
    let mut prev: i64 = 0;
    for _ in 0..count {
        let delta = zigzag_decode(read_vle(bytes, &mut pos)?);
        prev = prev.wrapping_add(delta);
        values.push(prev);
    }
    if pos != bytes.len() {
        return Err(FormatError::Corrupted("nadmiarowe bajty w chunku INT64".to_string()));
    }
    Ok(values)
}

fn encode_bool(values: &[bool]) -> Vec<u8> {
    // Wartość i trafia do bajtu i / 8, na bit i % 8; nieużywane bity ostatniego bajtu zostają zerami.
    let mut out = vec![0u8; values.len().div_ceil(8)];
    for (i, &v) in values.iter().enumerate() {
        if v {
            out[i / 8] |= 1 << (i % 8);
        }
    }
    out
}

fn decode_bool(bytes: &[u8], count: usize) -> Result<Vec<bool>> {
    if bytes.len() != count.div_ceil(8) {
        return Err(FormatError::Corrupted(format!(
            "chunk BOOLEAN ma {} B, a dla {} wartości powinien mieć {} B",
            bytes.len(),
            count,
            count.div_ceil(8)
        )));
    }
    let mut values = Vec::with_capacity(count);
    for i in 0..count {
        values.push(bytes[i / 8] & (1 << (i % 8)) != 0);
    }
    // Bity za ostatnią wartością muszą być zerami.
    if !count.is_multiple_of(8) && bytes[count / 8] >> (count % 8) != 0 {
        return Err(FormatError::Corrupted("niezerowe bity dopełnienia w chunku BOOLEAN".to_string()));
    }
    Ok(values)
}

fn encode_varchar(offsets: &[u64], data: &[u8]) -> Result<EncodedChunk> {
    // Bufor surowy: najpierw długości wszystkich napisów, potem ich bajty.
    let mut raw = Vec::with_capacity(offsets.len() + data.len());
    for i in 1..offsets.len() {
        write_vle(&mut raw, offsets[i] - offsets[i - 1]);
    }
    raw.extend_from_slice(data);

    let bytes = zstd::bulk::compress(&raw, ZSTD_LEVEL)?;
    Ok(EncodedChunk { bytes, raw_size: raw.len() as u64 })
}

fn decode_varchar(bytes: &[u8], count: usize, raw_size: u64) -> Result<Column> {
    // ZSTD zapisuje rozmiar danych w nagłówku ramki. Sprawdzamy go z `raw_size` ze stopki
    // przed dekompresją, żeby uszkodzony plik nie wymusił alokacji ogromnego bufora.
    match zstd::zstd_safe::get_frame_content_size(bytes) {
        Ok(Some(size)) if size == raw_size => {}
        _ => {
            return Err(FormatError::Corrupted(
                "rozmiar ramki ZSTD nie zgadza się z raw_size".to_string(),
            ));
        }
    }
    let mut raw = match zstd::bulk::decompress(bytes, raw_size as usize) {
        Ok(raw) => raw,
        Err(e) => {
            return Err(FormatError::Corrupted(format!("błąd dekompresji ZSTD: {}", e)));
        }
    };
    if raw.len() as u64 != raw_size {
        return Err(FormatError::Corrupted("zły rozmiar danych po dekompresji".to_string()));
    }

    // Każda długość zajmuje co najmniej 1 bajt.
    if count > raw.len() {
        return Err(FormatError::Corrupted(format!(
            "chunk VARCHAR ma {} B, a ma zawierać {} napisów",
            raw.len(),
            count
        )));
    }

    // Długości → offsety.
    let mut offsets = Vec::with_capacity(count + 1);
    offsets.push(0u64);
    let mut pos = 0;
    let mut total: u64 = 0;
    for _ in 0..count {
        let len = read_vle(&raw, &mut pos)?;
        total = match total.checked_add(len) {
            Some(t) => t,
            None => return Err(FormatError::Corrupted("suma długości napisów za duża".to_string())),
        };
        offsets.push(total);
    }

    // Reszta bufora to dokładnie sklejone napisy.
    if (raw.len() - pos) as u64 != total {
        return Err(FormatError::Corrupted(
            "suma długości napisów nie zgadza się z liczbą bajtów".to_string(),
        ));
    }
    // Usuwa długości z początku bufora; zostają same napisy (bez nowej alokacji).
    raw.drain(..pos);

    Ok(Column::Varchar { offsets, data: raw })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_corrupted<T>(r: Result<T>) -> bool {
        matches!(r, Err(FormatError::Corrupted(_)))
    }

    #[test]
    fn zigzag() {
        assert_eq!(zigzag_encode(0), 0);
        assert_eq!(zigzag_encode(-1), 1);
        assert_eq!(zigzag_encode(1), 2);
        assert_eq!(zigzag_encode(-2), 3);
        for n in [0, 1, -1, 1000, -1000, i64::MAX, i64::MIN] {
            assert_eq!(zigzag_decode(zigzag_encode(n)), n);
        }
    }

    #[test]
    fn vle() {
        // ręcznie policzone bajty
        let mut buf = Vec::new();
        write_vle(&mut buf, 300);
        assert_eq!(buf, vec![0xAC, 0x02]);

        // kilka liczb w jednym buforze, odczyt po kolei
        let values = [0, 127, 128, 300, u64::MAX];
        let mut buf = Vec::new();
        for v in values {
            write_vle(&mut buf, v);
        }
        let mut pos = 0;
        for v in values {
            assert_eq!(read_vle(&buf, &mut pos).unwrap(), v);
        }
        assert_eq!(pos, buf.len());

        // ucięta liczba oraz liczba dłuższa niż 10 bajtów
        assert!(is_corrupted(read_vle(&[0x80], &mut 0)));
        assert!(is_corrupted(read_vle(&[0x80; 11], &mut 0)));
    }

    #[test]
    fn int64_chunk() {
        // [100, 101, 99] → delty [100, 1, -2] → zigzag [200, 2, 3] → VLE
        let chunk = encode_column(&Column::Int64(vec![100, 101, 99])).unwrap();
        assert_eq!(chunk.bytes, vec![0xC8, 0x01, 0x02, 0x03]);

        // skrajne wartości (delta zawija się modulo 2^64) oraz pusta kolumna
        for values in [vec![i64::MAX, i64::MIN, 0, -1, i64::MIN], vec![]] {
            let col = Column::Int64(values);
            let chunk = encode_column(&col).unwrap();
            let decoded =
                decode_column(ColumnType::Int64, &chunk.bytes, col.len(), chunk.raw_size).unwrap();
            assert_eq!(decoded, col);
        }
    }

    #[test]
    fn varchar_chunk() {
        // przed kompresją: długości 2, 0, 7 jako VLE, potem bajty "abżółw"
        let col = Column::varchar_from_strs(&["ab", "", "żółw"]);
        let chunk = encode_column(&col).unwrap();
        let mut expected_raw = vec![0x02, 0x00, 0x07];
        expected_raw.extend_from_slice("abżółw".as_bytes());
        assert_eq!(zstd::bulk::decompress(&chunk.bytes, 100).unwrap(), expected_raw);

        let decoded = decode_column(ColumnType::Varchar, &chunk.bytes, 3, chunk.raw_size).unwrap();
        assert_eq!(decoded, col);
    }

    #[test]
    fn decode_rejects_corrupted_chunks() {
        // INT64: chunk ma 3 wartości, a oczekujemy 2 albo 4
        let bytes = [0xC8, 0x01, 0x02, 0x03];
        assert!(is_corrupted(decode_column(ColumnType::Int64, &bytes, 2, 4)));
        assert!(is_corrupted(decode_column(ColumnType::Int64, &bytes, 4, 4)));

        // VARCHAR: zły raw_size, zła liczba napisów, śmieci zamiast ramki ZSTD
        let chunk = encode_column(&Column::varchar_from_strs(&["ab", "cd"])).unwrap();
        assert!(is_corrupted(decode_column(ColumnType::Varchar, &chunk.bytes, 2, chunk.raw_size + 1)));
        assert!(is_corrupted(decode_column(ColumnType::Varchar, &chunk.bytes, 3, chunk.raw_size)));
        assert!(is_corrupted(decode_column(ColumnType::Varchar, &[1, 2, 3], 1, 10)));
    }
}
