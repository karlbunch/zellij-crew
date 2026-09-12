# Two crates:
#   plugin/  package zellij-crew-plugin -> wasm32-wasip1, installed as zellij-crew.wasm
#   cli/     package zellij-crew-cli    -> *-unknown-linux-musl, static zellij-crew binary
#
# The musl CLI build links libcurl via the vendored_curl feature, so it needs a C
# toolchain: `sudo apt install musl-tools` (Debian/Ubuntu) or the musl package on Void.

WASM_TARGET = wasm32-wasip1
MUSL_TARGET = x86_64-unknown-linux-musl
ARM_TARGET  = aarch64-unknown-linux-musl

WASM_BIN = target/$(WASM_TARGET)/release/zellij-crew-plugin.wasm
CLI_BIN  = target/$(MUSL_TARGET)/release/zellij-crew

CONFIG_DIR = $(HOME)/.config/zellij
BIN_DIR    = $(HOME)/.local/bin
PLUGIN     = $(CONFIG_DIR)/zellij-crew.wasm
PERMS      = $(HOME)/.cache/zellij/permissions.kdl

# For `reload`: must equal the `names` child on the load_plugins entry in config.kdl
# (start-or-reload-plugin only reaches an instance with an identical configuration).
NAMES ?=

.PHONY: build build-plugin build-cli install install-plugin install-cli install-permissions cross reload clean

build: build-plugin build-cli

build-plugin:
	cargo build --release --target $(WASM_TARGET) -p zellij-crew-plugin

build-cli:
	cargo build --release --target $(MUSL_TARGET) -p zellij-crew-cli

# The CLI is being rewritten; until it lands, `install` ships only the plugin.
install: install-plugin

install-plugin: build-plugin
	@mkdir -p $(CONFIG_DIR)
	cp $(WASM_BIN) $(PLUGIN)

install-cli: build-cli
	@mkdir -p $(BIN_DIR)
	install -m 755 $(CLI_BIN) $(BIN_DIR)/zellij-crew

# Grant the daemon's two permissions ahead of time so no session ever shows the
# prompt (closing the prompt pane would unload the daemon for that session). The
# key is the path as zellij resolves "file:~/.config/zellij/zellij-crew.wasm".
install-permissions:
	@mkdir -p $(dir $(PERMS))
	@if grep -qsF '"$(PLUGIN)"' $(PERMS); then echo "already granted: $(PLUGIN)"; else \
	    printf '"%s" {\n    ReadApplicationState\n    ChangeApplicationState\n}\n' "$(PLUGIN)" >> $(PERMS); \
	    echo "granted: $(PLUGIN)"; fi

cross:
	cross build --release --target $(ARM_TARGET) -p zellij-crew-cli

# Dev loop: reinstall and hot-reload the daemon in the running session. Pass
# NAMES="..." matching config.kdl, or zellij starts a second, visible instance.
reload: install-plugin
	zellij action start-or-reload-plugin $(if $(NAMES),-c "names=$(NAMES)",) "file:$(PLUGIN)"

clean:
	cargo clean
