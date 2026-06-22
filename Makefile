# Rusty Jack — local build helpers
#
# Make does not load your shell profile, so we prepend ~/.cargo/bin to PATH.
# If cargo is still missing, run:  curl -sSf https://sh.rustup.rs | sh
# then:  source "$HOME/.cargo/env"
#
# If the active rustup toolchain is incomplete, check-cargo offers to repair it.
# Non-interactive builds can pass REPAIR_RUST=1 to repair without prompting.

.PHONY: all build build-release check-cargo check-makefile clean clippy do-release driver-bundle fmt install list list-hdmi package publish-release release render-homebrew-formula sign-driver-bundle test uninstall universal update-release-pr upgrade validate-driver-bundle

export MACOSX_DEPLOYMENT_TARGET ?= 12.0
export PATH := $(HOME)/.cargo/bin:$(PATH)
export CARGO_BUILD_JOBS ?= $(shell sysctl -n hw.logicalcpu 2>/dev/null || echo 4)

CARGO ?= cargo
INSTALL_BIN_DIR ?= $(HOME)/.cargo/bin
INSTALL_SHARE_DIR ?= $(HOME)/.cargo/share/rusty-jack
BIN_NAME := rusty-jack
RELEASE_BIN := target/release/$(BIN_NAME)
INSTALLED_BIN := $(INSTALL_BIN_DIR)/$(BIN_NAME)
GIT_COMMIT_SHORT := $(shell git rev-parse --short HEAD 2>/dev/null || echo unknown)
export RUSTY_JACK_GIT_COMMIT := $(GIT_COMMIT_SHORT)

# Keep make from invoking cargo when nothing changed.
# Use git to enumerate tracked Rust sources (fast + includes new files when added to git).
RUST_SOURCES := $(shell git ls-files '*.rs' 2>/dev/null)
RUST_BUILD_INPUTS := Cargo.toml Cargo.lock build.rs $(RUST_SOURCES)
GIT_COMMIT_STAMP := target/.rusty-jack-git-commit

DRIVER_BUNDLE_OUTPUT ?= target/share/rusty-jack
DRIVER_BUNDLE := $(DRIVER_BUNDLE_OUTPUT)/RustyJack.driver
DRIVER_BUNDLE_STAMP := $(DRIVER_BUNDLE_OUTPUT)/.RustyJack.driver.stamp
DRIVER_BUNDLE_SOURCES := \
	Cargo.toml \
	driver/RustyJack/Info.plist.in \
	driver/RustyJack/RustyJackAudioServerPlugIn.c \
	driver/RustyJack/passthrough_ring.c \
	driver/RustyJack/passthrough_ring.h \
	scripts/build-driver-bundle

HOMEBREW_FORMULA_TEMPLATE := packaging/homebrew/rusty-jack.formula.in

# Top-level .PHONY entries and target rule blocks must stay in lexicographic order by target name.

$(DRIVER_BUNDLE_STAMP): $(DRIVER_BUNDLE_SOURCES)
	./scripts/build-driver-bundle "$(DRIVER_BUNDLE_OUTPUT)"
	@touch "$@"

$(GIT_COMMIT_STAMP):
	@mkdir -p target
	@git rev-parse --short HEAD > "$@"

$(RELEASE_BIN): $(RUST_BUILD_INPUTS) $(GIT_COMMIT_STAMP)
	@$(MAKE) check-cargo
	$(CARGO) build --release

all: test build

build: check-cargo
	$(CARGO) build

build-release: $(RELEASE_BIN)

check-cargo:
	@command -v $(CARGO) >/dev/null 2>&1 || { \
		printf '\nerror: cargo not found (looked in PATH and $$HOME/.cargo/bin)\n\n'; \
		printf 'Install Rust:\n  curl --proto '"'"'=https'"'"' --tlsv1.2 -sSf https://sh.rustup.rs | sh\n\n'; \
		printf 'Then load it in this shell:\n  source "$$HOME/.cargo/env"\n\n'; \
		printf 'Or build without make:\n  ~/.cargo/bin/cargo build --release\n\n'; \
		exit 1; \
	}
	@command -v rustc >/dev/null 2>&1 || { \
		printf '\nerror: rustc not found (looked in PATH and $$HOME/.cargo/bin)\n\n'; \
		printf 'Install Rust with rustup (see above) or ensure rustc is on PATH.\n\n'; \
		exit 1; \
	}
	@TARGET_LIBDIR=$$(rustc --print target-libdir 2>/dev/null); \
	HOST=$$(rustc -vV 2>/dev/null | sed -n 's/^host: //p'); \
	if [ -z "$$TARGET_LIBDIR" ] || ! ls "$$TARGET_LIBDIR"/libstd-*.rlib >/dev/null 2>&1; then \
		REPAIR=0; \
		printf '\nerror: Rust standard library is missing'; \
		if [ -n "$$HOST" ]; then printf ' for %s' "$$HOST"; fi; \
		printf '\n\n'; \
		printf 'This usually means a partial or interrupted rustup update left rust-std off disk.\n'; \
		if [ "$(REPAIR_RUST)" = "1" ]; then \
			REPAIR=1; \
		elif [ -t 0 ]; then \
			printf 'Repair the stable Rust toolchain now? [y/N] '; \
			read -r confirm; \
			case "$$confirm" in y|Y|yes|YES) REPAIR=1;; esac; \
		else \
			printf 'Re-run interactively to repair automatically, or run:\n'; \
			printf '  rustup toolchain reinstall stable\n'; \
			printf '  make install REPAIR_RUST=1\n\n'; \
			exit 1; \
		fi; \
		if [ "$$REPAIR" != "1" ]; then \
			printf 'Build cancelled.\n\n'; \
			exit 1; \
		fi; \
		command -v rustup >/dev/null 2>&1 || { \
			printf 'error: rustup not found; install rustup to repair the toolchain\n\n'; \
			exit 1; \
		}; \
		printf 'Repairing Rust toolchain (stable)...\n'; \
		rustup toolchain reinstall stable; \
		rustup component add rustfmt clippy 2>/dev/null || true; \
		TARGET_LIBDIR=$$(rustc --print target-libdir 2>/dev/null); \
		if [ -z "$$TARGET_LIBDIR" ] || ! ls "$$TARGET_LIBDIR"/libstd-*.rlib >/dev/null 2>&1; then \
			printf '\nerror: Rust toolchain repair failed; std library still missing\n\n'; \
			exit 1; \
		fi; \
		printf 'Rust toolchain repair complete.\n\n'; \
	fi

check-makefile:
	@./scripts/check-makefile-target-order

clean: check-cargo
	$(CARGO) clean

clippy: check-cargo
	$(CARGO) clippy --all-targets -- -D warnings

do-release:
	@chmod +x scripts/do-release scripts/update-release-pr scripts/publish-release scripts/release-lib
	@./scripts/do-release

driver-bundle: $(DRIVER_BUNDLE_STAMP)

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

publish-release:
	@chmod +x scripts/publish-release scripts/release-lib
	@./scripts/publish-release

release: check-cargo
	$(CARGO) build --release

render-homebrew-formula:
	@test -n '$(ARCHIVE_URL)' || { echo 'ARCHIVE_URL is required' >&2; exit 1; }
	@test -n '$(ARCHIVE_SHA256)' || { echo 'ARCHIVE_SHA256 is required' >&2; exit 1; }
	@test -n '$(GIT_COMMIT)' || GIT_COMMIT=$$(git rev-parse --short HEAD 2>/dev/null || echo unknown); \
	sed \
	  -e 's|@ARCHIVE_URL@|$(ARCHIVE_URL)|g' \
	  -e 's|@ARCHIVE_SHA256@|$(ARCHIVE_SHA256)|g' \
	  -e "s|@GIT_COMMIT@|$${GIT_COMMIT:-unknown}|g" \
	  '$(HOMEBREW_FORMULA_TEMPLATE)'

sign-driver-bundle: $(DRIVER_BUNDLE_STAMP) scripts/sign-driver-bundle
	chmod +x scripts/sign-driver-bundle
	./scripts/sign-driver-bundle "$(DRIVER_BUNDLE)"

test: check-cargo
	$(CARGO) test --all-targets

uninstall: check-cargo
	-@command -v rusty-jack >/dev/null 2>&1 && rusty-jack uninstall || true
	-@$(CARGO) uninstall rusty-jack 2>/dev/null || true
	@if [ -f "$(INSTALLED_BIN)" ] || [ -d "$(INSTALL_SHARE_DIR)" ]; then \
		if [ "$(YES)" != "1" ]; then \
			if [ ! -t 0 ]; then \
				echo "Refusing to remove installed files without confirmation."; \
				echo "Re-run with: make uninstall YES=1"; \
				exit 1; \
			fi; \
			echo "The following will be removed:"; \
			[ -f "$(INSTALLED_BIN)" ] && echo "  $(INSTALLED_BIN)"; \
			[ -d "$(INSTALL_SHARE_DIR)" ] && echo "  $(INSTALL_SHARE_DIR)"; \
			printf "Continue? [y/N] "; \
			read -r confirm; \
			case "$$confirm" in y|Y|yes|YES) ;; *) echo "Uninstall cancelled."; exit 0;; esac; \
		fi; \
		if [ -f "$(INSTALLED_BIN)" ]; then \
			echo "Removing $(INSTALLED_BIN)"; \
			rm -f "$(INSTALLED_BIN)"; \
		fi; \
		if [ -d "$(INSTALL_SHARE_DIR)" ]; then \
			echo "Removing $(INSTALL_SHARE_DIR)"; \
			rm -rf "$(INSTALL_SHARE_DIR)"; \
		fi; \
	fi

universal: check-cargo
	./scripts/build-universal

update-release-pr:
	@chmod +x scripts/update-release-pr scripts/release-lib
	@./scripts/update-release-pr

upgrade: check-cargo
	@mkdir -p "$(INSTALL_BIN_DIR)"
	@mkdir -p "$(INSTALL_SHARE_DIR)"
	@$(MAKE) build-release
	@PREV_VER=""; \
	if [ -f "$(INSTALLED_BIN)" ]; then \
		PREV_VER=$$("$(INSTALLED_BIN)" --version 2>/dev/null | tr -d '\n'); \
	fi; \
	if [ -f "$(INSTALLED_BIN)" ] && cmp -s "$(RELEASE_BIN)" "$(INSTALLED_BIN)"; then \
		echo "rusty-jack already installed: $(INSTALLED_BIN)"; \
	else \
		echo "Installing rusty-jack to $(INSTALLED_BIN)"; \
		install -m 755 "$(RELEASE_BIN)" "$(INSTALLED_BIN)"; \
	fi; \
	$(MAKE) driver-bundle DRIVER_BUNDLE_OUTPUT="$(INSTALL_SHARE_DIR)"; \
	RUSTY_JACK_UPGRADE_PREVIOUS_VERSION="$$PREV_VER" rusty-jack upgrade --force

validate-driver-bundle: $(DRIVER_BUNDLE_STAMP) scripts/validate-driver-bundle
	./scripts/validate-driver-bundle "$(DRIVER_BUNDLE)"
