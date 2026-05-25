# Rusty Jack — local build helpers
#
# Make does not load your shell profile, so we prepend ~/.cargo/bin to PATH.
# If cargo is still missing, run:  curl -sSf https://sh.rustup.rs | sh
# then:  source "$HOME/.cargo/env"

.PHONY: all build release test fmt clippy universal clean install uninstall upgrade check-cargo

export MACOSX_DEPLOYMENT_TARGET ?= 12.0
export PATH := $(HOME)/.cargo/bin:$(PATH)
export CARGO_BUILD_JOBS ?= $(shell sysctl -n hw.logicalcpu 2>/dev/null || echo 4)

CARGO ?= cargo

check-cargo:
	@command -v $(CARGO) >/dev/null 2>&1 || { \
		printf '\nerror: cargo not found (looked in PATH and $$HOME/.cargo/bin)\n\n'; \
		printf 'Install Rust:\n  curl --proto '"'"'=https'"'"' --tlsv1.2 -sSf https://sh.rustup.rs | sh\n\n'; \
		printf 'Then load it in this shell:\n  source "$$HOME/.cargo/env"\n\n'; \
		printf 'Or build without make:\n  ~/.cargo/bin/cargo build --release\n\n'; \
		exit 1; \
	}

all: test build

build: check-cargo
	$(CARGO) build

release: check-cargo
	$(CARGO) build --release

test: check-cargo
	$(CARGO) test --all-targets

fmt: check-cargo
	$(CARGO) fmt --all

clippy: check-cargo
	$(CARGO) clippy --all-targets -- -D warnings

universal: check-cargo
	./scripts/build-universal

install: check-cargo
	$(CARGO) install --path . --force --locked --target-dir target

upgrade: install
	rusty-jack upgrade

uninstall: check-cargo
	-@command -v rusty-jack >/dev/null 2>&1 && rusty-jack uninstall || true
	$(CARGO) uninstall rusty-jack || true

clean: check-cargo
	$(CARGO) clean

list: build
	$(CARGO) run -- list

list-hdmi: build
	$(CARGO) run -- list --hdmi
