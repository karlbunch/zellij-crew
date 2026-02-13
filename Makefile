WASM_TARGET = wasm32-wasip1
WASM_BIN = target/$(WASM_TARGET)/release/zellij-crew.wasm
INSTALL_DIR = $(HOME)/.config/zellij
PLUGIN_URL = file://$(INSTALL_DIR)/zellij-crew.wasm

.PHONY: build install reload clean

build:
	cargo build --target $(WASM_TARGET) --release

install: build
	@mkdir -p $(INSTALL_DIR)
	cp $(WASM_BIN) $(INSTALL_DIR)/

reload: install
	zellij action start-or-reload-plugin "$(PLUGIN_URL)"

clean:
	cargo clean
