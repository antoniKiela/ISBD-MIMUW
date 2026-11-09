CARGO_FLAGS = --release
BIN = target/release

.PHONY: all test sample clean

# Buduje bibliotekę i programy: target/release/bgs_gen, target/release/bgs_metrics
all:
	cargo build $(CARGO_FLAGS)

# Testy jednostkowe (w modułach src/) i end-to-end (tests/)
test:
	cargo test $(CARGO_FLAGS)

# Regeneruje przykładowy plik (1×INT64 + 1×BOOLEAN + 2×VARCHAR) i oczekiwany wynik metryk
sample: all
	$(BIN)/bgs_gen sample/sample.bgs 20000 42
	$(BIN)/bgs_metrics sample/sample.bgs > sample/sample_metrics.txt

clean:
	cargo clean
