# Two crates:
#   plugin/  -> wasm32-wasip1        headless naming daemon
#   cli/     -> *-unknown-linux-musl  zellij-crew messaging CLI (static)
#
# The musl CLI build links libcurl via the vendored_curl feature, so it needs a C
# toolchain: `sudo apt install musl-tools` (Debian/Ubuntu) or the musl package on Void.

WASM_TARGET = wasm32-wasip1
MUSL_TARGET = x86_64-unknown-linux-musl
ARM_TARGET  = aarch64-unknown-linux-musl

WASM_BIN = target/$(WASM_TARGET)/release/zellij-crew.wasm
CLI_BIN  = target/$(MUSL_TARGET)/release/zellij-crew

CONFIG_DIR = $(HOME)/.config/zellij
BIN_DIR    = $(HOME)/.local/bin
PLUGIN_URL = file://$(CONFIG_DIR)/zellij-crew.wasm

.PHONY: build build-plugin build-cli install install-plugin install-cli cross reload clean

build: build-plugin build-cli

build-plugin:
	cargo build --release --target $(WASM_TARGET) -p zellij-crew

build-cli:
	cargo build --release --target $(MUSL_TARGET) -p zellij-crew-cli

install: install-plugin install-cli

install-plugin: build-plugin
	@mkdir -p $(CONFIG_DIR)
	cp $(WASM_BIN) $(CONFIG_DIR)/

install-cli: build-cli
	@mkdir -p $(BIN_DIR)
	install -m 755 $(CLI_BIN) $(BIN_DIR)/zellij-crew

# Cross-build the CLI for aarch64 musl (needs `cargo install cross` + docker/podman).
cross:
	cross build --release --target $(ARM_TARGET) -p zellij-crew-cli

# Reinstall the plugin and hot-reload it in the running session (dev loop).
reload: install-plugin
	zellij action start-or-reload-plugin "$(PLUGIN_URL)"

clean:
	cargo clean
