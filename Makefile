# Rusty Jack — local build helpers
#
# Make does not load your shell profile, so we prepend ~/.cargo/bin to PATH.
# If cargo is still missing, run:  curl -sSf https://sh.rustup.rs | sh
# then:  source "$HOME/.cargo/env"

.PHONY: all build build-release check-cargo clean clippy driver-bundle fmt install list list-hdmi package release test uninstall universal upgrade validate-driver-bundle

export MACOSX_DEPLOYMENT_TARGET ?= 12.0
export PATH := $(HOME)/.cargo/bin:$(PATH)
export CARGO_BUILD_JOBS ?= $(shell sysctl -n hw.logicalcpu 2>/dev/null || echo 4)

CARGO ?= cargo
INSTALL_BIN_DIR ?= $(HOME)/.cargo/bin
INSTALL_SHARE_DIR ?= $(HOME)/.cargo/share/rusty-jack
BIN_NAME := rusty-jack
RELEASE_BIN := target/release/$(BIN_NAME)
INSTALLED_BIN := $(INSTALL_BIN_DIR)/$(BIN_NAME)

# Keep make from invoking cargo when nothing changed.
# Use git to enumerate tracked Rust sources (fast + includes new files when added to git).
RUST_SOURCES := $(shell git ls-files '*.rs' 2>/dev/null)
RUST_BUILD_INPUTS := Cargo.toml Cargo.lock $(RUST_SOURCES)

DRIVER_BUNDLE_OUTPUT ?= target/share/rusty-jack
DRIVER_BUNDLE := $(DRIVER_BUNDLE_OUTPUT)/RustyJack.driver
DRIVER_BUNDLE_STAMP := $(DRIVER_BUNDLE)/.built
DRIVER_BUNDLE_SOURCES := \
	Cargo.toml \
	driver/RustyJack/Info.plist.in \
	driver/RustyJack/RustyJackAudioServerPlugIn.c \
	driver/RustyJack/passthrough_ring.c \
	driver/RustyJack/passthrough_ring.h \
	scripts/build-driver-bundle

all: test build

build: check-cargo
	$(CARGO) build

build-release: $(RELEASE_BIN)

$(RELEASE_BIN): $(RUST_BUILD_INPUTS)
	@$(MAKE) check-cargo
	$(CARGO) build --release

check-cargo:
	@command -v $(CARGO) >/dev/null 2>&1 || { \
		printf '\nerror: cargo not found (looked in PATH and $$HOME/.cargo/bin)\n\n'; \
		printf 'Install Rust:\n  curl --proto '"'"'=https'"'"' --tlsv1.2 -sSf https://sh.rustup.rs | sh\n\n'; \
		printf 'Then load it in this shell:\n  source "$$HOME/.cargo/env"\n\n'; \
		printf 'Or build without make:\n  ~/.cargo/bin/cargo build --release\n\n'; \
		exit 1; \
	}

clean: check-cargo
	$(CARGO) clean

clippy: check-cargo
	$(CARGO) clippy --all-targets -- -D warnings

driver-bundle: $(DRIVER_BUNDLE_STAMP)

$(DRIVER_BUNDLE_STAMP): $(DRIVER_BUNDLE_SOURCES)
	./scripts/build-driver-bundle "$(DRIVER_BUNDLE_OUTPUT)"
	@touch "$@"

fmt: check-cargo
	$(CARGO) fmt --all

install: check-cargo
	@mkdir -p "$(INSTALL_BIN_DIR)"
	@mkdir -p "$(INSTALL_SHARE_DIR)"
	@$(MAKE) build-release
	@if [ -f "$(INSTALLED_BIN)" ] && cmp -s "$(RELEASE_BIN)" "$(INSTALLED_BIN)"; then \
		echo "rusty-jack already installed: $(INSTALLED_BIN)"; \
	else \
		echo "Installing rusty-jack to $(INSTALLED_BIN)"; \
		install -m 755 "$(RELEASE_BIN)" "$(INSTALLED_BIN)"; \
	fi
	@$(MAKE) driver-bundle DRIVER_BUNDLE_OUTPUT="$(INSTALL_SHARE_DIR)"

list: build
	$(CARGO) run -- list

list-hdmi: build
	$(CARGO) run -- list --hdmi

package: driver-bundle release

release: check-cargo
	$(CARGO) build --release

test: check-cargo
	$(CARGO) test --all-targets

uninstall: check-cargo
	-@command -v rusty-jack >/dev/null 2>&1 && rusty-jack uninstall || true
	$(CARGO) uninstall rusty-jack || true

universal: check-cargo
	./scripts/build-universal

upgrade: install
	rusty-jack upgrade --force

validate-driver-bundle: $(DRIVER_BUNDLE_STAMP) scripts/validate-driver-bundle
	./scripts/validate-driver-bundle "$(DRIVER_BUNDLE)"
