# macOS: the Command Line Tools SDK on some machines fails to link ("unknown architecture arm64e");
# fall back to Xcode's SDK when SDKROOT is unset and Xcode is installed.
ifeq ($(shell uname -s),Darwin)
ifndef SDKROOT
XCODE_SDK := /Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk
ifneq ($(wildcard $(XCODE_SDK)),)
export SDKROOT := $(XCODE_SDK)
endif
endif
endif
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
	@echo "Quick run: reference tests without their tools are SKIPPED (TAU_ALLOW_SKIP=1). Use 'make rust-conformance' for the real check."
	TAU_ALLOW_SKIP=1 cargo test --manifest-path tau-video/Cargo.toml --all-features

# Opt-in block-motion experiment; the normal rust target stays unchanged.
.PHONY: rust-experimental
rust-experimental:
	cargo build --release --manifest-path tau-video/Cargo.toml --features cli,mp3-cli,block-motion,b-pictures

# Conservative per-picture rate budget; opt-in while quality is evaluated.
.PHONY: rust-safe-rate
rust-safe-rate:
	cargo build --release --manifest-path tau-video/Cargo.toml --features cli,mp3-cli,safe-rate

# Conformance run against the pinned Tau V vectors (read in place; see tests/pins). Unlike `rust-test`,
# this sets the variables that otherwise make the reference tests return early without checking anything.
TAU_V_ROOT ?= $(CURDIR)/../tau-nu/.claude/worktrees/codec-alternatives
# Our own draft 1.5/1.6 vectors: regenerate, check the C checker against the spec-derived expectations, then the Rust checker.
.PHONY: vectors-15-16
vectors-15-16: all
	python3 -I tests/vectors/make_vectors_15_16.py --verify
	git diff --exit-code -- tests/vectors/tnv3_15_16
	cargo test --manifest-path tau-video-cli/Cargo.toml --test vectors_15_16

# One environment for every package's reference tests (see tests/reference_env.rs in each package).
FFMPEG ?= $(shell command -v ffmpeg)
FFPROBE ?= $(shell command -v ffprobe)
TEST_ENV = TAU_TVC_VECTOR_DIR="$(TAU_V_ROOT)/tools/tvc_ref/vectors" TAU_VIDEO_VECTOR_DIR="$(TAU_V_ROOT)/tools/tnv3_ref/vectors" \
	TAU_TVC_DECODER="$(CURDIR)/tools/tvc_ref/tvc_dec" TAU_TNV3_TOOL="$(CURDIR)/tools/tnv3_ref/tnv3" \
	TAU_TEST_TNV3="$(CURDIR)/tools/tnv3_ref/tnv3" TAU_TEST_PYTHON=python3 \
	TAU_TEST_FFMPEG="$(FFMPEG)" TAU_TEST_FFPROBE="$(FFPROBE)" TAU_TEST_TVC="$(CURDIR)/tools/tvc_ref/tvc_enc" \
	TAU_TEST_C_SCRIPT="$(CURDIR)/tools/tnv3_ref/encode_film.py" \
	TAU_TEST_TVC_ENC="$(CURDIR)/tools/tvc_ref/tvc_enc" TAU_TEST_TAC="$(CURDIR)/tools/tac_ref/tac" \
	TAU_TEST_FONT="$(TAU_V_ROOT)/tools/tst_ref/fonts/Roboto-Regular.ttf"

# Pixel comparison of our subtitle reader/renderer with Tau V's tst_render.py (needs Python 3 with Pillow).
.PHONY: compare-render
compare-render:
	cargo build --manifest-path tau-video-cli/Cargo.toml
	python3 -I tests/compare_tst_render.py --tau-v "$(TAU_V_ROOT)" --cli tau-video-cli/target/debug/tau-codecs-cli

.PHONY: rust-conformance
rust-conformance: all
	python3 -I tests/pins/check_pins.py --root "$(TAU_V_ROOT)" --skip-films
	python3 -I tests/pins/check_tac_vectors.py
	python3 -I tests/pins/check_tac_trim.py
	$(TEST_ENV) cargo test --manifest-path tau-video/Cargo.toml --all-features
	$(TEST_ENV) cargo test --manifest-path tau-video-cli/Cargo.toml
	$(TEST_ENV) cargo test --manifest-path tau-encoder/src-tauri/Cargo.toml
