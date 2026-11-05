CARGO_FLAGS = --release
BIN = target/release

.PHONY: all test sample clean

# Buduje bibliotekę i programy: target/release/isbd_gen, target/release/isbd_metrics
all:
	cargo build $(CARGO_FLAGS)

# Testy jednostkowe (w modułach src/) i end-to-end (tests/)
test:
	cargo test $(CARGO_FLAGS)

# Regeneruje przykładowy plik (2×INT64 + 2×VARCHAR) i oczekiwany wynik metryk
sample: all
	$(BIN)/isbd_gen sample/sample.bgs 20000 42 8192
	$(BIN)/isbd_metrics sample/sample.bgs > sample/sample_metrics.txt

clean:
	cargo clean
