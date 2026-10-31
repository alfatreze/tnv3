CC ?= cc
CFLAGS ?= -O3
LDLIBS ?= -lm

.PHONY: all experimental test clean
all: tools/tvc_ref/tvc_enc tools/tvc_ref/tvc_dec tools/tac_ref/tac tools/tac_ref/tac_fx tools/tnv3_ref/tnv3

tools/tvc_ref/tvc_enc: tools/tvc_ref/tvc_enc.c tools/tvc_ref/tvc.h $(wildcard tools/tvc_ref/enc/*.h)
	$(CC) $(CFLAGS) -o $@ $< $(LDLIBS)
tools/tvc_ref/tvc_dec: tools/tvc_ref/tvc_dec.c tools/tvc_ref/tvc.h
	$(CC) $(CFLAGS) -o $@ $< $(LDLIBS)
tools/tac_ref/tac: tools/tac_ref/tac.c tools/tvc_ref/tvc.h
	$(CC) $(CFLAGS) -o $@ $< $(LDLIBS)
tools/tac_ref/tac_fx: tools/tac_ref/tac_fx.c tools/tac_ref/tac.c tools/tvc_ref/tvc.h
	$(CC) $(CFLAGS) -o $@ $< $(LDLIBS)
tools/tnv3_ref/tnv3: tools/tnv3_ref/tnv3.c
	$(CC) $(CFLAGS) -o $@ $< $(LDLIBS)
experimental: tools/tvc_ref/tvc_enc_slice_experimental
tools/tvc_ref/tvc_enc_slice_experimental: tools/tvc_ref/tvc_enc_slice_experimental.c tools/tvc_ref/tvc.h $(wildcard tools/tvc_ref/enc/*.h)
	$(CC) $(CFLAGS) -pthread -o $@ $< $(LDLIBS)
test: all experimental
	python3 tests/check_manifest.py
	python3 tests/smoke.py
clean:
	rm -f tools/tvc_ref/tvc_enc tools/tvc_ref/tvc_dec tools/tvc_ref/tvc_enc_slice_experimental tools/tac_ref/tac tools/tac_ref/tac_fx tools/tnv3_ref/tnv3

# The Rust library and CLI are built separately from the C reference tools.
.PHONY: rust rust-test
rust:
	cargo build --release --manifest-path tau-video/Cargo.toml --features cli,mp3-cli
rust-test:
	cargo test --manifest-path tau-video/Cargo.toml --all-features

# Opt-in block-motion experiment; the normal rust target stays unchanged.
.PHONY: rust-experimental
rust-experimental:
	cargo build --release --manifest-path tau-video/Cargo.toml --features cli,mp3-cli,block-motion,b-pictures
