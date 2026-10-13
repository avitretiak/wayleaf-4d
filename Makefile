CXX ?= c++
CXXFLAGS ?= -O2 -std=c++17 -Wall -Wextra -Wpedantic
PKGS = gio-2.0 gstreamer-app-1.0 gstreamer-video-1.0 json-glib-1.0 libcurl
QT_PKGS = Qt6Widgets Qt6DBus

all: nanoleaf-sync nanoleaf-sync-tray

nanoleaf-sync: nanoleaf-sync.cpp
	$(CXX) $(CXXFLAGS) $(shell pkg-config --cflags $(PKGS)) $< -o $@ $(shell pkg-config --libs $(PKGS))
nanoleaf-sync-tray: nanoleaf-sync-tray.cpp
	$(CXX) $(CXXFLAGS) -fPIC $(shell pkg-config --cflags $(QT_PKGS)) $< -o $@ $(shell pkg-config --libs $(QT_PKGS))

.PHONY: all clean install-user
clean:
	rm -f nanoleaf-sync nanoleaf-sync-tray

install-user: nanoleaf-sync nanoleaf-sync-tray
	install -Dm755 nanoleaf-sync "$(HOME)/.local/bin/nanoleaf-sync"
	install -Dm755 nanoleaf-sync-tray "$(HOME)/.local/bin/nanoleaf-sync-tray"
	install -Dm644 share/applications/org.nanoleaf.NanoleafSync.desktop "$(HOME)/.local/share/applications/org.nanoleaf.NanoleafSync.desktop"
