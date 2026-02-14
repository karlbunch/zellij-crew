WASM_TARGET = wasm32-wasip1
WASM_BIN = target/$(WASM_TARGET)/release/zellij-crew.wasm
CLI_BIN = target/release/zellij-crew
INSTALL_DIR = $(HOME)/.config/zellij
PLUGIN_URL = file://$(INSTALL_DIR)/zellij-crew.wasm

.PHONY: build build-plugin build-cli install setup reload clean

build: build-plugin build-cli

build-plugin:
	cargo build --target $(WASM_TARGET) --release -p zellij-crew

build-cli:
	cargo build --release -p zellij-crew-cli

install: build
	@mkdir -p $(INSTALL_DIR) $(HOME)/.local/bin
	cp $(WASM_BIN) $(INSTALL_DIR)/
	cp $(CLI_BIN) $(INSTALL_DIR)/
	chmod +x $(INSTALL_DIR)/zellij-crew
	ln -sf $(INSTALL_DIR)/zellij-crew $(HOME)/.local/bin/zellij-crew

setup: install
	$(INSTALL_DIR)/zellij-crew --setup

reload: install
	zellij action start-or-reload-plugin "$(PLUGIN_URL)"

clean:
	cargo clean
