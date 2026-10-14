CXX ?= c++
CXXFLAGS ?= -O2 -std=c++17 -Wall -Wextra -Wpedantic
PKGS = gio-2.0 gstreamer-app-1.0 gstreamer-video-1.0 json-glib-1.0 libcurl
QT_PKGS = Qt6Core Qt6Qml Qt6DBus
QT_LIBEXECDIR ?= $(shell pkg-config --variable=libexecdir Qt6Core)
MOC ?= $(QT_LIBEXECDIR)/moc
APP_ID = io.github.avitretiak.Wayleaf4D
PLASMA_PACKAGE = share/plasma/$(APP_ID)
USER_SHARE = $(HOME)/.local/share
PLASMA_DIR = $(USER_SHARE)/plasma/plasmoids/$(APP_ID)
QML_MODULE_DIR = $(PLASMA_DIR)/contents/ui/Nanoleaf
USER_CONFIG = $(if $(XDG_CONFIG_HOME),$(XDG_CONFIG_HOME),$(HOME)/.config)
BUILD_DIR = build
WORKER = $(BUILD_DIR)/nanoleaf-sync
BACKEND_MOC = $(BUILD_DIR)/backend.moc
BACKEND_PLUGIN = $(BUILD_DIR)/libnanoleafbackend.so

all: $(WORKER) $(BACKEND_PLUGIN)

$(BUILD_DIR):
	mkdir -p $@

$(WORKER): nanoleaf-sync.cpp | $(BUILD_DIR)
	$(CXX) $(CXXFLAGS) $(shell pkg-config --cflags $(PKGS)) $< -o $@ $(shell pkg-config --libs $(PKGS))

$(BACKEND_MOC): src/plasma/backend.cpp | $(BUILD_DIR)
	$(MOC) $(shell pkg-config --cflags Qt6Core Qt6Qml Qt6DBus) $< -o $@

$(BACKEND_PLUGIN): src/plasma/backend.cpp $(BACKEND_MOC) | $(BUILD_DIR)
	$(CXX) $(CXXFLAGS) -fPIC -shared -I$(BUILD_DIR) $(shell pkg-config --cflags $(QT_PKGS)) $< -o $@ $(shell pkg-config --libs $(QT_PKGS))

.PHONY: all clean install-user
clean:
	rm -rf build
	rm -f nanoleaf-sync nanoleaf-sync-tray libnanoleafbackend.so src/plasma/backend.moc

install-user: all
	install -Dm755 $(WORKER) "$(HOME)/.local/bin/nanoleaf-sync"
	rm -f "$(HOME)/.local/bin/nanoleaf-sync-tray"
	install -d "$(PLASMA_DIR)"
	cp -R "$(PLASMA_PACKAGE)/." "$(PLASMA_DIR)/"
	install -Dm755 $(BACKEND_PLUGIN) "$(QML_MODULE_DIR)/libnanoleafbackend.so"
	install -Dm644 share/systemd/user/nanoleaf-sync.service "$(USER_CONFIG)/systemd/user/nanoleaf-sync.service"
	install -Dm644 share/applications/$(APP_ID).desktop "$(USER_SHARE)/applications/$(APP_ID).desktop"
	systemctl --user daemon-reload
