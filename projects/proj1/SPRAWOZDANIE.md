# Sprawozdanie – Projekt 1
Antoni Kiela  
458905

## 1. Środowisko pomiarowe

| Element | Wartość |
| --- | --- |
| Procesor | Intel Core i5-9600K, 6 rdzeni / 6 wątków, 3,7–4,6 GHz, L3 9 MiB |
| Pamięć | 16 GiB RAM (fizycznie); dla systemu dostępne 15,5 GiB (`MemTotal`) |
| Dysk z plikami testowymi | NVMe Samsung SSD 970 EVO 250 GB, ext4, scheduler `none`, `read_ahead_kb = 128` |
| System | Linux 7.2.9 (Arch), Rust 1.99, `cargo build --release` |

Pliki testowe (losowe dane z `/dev/urandom`, katalog `test_files/`):

| Plik | Rozmiar | Rola |
| --- | --- | --- |
| `4M.bin` | 4 MiB | test poprawności, czasy rzędu milisekund |
| `4G.bin` | 4 GiB | mieści się w pamięci – porównanie zimnego i ciepłego cache |
| `16G.bin` | 16 GiB | większy niż pamięć dostępna dla systemu (15,5 GiB) – zawsze czytany z dysku |

Wszystkie pomiary wykonano na jednym dysku – NVMe Samsung 970 EVO.

## 2. Metodologia

- **Pomiar czasu**: `std::time::Instant` (zegar monotoniczny). Mierzymy od otwarcia pliku do końca obliczeń;
  start procesu i wypisanie wyniku nie są liczone. Program wypisuje czas na stderr (`time_ms=...`).
- **Powtórzenia**: każdą konfigurację uruchamiamy 3 razy i bierzemy **najkrótszy** czas.
  Wszystkie czasy są w kolumnie `runs_s`.
- **Zimny cache (`cold`)**: przed każdym uruchomieniem czyścimy pamięć podręczną plików:
  `sync` i `sudo sysctl -w vm.drop_caches=3`.
- **Ciepły cache (`warm`)**: najpierw jedno uruchomienie na rozgrzewkę, które wczytuje plik do pamięci, potem pomiary.
  Dla `16G.bin` tego trybu nie ma: plik nie mieści się w pamięci, więc przy czytaniu po kolei i tak zawsze jest
  czytany z dysku. Dlatego nie było potrzeby używać `MemoryMax`.
- Wyniki są w `results/latency_results.csv`, `throughput_results.csv` i `pipeline_results.csv`; czasy w sekundach.
- Pominięte kombinacje (trwałyby godzinami): `latency` z M = 512 na 4G/16G i M = 4096 na 16G (ponad 2 mln
  losowych odczytów) oraz `pipeline` z M = 64 KiB dla K = 10000 na 4G i K ≥ 1000 na 16G.

Pomiary na `4M.bin` trwają 6–86 ms, więc przeważa w nich szum i stałe koszty (np. otwarcie pliku).
Wnioski opieramy na `4G.bin` i `16G.bin`; wszystkie dane są w plikach CSV.

---

## 3. Latencja – losowy odczyt (`latency` → `latency_mt`)
Wywołanie: `latency_mt <plik> <M> <wątki>`.

### Wyniki (czas w sekundach, minimum z 3)

`4G.bin`, **zimny cache**:

| M | `latency` | T=1 | T=2 | T=4 | T=8 | T=16 | T=32 | T=64 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 4 KiB | 99,47 | 83,28 | 49,09 | 24,23 | 14,61 | 11,01 | 9,91 | **9,87** |
| 64 KiB | 46,36 | 24,91 | 13,96 | 10,70 | 9,59 | 9,21 | 8,87 | **8,52** |
| 1 MiB | 17,15 | 10,17 | 8,22 | 8,13 | 7,47 | 7,24 | 7,12 | **7,13** |

`4G.bin`, **ciepły cache** (plik w RAM):

| M | `latency` | T=1 | T=2 | T=4 | T=8 | T=16 | T=32 | T=64 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 4 KiB | 6,97 | 7,14 | 7,12 | 7,09 | 7,12 | 7,13 | 7,19 | 7,24 |
| 64 KiB | 6,16 | 6,03 | 6,04 | 6,04 | 6,06 | 6,06 | 6,09 | 6,16 |
| 1 MiB | 6,18 | 5,90 | 5,93 | 5,93 | 5,96 | 6,01 | 5,95 | 5,97 |

`16G.bin`, zimny cache:

| M | `latency` | T=1 | T=2 | T=4 | T=8 | T=16 | T=32 | T=64 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 64 KiB | 142,58 | 88,12 | 49,42 | 38,61 | 36,98 | 37,71 | 36,09 | **34,37** |
| 1 MiB | 68,64 | 41,61 | 33,31 | 32,61 | 30,45 | 29,10 | 28,43 | **28,42** |

### Czas działania w funkcji M i liczby wątków

- **Zimny cache, małe M**: podwojenie liczby wątków prawie o połowę skraca czas, aż do ok. 8 wątków
  (4 KiB: 83 s → 49 s → 24 s → 15 s). Dalej czas spada już powoli i przy 32–64 wątkach wynosi ~9,9 s,
  czyli **10× mniej** niż w oryginale (99,5 s).
- **Większe M**: odczytów jest mniej (dla 1 MiB 256 razy mniej niż dla 4 KiB), więc oryginał działa krócej
  (99 s → 46 s → 17 s). Wątki pomagają wtedy mniej, bo czas szybko zbliża się do samego czasu obliczeń:
  dla 1 MiB i 64 wątków to 7,1 s, a z plikiem w RAM 6,0 s.
- **Ciepły cache**: liczba wątków prawie nic nie zmienia (dla 4 KiB zawsze 7,1–7,2 s), a przy małym M
  więcej wątków nawet lekko wydłuża czas.

### Czy zwiększanie liczby wątków zmniejszy czas wykonania programu?

**Tak, ale tylko gdy program czeka na dysk – i tylko do pewnego momentu.**
Oryginał czyta jeden blok, czeka na dysk, liczy MD5 i dopiero wtedy prosi o następny blok. Dysk NVMe może
obsługiwać wiele odczytów naraz, ale dostaje zawsze tylko jeden. Dlatego czas ≈ liczba bloków × czas jednego
odczytu. Przy T wątkach dysk dostaje T odczytów naraz, więc czekanie na nie odbywa się równolegle.

Kolejne wątki przestają pomagać, gdy:
1. **nie nadąża jedyny wątek liczący** – dla 4 KiB samo liczenie trwa ~7 s (tyle trwa cały program, gdy plik
   jest w RAM), a przy 64 wątkach program trwa 9,9 s, czyli blisko tej granicy;
2. **dysk nie wykona więcej odczytów na sekundę** – dla `16G.bin` i M = 64 KiB czas stoi na 34–39 s już od
   4 wątków, choć samo liczenie zajmuje ~24 s.

Gdy plik jest w pamięci, nie ma na co czekać, więc wątki tylko dokładają koszt: przełączanie między wątkami
i przepychanie się przy kanale. Stąd lekki wzrost czasu przy większej liczbie wątków.

### Czy zwiększenie liczby wątków zmniejszy latencję operacji odczytu?

**Nie.** Więcej wątków oznacza więcej odczytów na sekundę (większą **przepustowość**), ale pojedynczy odczyt
nie staje się szybszy. Przy wielu wątkach jest wręcz wolniejszy, bo odczyty czekają w kolejce dysku.

Widać to w wynikach dla `4G.bin` i M = 4 KiB (zimny cache): do 4 wątków liczba odczytów na sekundę rośnie
prawie tyle razy, ile dodamy wątków, od 8 wątków coraz wolniej, a przy 32–64 wątkach już się nie zmienia
(9,91 vs 9,87 s). Dodatkowe wątki nie przyspieszają wtedy programu – ich odczyty tylko czekają w kolejce.

### Jak na latencję wpływa posiadanie pliku w pamięci (file cache)?

**Bardzo mocno.** Dla M = 4 KiB oryginał potrzebuje na jeden blok **94,9 µs** przy zimnym cache
(99,47 s / 1 048 576), a **6,6 µs** przy ciepłym (6,97 s / 1 048 576), czyli ~14× mniej. Te 6,6 µs to głównie
MD5 bloku 4 KiB oraz wywołanie systemowe i skopiowanie 4 KiB z pamięci. Czekanie na dysk (~88 µs na odczyt)
znika całkowicie. Dlatego przy ciepłym cache wątki nic nie dają – nie ma już czekania, które mogłyby ukryć.

---

## 4. Przepustowość – mechanizm zero-copy (`throughput` → `throughput_mmap`)
Wywołanie: `throughput_mmap <plik> <M>`.

### Wyniki (czas w sekundach, minimum z 3)

| M | 4G cold `read` | 4G cold `mmap` | 4G warm `read` | 4G warm `mmap` | 16G cold `read` | 16G cold `mmap` |
| --- | --- | --- | --- | --- | --- | --- |
| 512 B | 10,90 | **6,80** | 10,80 | **6,65** | 44,32 | **28,11** |
| 4 KiB | 6,78 | 6,25 | 6,64 | 6,02 | 27,06 | 24,83 |
| 64 KiB | 6,26 | 6,12 | 6,03 | 5,87 | 24,74 | 24,36 |
| 1 MiB | 6,18 | 6,10 | 6,02 | 5,86 | 24,39 | 24,26 |
| 16 MiB | 7,08 | 6,09 | 6,24 | 5,86 | 27,95 | 24,22 |

### Czas działania w funkcji M (przed i po poprawce)

- **`throughput` (read)** mocno zależy od M. Przy 512 B wykonuje 8,4 mln (4G) / 33,5 mln (16G) wywołań systemowych
  i jest **~1,8× wolniejszy** niż przy dużych blokach. Od 64 KiB czas już się nie zmienia.
- **`throughput_mmap`** prawie nie zależy od M (4G warm: 5,86–6,65 s), bo nie wywołuje systemu dla każdego bloku –
  jądro wczytuje dane stronami, niezależnie od rozmiaru bloku. Przy M = 512 zostaje tylko dodatkowy koszt MD5
  dla bardzo małych bloków (MD5 zawsze dolicza do danych blok dopełnienia).
- **Największy zysk z mmap** jest przy M = 512: czas krótszy o 37–39% (4G warm: 10,80 → 6,65 s;
  16G: 44,32 → 28,11 s). Dla 64 KiB–1 MiB różnica to tylko 0,5–3%.
- **M = 16 MiB**: `read` znów zwalnia (4G cold 7,08 s, 16G 27,95 s), a `mmap` nie. Bufor 16 MiB nie mieści się
  w pamięci podręcznej procesora L3 (9 MiB), więc dane są najpierw kopiowane do RAM, a potem MD5 czyta je z RAM
  jeszcze raz. Widać to także przy ciepłym cache (6,24 vs 5,86 s). Przy zimnym cache dochodzi to, że każde `read`
  czeka na całe 16 MiB z dysku i w tym czasie nic nie jest liczone.

### Czy jest schemat odczytu, w którym mmap zmniejszy obciążenie procesora bardziej niż read?

**Tak – gdy czytamy dużo małych kawałków danych, które są już w pamięci** (małe bloki, wielokrotne czytanie tych
samych danych). Przy `read` każdy blok to wejście do jądra i skopiowanie danych do bufora programu. Przy `mmap`
jądro działa tylko raz na stronę, a w praktyce raz na kilkanaście stron, bo od razu podpina też sąsiednie
(*fault-around*).

Przy dużych blokach (≥ 64 KiB) koszt `read` rozkłada się na dużo danych i obie metody dają prawie ten sam czas.
Tak samo jest przy czytaniu z dysku (`16G.bin`, zimny cache): dla M = 64 KiB–1 MiB różnica jest mniejsza niż 2%
(1 MiB: 24,39 vs 24,26 s), bo brakującą stronę i tak trzeba przeczytać z dysku.

### Jak na latencję wpływa posiadanie pliku w pamięci (file cache)?

Przy czytaniu **po kolei** na NVMe – **niewiele**: dla `4G.bin` zimny i ciepły cache różnią się o 1–4%
(np. M = 1 MiB: 6,18 vs 6,02 s dla `read`, 6,10 vs 5,86 s dla `mmap`). Powody:
1. jądro widzi, że plik jest czytany po kolei, i wczytuje kolejne fragmenty z wyprzedzeniem (*readahead*).
   Gdy program o nie prosi, zwykle już są w pamięci, więc nie czeka na dysk;
2. dysk czyta ~2,4 GB/s (osobny pomiar `dd` na `16G.bin`: 16 GiB w 7,3 s), a MD5 liczy ~0,7 GB/s.
   Program czeka więc na obliczenia, a nie na dysk.

Wyraźna różnica jest tylko dla `read` z M = 16 MiB (7,08 vs 6,24 s), gdzie czytanie nie odbywa się równolegle
z liczeniem. Dla porównania: przy czytaniu losowym (rozdział 3) cache skraca czas ~14×.

---

## 5. Przeplot obliczeń i I/O – pipelining (`pipeline` → `pipeline_mt`)

**Poprawka.** Dwa wątki: jeden czyta plik po kolei (`pread` do nowych buforów), drugi liczy (`stretch_md5` K razy + XOR).
Łączy je kanał `sync_channel(4)`, więc w pamięci jest najwyżej ~6 bloków, niezależnie od rozmiaru pliku.
Wywołanie: `pipeline_mt <plik> <M> <K>`.

### Wyniki (czas w sekundach, minimum z 3; `pipeline` / `pipeline_mt`)

| Plik, cache, M | K = 1 | K = 10 | K = 100 | K = 1000 | K = 10000 |
| --- | --- | --- | --- | --- | --- |
| 4G cold, 64 KiB | 6,20 / 5,82 | 6,25 / 5,88 | 6,81 / 6,48 | 12,24 / 11,99 | – |
| 4G cold, 1 MiB | 6,32 / 5,83 | 6,21 / 5,83 | 6,25 / 5,88 | 6,59 / 6,23 | 9,92 / 9,57 |
| 4G cold, 16 MiB | 7,08 / 5,88 | 7,09 / 5,88 | 7,11 / 5,89 | 7,12 / 5,90 | 7,34 / 6,11 |
| 4G warm, 64 KiB | 6,05 / 5,86 | 6,10 / 5,93 | 6,63 / 6,48 | 11,83 / 11,82 | – |
| 4G warm, 1 MiB | 5,95 / 5,77 | 5,95 / 5,79 | 5,99 / 5,82 | 6,33 / 6,16 | 9,63 / 9,52 |
| 4G warm, 16 MiB | 6,21 / 5,79 | 6,21 / 5,80 | 6,21 / 5,80 | 6,23 / 5,82 | 6,44 / 6,02 |
| 16G cold, 64 KiB | 24,85 / 23,33 | 25,02 / 23,53 | 27,12 / 25,67 | – | – |
| 16G cold, 1 MiB | 24,86 / 23,32 | 24,83 / 23,30 | 24,94 / 23,48 | 26,29 / 24,83 | 39,74 / 38,28 |
| 16G cold, 16 MiB | 28,59 / 23,47 | 28,29 / 23,47 | 28,29 / 24,09 | 29,16 / 23,58 | 29,24 / 24,42 |

### Czas działania w funkcji M i K

- **K** dodaje (K − 1) krótkich MD5 (z 16 bajtów) na każdy blok. Z wyników `4G warm, 1 MiB`:
  (9,52 − 5,77) s / (4096 bloków · 9999) ≈ **92 ns na jedno dodatkowe MD5**. Pierwsze MD5, z całego bloku 1 MiB,
  trwa 5,77 s / 4096 ≈ **1,41 ms**.
- **M** zmienia liczbę bloków, a więc i liczbę dodatkowych MD5. Dla 64 KiB bloków jest 16× więcej niż dla 1 MiB,
  więc czas rośnie już przy K = 100 (4G: 6,48 s), a przy K = 1000 się podwaja (12 s). Dla 16 MiB nawet
  K = 10000 dodaje tylko ~0,2 s (4G) / ~0,9 s (16G).
- **`pipeline_mt` jest zawsze szybszy**, ale na NVMe zysk jest mały: 0–8% dla M ≤ 1 MiB, tym mniejszy, im większe K,
  bo coraz większą część czasu zajmuje liczenie (16G, 1 MiB, K = 1: 24,86 → 23,32 s). Dla M = 16 MiB przy zimnym
  cache zysk to **15–19%** (16G: 28,59 → 23,47 s; 4G cold: 7,08 → 5,88 s).

W teorii czas powinien się zmienić z „czytanie + liczenie” na „dłuższe z dwóch”. Dla `16G.bin` samo czytanie
trwa ~7,3 s, a liczenie ~23,3 s, czyli suma to ~30,6 s, a dłuższe z nich ~23,3 s. `pipeline_mt` osiąga **23,3 s**,
czyli tyle, ile trwa samo liczenie. Oryginał (24,9 s) jest jednak dużo bliżej 23,3 s niż 30,6 s, bo
**jądro już w wersji jednowątkowej czyta z wyprzedzeniem**: gdy program liczy MD5, jądro w tle wczytuje kolejne
fragmenty pliku. Wersja z dwoma wątkami oszczędza już tylko koszt wywołania `read` i kopiowania.
Przy M = 16 MiB jeden blok jest dużo większy niż to, co jądro wczytuje z wyprzedzeniem (128 KiB), więc oryginał
czeka na każde 16 MiB z dysku – i tu `pipeline_mt` daje największy zysk.

### K_opt dla M = 1 MiB

Program nie zwalnia, dopóki liczenie jednego bloku trwa krócej niż jego czytanie:
1,41 ms + (K − 1) · 92 ns ≤ czas odczytu bloku.

| Przypadek | Czas odczytu bloku 1 MiB | K_opt |
| --- | --- | --- |
| Plik w RAM (`4G warm`) | ~0,04 ms (`read` + kopia z page cache: (6,02 − 5,86) s / 4096, różnica `throughput` i `throughput_mmap`) | **1** |
| Plik czytany z dysku NVMe (`16G cold`) | ~0,45 ms (2,4 GB/s) | **1** |

**Na tym komputerze dysk NVMe jest szybszy niż MD5 już przy K = 1.** Założenie z treści zadania („dla małych K
dysk jest wolniejszy niż obliczanie funkcji skrótu”) tu nie zachodzi: od początku czekamy na obliczenia, więc
dokładnie K_opt = 1. Potwierdzają to dane – zimny i ciepły cache dają prawie ten sam czas
(1 MiB: 5,83 vs 5,77 s przy K = 1; 9,57 vs 9,52 s przy K = 10000).

W praktyce dodatkowe MD5 są tak tanie, że do **K ≈ 100** spowolnienie jest poniżej 1%, czyli w granicach szumu
(16G 23,32 → 23,48 s; 4G warm 5,77 → 5,82 s). Od K = 1000 jest już wyraźne (+6,5%), a przy K = 10000 wynosi +64%.

### Czy dodanie większej liczby wątków po stronie czytania lub funkcji hashującej zmieni cokolwiek?

Odpowiedź zależy od tego, która część jest wolniejsza. (Takiego wariantu nie zbudowaliśmy – wnioski wynikają
z pomiarów powyżej.)

- **Więcej wątków czytających: nie.** Wątek czytający i tak czeka, aż zwolni się miejsce w kanale, bo liczenie
  jest ~3× wolniejsze od dysku.
- **Więcej wątków liczących: tak.** Jeden wątek liczy ~0,7 GB/s, a dysk daje ~2,4 GB/s. 3–4 wątki liczące
  (procesor ma 6 rdzeni) mogłyby skrócić czas dla `16G.bin` z ~23 s do ok. 7–8 s, czyli do granicy dysku.
  Przy dużym K zysk byłby jeszcze większy. Bloki można liczyć niezależnie, bo wynik XOR nie zależy od kolejności.

---

## 6. Podsumowanie

1. **Latencja (losowy odczyt)**: wiele wątków `pread` skraca czas nawet 10× (4G, M = 4 KiB, zimny cache: 99,5 → 9,9 s),
   bo dysk NVMe dostaje wiele odczytów naraz. Rośnie liczba odczytów na sekundę, ale pojedynczy odczyt nie jest
   szybszy – od ~8 wątków jest nawet wolniejszy, bo czeka w kolejce. Gdy plik jest w cache, wątki nie pomagają.
2. **Przepustowość (mmap)**: brak kopiowania daje największy zysk przy małych blokach (M = 512: czas krótszy
   o 37–39%). Przy dużych blokach różnicy prawie nie ma.
3. **Pipelining**: na NVMe program czeka na MD5, a nie na dysk, więc K_opt = 1. Zysk z dwóch wątków jest mały (0–8%),
   bo jądro już samo czyta z wyprzedzeniem. Duży zysk (15–19%) jest tylko dla bloków większych niż to wyprzedzenie.
   Przyspieszyć mogłyby tylko dodatkowe wątki liczące.
4. **Wspólny wniosek**: na szybkim NVMe przy czytaniu po kolei najwolniejszy jest procesor. Dysk jest problemem tylko
   przy losowym czytaniu małych bloków. Optymalizacje odczytu opłacają się tylko tam, gdzie to odczyt jest
   najwolniejszą częścią.

## 7. Ograniczenia

- Wszystkie pomiary na jednym dysku (NVMe Samsung 970 EVO).
- Nie używaliśmy `O_DIRECT`. Cache czyścimy przez `drop_caches`, więc jądro nadal czyta z wyprzedzeniem.