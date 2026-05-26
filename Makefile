# Rusty Jack — local build helpers
#
# Make does not load your shell profile, so we prepend ~/.cargo/bin to PATH.
# If cargo is still missing, run:  curl -sSf https://sh.rustup.rs | sh
# then:  source "$HOME/.cargo/env"

.PHONY: all build check-cargo clean clippy driver-bundle fmt install list list-hdmi package release test uninstall universal upgrade validate-driver-bundle

export MACOSX_DEPLOYMENT_TARGET ?= 12.0
export PATH := $(HOME)/.cargo/bin:$(PATH)
export CARGO_BUILD_JOBS ?= $(shell sysctl -n hw.logicalcpu 2>/dev/null || echo 4)

CARGO ?= cargo
DRIVER_BUNDLE_OUTPUT ?= target/share/rusty-jack
DRIVER_BUNDLE := $(DRIVER_BUNDLE_OUTPUT)/RustyJack.driver
DRIVER_BUNDLE_STAMP := $(DRIVER_BUNDLE)/.built
DRIVER_BUNDLE_SOURCES := \
	Cargo.toml \
	driver/RustyJack/Info.plist.in \
	driver/RustyJack/RustyJackAudioServerPlugIn.c \
	scripts/build-driver-bundle

all: test build

build: check-cargo
	$(CARGO) build

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
	$(CARGO) install --path . --force --locked --target-dir target
	$(MAKE) driver-bundle DRIVER_BUNDLE_OUTPUT="$$HOME/.cargo/share/rusty-jack"

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
	rusty-jack upgrade

validate-driver-bundle: $(DRIVER_BUNDLE_STAMP) scripts/validate-driver-bundle
	./scripts/validate-driver-bundle "$(DRIVER_BUNDLE)"
