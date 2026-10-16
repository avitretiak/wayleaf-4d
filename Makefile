APP_ID = io.github.avitretiak.Wayleaf4D
PLASMA_PACKAGE = share/plasma/$(APP_ID)
USER_SHARE = $(if $(filter /%,$(XDG_DATA_HOME)),$(XDG_DATA_HOME),$(HOME)/.local/share)
PLASMA_DIR = $(USER_SHARE)/plasma/plasmoids/$(APP_ID)
USER_ICON_DIR = $(USER_SHARE)/icons/hicolor/scalable/apps
USER_CONFIG = $(if $(filter /%,$(XDG_CONFIG_HOME)),$(XDG_CONFIG_HOME),$(HOME)/.config)
USER_BIN = $(HOME)/.local/bin
BUILD_DIR = build
CARGO_TARGET_DIR = $(BUILD_DIR)/cargo
WORKER = $(BUILD_DIR)/wayleaf-4d
DBUS_SERVICE = $(USER_SHARE)/dbus-1/services/io.github.avitretiak.Wayleaf4D.Control.service

.PHONY: all clean install-user shaders check-shaders
all: check-shaders $(WORKER)

SHADER_DIR = $(PLASMA_PACKAGE)/contents/shaders
SHADER_COMPILER = /usr/lib/qt6/bin/qsb

check-shaders:
	@for pair in "$(SHADER_DIR)/glow_ring.vert $(SHADER_DIR)/glow_ring.vert.qsb" "$(SHADER_DIR)/glow_ring.frag $(SHADER_DIR)/glow_ring.frag.qsb"; do \
		set -- $$pair; \
		if [ ! -f "$$2" ] || [ "$$1" -nt "$$2" ]; then \
			echo "Missing or stale shader package: $$2" >&2; \
			echo "Run 'make shaders' to regenerate it." >&2; \
			exit 1; \
		fi; \
	done

shaders:
	$(SHADER_COMPILER) --qt6 -o $(SHADER_DIR)/glow_ring.vert.qsb $(SHADER_DIR)/glow_ring.vert
	$(SHADER_COMPILER) --qt6 -o $(SHADER_DIR)/glow_ring.frag.qsb $(SHADER_DIR)/glow_ring.frag

$(WORKER): Makefile Cargo.toml Cargo.lock $(wildcard src/*.rs)
	cargo build --release --locked --target-dir $(CARGO_TARGET_DIR)
	cp $(CARGO_TARGET_DIR)/release/wayleaf-4d $@

clean:
	rm -rf $(BUILD_DIR)

install-user: all
	@if systemctl --user is-active --quiet nanoleaf-sync.service; then \
		echo "Refusing to replace active nanoleaf-sync.service; stop it first." >&2; \
		exit 1; \
	fi
	@for comm in /proc/[0-9]*/comm; do \
		read -r name < "$$comm" || continue; \
		if [ "$$name" = nanoleaf-sync ]; then \
			echo "Refusing to replace active nanoleaf-sync worker; stop it first." >&2; \
			exit 1; \
		fi; \
	done
	@if systemctl --user is-enabled --quiet nanoleaf-sync.service >/dev/null 2>&1; then \
		systemctl --user disable nanoleaf-sync.service; \
	fi
	rm -f "$(USER_CONFIG)/systemd/user/nanoleaf-sync.service" "$(USER_BIN)/nanoleaf-sync" "$(USER_BIN)/nanoleaf-sync-tray"
	install -Dm755 $(WORKER) "$(USER_BIN)/wayleaf-4d.new"
	mv -f -- "$(USER_BIN)/wayleaf-4d.new" "$(USER_BIN)/wayleaf-4d"
	install -d "$(PLASMA_DIR)"
	cp -R "$(PLASMA_PACKAGE)/." "$(PLASMA_DIR)/"
	install -Dm644 "$(PLASMA_PACKAGE)/contents/ambient-light.svg" "$(USER_ICON_DIR)/$(APP_ID).svg"
	install -Dm644 share/systemd/user/wayleaf-4d.service "$(USER_CONFIG)/systemd/user/wayleaf-4d.service"
	install -Dm644 share/applications/$(APP_ID).desktop "$(USER_SHARE)/applications/$(APP_ID).desktop"
	install -d "$(dir $(DBUS_SERVICE))"
	printf '[D-BUS Service]\nName=io.github.avitretiak.Wayleaf4D.Control\nExec="%s/wayleaf-4d" --control\n' "$(USER_BIN)" > "$(DBUS_SERVICE)"
	busctl --user call org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus ReloadConfig
	rm -f "$(PLASMA_DIR)/contents/ui/Nanoleaf/libnanoleafbackend.so"
	systemctl --user daemon-reload
