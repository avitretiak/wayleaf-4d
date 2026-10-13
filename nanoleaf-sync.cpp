#include <gio/gio.h>
#include <glib/gstdio.h>
#include <gst/app/gstappsink.h>
#include <gst/gst.h>
#include <gst/video/video.h>
#include <json-glib/json-glib.h>
#include <curl/curl.h>

#include <algorithm>
#include <array>
#include <cassert>
#include <cmath>
#include <csignal>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <utility>
#include <string>
#include <vector>
#include <chrono>
#include <thread>
#include <arpa/inet.h>
#include <netdb.h>
#include <sys/socket.h>
#include <unistd.h>

namespace {
constexpr int kPanels = 40;
constexpr int kDefaultPort = 60222;
constexpr int kSmallWidth = 320;
volatile sig_atomic_t running = 1;

struct Color { float r = 0, g = 0, b = 0; };
struct Rect { float x0 = 0, x1 = 1, y0 = 0, y1 = 1; };
struct Zone { uint16_t id; float x0, x1, y0, y1; };
struct PanelPosition { uint16_t id; int64_t x, y; };
struct Config {
  std::string host, token, corner = "bottom-center", config_path, portal_restore_token;
  int port = kDefaultPort, offset = 0, fps = 30, max_step = 12, edge_depth = 8, center_bleed = 0, peak_mix = 15, feather_depth = 5;
  float brightness = 1.5f, smooth = 0.35f, black_threshold = 0.01f, saturation_boost = 1.5f;
  bool reverse = false, letterbox = true, midtone_lift = false, strict_black = false;
};
struct Portal { GDBusConnection *bus = nullptr; std::string session; int fd = -1; guint32 node = 0; int width = 1920, height = 1080; };
struct HttpResult { std::string body; long status = 0; };

void stop_signal(int) { running = 0; }
void fail(const std::string &message) { std::fprintf(stderr, "error: %s\n", message.c_str()); std::exit(1); }

bool parse_int(const char *s, int *out) {
  char *end = nullptr; long n = std::strtol(s, &end, 10);
  if (!s[0] || !end || *end || n < -2147483647L || n > 2147483647L) return false;
  *out = static_cast<int>(n); return true;
}
bool parse_float(const char *s, float *out) {
  char *end = nullptr; float n = std::strtof(s, &end);
  if (!s[0] || !end || *end || !std::isfinite(n)) return false;
  *out = n; return true;
}
void usage(const char *name) {
  std::printf("Usage: %s [--pair --ip HOST] | [--ip HOST --token TOKEN] [options]\n"
              "  --config PATH             config file (default: XDG config dir/nanoleaf-sync/config.json)\n"
              "  --pair                    pair now and securely save the device token\n"
              "  --reverse --forward --offset 0..39 --fps 1..60 --port 60222\n"
              "  --brightness 0..2 --saturation-boost 0..3 --smooth 0.05..1\n"
              "  --black-threshold 0..0.1 --max-step 1..255 --edge-depth 5..20%%\n"
              "  --no-letterbox --letterbox --midtone-lift --no-midtone-lift\n"
              "  --strict-black --no-strict-black --self-test --capture-test --capture-debug\n", name);
}
Config arguments(int argc, char **argv, bool *self_test, bool *capture_test, bool *capture_debug, bool *pair, Config c) {
  *self_test = false; *capture_test = false; *capture_debug = false; *pair = false;
  for (int i = 1; i < argc; ++i) {
    std::string a = argv[i];
    if (a == "--help" || a == "-h") { usage(argv[0]); std::exit(0); }
    if (a == "--self-test") { *self_test = true; continue; }
    if (a == "--capture-test") { *capture_test = true; continue; }
    if (a == "--capture-debug") { *capture_debug = true; continue; }
    if (a == "--pair") { *pair = true; continue; }
    if (a == "--reverse") { c.reverse = true; continue; }
    if (a == "--forward") { c.reverse = false; continue; }
    if (a == "--no-letterbox") { c.letterbox = false; continue; }
    if (a == "--letterbox") { c.letterbox = true; continue; }
    if (a == "--midtone-lift") { c.midtone_lift = true; continue; }
    if (a == "--no-midtone-lift") { c.midtone_lift = false; continue; }
    if (a == "--strict-black") { c.strict_black = true; continue; }
    if (a == "--no-strict-black") { c.strict_black = false; continue; }
    if (i + 1 == argc) fail("missing value for " + a);
    const char *v = argv[++i]; int n; float f;
    if (a == "--config") c.config_path = v;
    else if (a == "--ip") c.host = v;
    else if (a == "--token") c.token = v;
    else if (a == "--corner") c.corner = v;
    else if (a == "--offset" && parse_int(v, &n)) c.offset = n;
    else if (a == "--port" && parse_int(v, &n)) c.port = n;
    else if (a == "--fps" && parse_int(v, &n)) c.fps = n;
    else if (a == "--max-step" && parse_int(v, &n)) c.max_step = n;
    else if (a == "--edge-depth" && parse_int(v, &n)) c.edge_depth = n;
    else if (a == "--smooth" && parse_float(v, &f)) c.smooth = f;
    else if (a == "--black-threshold" && parse_float(v, &f)) c.black_threshold = f;
    else if (a == "--brightness" && parse_float(v, &f)) c.brightness = f;
    else if (a == "--saturation-boost" && parse_float(v, &f)) c.saturation_boost = f;
    else fail("invalid option or value: " + a + " " + v);
    if (a == "--offset") c.offset = n;
    if (a == "--port") c.port = n;
    if (a == "--fps") c.fps = n;
    if (a == "--max-step") c.max_step = n;
    if (a == "--edge-depth") c.edge_depth = n;
    if (a == "--smooth") c.smooth = f;
    if (a == "--black-threshold") c.black_threshold = f;
    if (a == "--brightness") c.brightness = f;
    if (a == "--saturation-boost") c.saturation_boost = f;
  }
  if (c.offset < 0 || c.offset > 39 || c.port < 1 || c.port > 65535 || c.fps < 1 || c.fps > 60 || c.max_step < 1 || c.max_step > 255 || c.edge_depth < 5 || c.edge_depth > 20 || c.smooth < 0.05f || c.smooth > 1 || c.black_threshold < 0 || c.black_threshold > 0.1f || c.brightness < 0 || c.brightness > 2 || c.saturation_boost < 0 || c.saturation_boost > 3) fail("option value outside its supported range");
  if (c.corner != "bottom-center" && c.corner != "bottom-left" && c.corner != "top-left" && c.corner != "top-right" && c.corner != "bottom-right") fail("unknown corner: " + c.corner);
  return c;
}

size_t append_http(char *data, size_t size, size_t count, void *opaque) {
  auto *body = static_cast<std::string *>(opaque); body->append(data, size * count); return size * count;
}
HttpResult http(const Config &c, const std::string &path, const char *method, const char *payload = nullptr) {
  CURL *curl = curl_easy_init(); if (!curl) fail("libcurl initialization failed");
  HttpResult result;
  std::string url = "http://" + c.host + ":16021/api/v1/" + c.token + path;
  curl_easy_setopt(curl, CURLOPT_URL, url.c_str());
  curl_easy_setopt(curl, CURLOPT_CUSTOMREQUEST, method);
  curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, append_http);
  curl_easy_setopt(curl, CURLOPT_WRITEDATA, &result.body);
  curl_easy_setopt(curl, CURLOPT_CONNECTTIMEOUT, 3L);
  curl_easy_setopt(curl, CURLOPT_TIMEOUT, 5L);
  curl_easy_setopt(curl, CURLOPT_NOSIGNAL, 1L);
  if (payload) { curl_easy_setopt(curl, CURLOPT_POSTFIELDS, payload); curl_easy_setopt(curl, CURLOPT_POSTFIELDSIZE, static_cast<long>(std::strlen(payload))); }
  CURLcode rc = curl_easy_perform(curl);
  if (rc == CURLE_OK) curl_easy_getinfo(curl, CURLINFO_RESPONSE_CODE, &result.status);
  std::string error = curl_easy_strerror(rc); curl_easy_cleanup(curl);
  if (rc != CURLE_OK) fail("Nanoleaf HTTP request failed: " + error);
  if (result.status < 200 || result.status >= 300) fail("Nanoleaf API returned HTTP " + std::to_string(result.status));
  return result;
}
JsonNode *parse_json(const std::string &text, JsonParser **parser_out = nullptr) {
  GError *error = nullptr; JsonParser *parser = json_parser_new();
  if (!json_parser_load_from_data(parser, text.data(), static_cast<gssize>(text.size()), &error)) {
    std::string message = error ? error->message : "invalid JSON"; g_clear_error(&error); g_object_unref(parser); fail("invalid Nanoleaf response: " + message);
  }
  JsonNode *root = json_parser_get_root(parser);
  if (parser_out) *parser_out = parser; else g_object_unref(parser);
  return root;
}
const char *config_string(JsonObject *obj, const char *key) {
  if (!json_object_has_member(obj, key)) return nullptr;
  JsonNode *node = json_object_get_member(obj, key);
  if (!JSON_NODE_HOLDS_VALUE(node)) fail(std::string("invalid config string: ") + key);
  GValue value = G_VALUE_INIT; json_node_get_value(node, &value);
  gboolean is_string = G_VALUE_HOLDS_STRING(&value); g_value_unset(&value);
  if (!is_string) fail(std::string("invalid config string: ") + key);
  return json_node_get_string(node);
}
void load_config(Config &c) {
  if (!g_file_test(c.config_path.c_str(), G_FILE_TEST_EXISTS)) return;
  JsonParser *parser = json_parser_new(); GError *error = nullptr;
  if (!json_parser_load_from_file(parser, c.config_path.c_str(), &error)) {
    std::string message = error ? error->message : "invalid JSON"; g_clear_error(&error); g_object_unref(parser);
    fail("cannot read config: " + message);
  }
  JsonNode *root = json_parser_get_root(parser);
  if (!JSON_NODE_HOLDS_OBJECT(root)) { g_object_unref(parser); fail("config root must be an object"); }
  JsonObject *obj = json_node_get_object(root);
  if (c.host.empty()) if (const char *v = config_string(obj, "ip")) c.host = v;
  if (c.token.empty()) if (const char *v = config_string(obj, "token")) c.token = v;
  if (const char *v = config_string(obj, "portal_restore_token")) c.portal_restore_token = v;
  if (json_object_has_member(obj, "port")) c.port = static_cast<int>(json_object_get_int_member(obj, "port"));
  if (json_object_has_member(obj, "offset")) c.offset = static_cast<int>(json_object_get_int_member(obj, "offset"));
  if (json_object_has_member(obj, "edge_depth")) c.edge_depth = static_cast<int>(json_object_get_int_member(obj, "edge_depth"));
  if (json_object_has_member(obj, "center_bleed")) c.center_bleed = static_cast<int>(json_object_get_int_member(obj, "center_bleed"));
  if (json_object_has_member(obj, "peak_mix")) c.peak_mix = static_cast<int>(json_object_get_int_member(obj, "peak_mix"));
  if (json_object_has_member(obj, "feather_depth")) c.feather_depth = static_cast<int>(json_object_get_int_member(obj, "feather_depth"));
  if (json_object_has_member(obj, "fps")) c.fps = static_cast<int>(json_object_get_int_member(obj, "fps"));
  if (json_object_has_member(obj, "max_step")) c.max_step = static_cast<int>(json_object_get_int_member(obj, "max_step"));
  if (json_object_has_member(obj, "smooth")) c.smooth = static_cast<float>(json_object_get_double_member(obj, "smooth"));
  if (json_object_has_member(obj, "black_threshold")) c.black_threshold = static_cast<float>(json_object_get_double_member(obj, "black_threshold"));
  if (json_object_has_member(obj, "saturation_boost")) c.saturation_boost = static_cast<float>(json_object_get_double_member(obj, "saturation_boost"));
  if (json_object_has_member(obj, "brightness")) c.brightness = static_cast<float>(json_object_get_double_member(obj, "brightness"));
  if (json_object_has_member(obj, "reverse")) c.reverse = json_object_get_boolean_member(obj, "reverse");
  if (json_object_has_member(obj, "letterbox")) c.letterbox = json_object_get_boolean_member(obj, "letterbox");
  if (json_object_has_member(obj, "midtone_lift")) c.midtone_lift = json_object_get_boolean_member(obj, "midtone_lift");
  if (json_object_has_member(obj, "strict_black")) c.strict_black = json_object_get_boolean_member(obj, "strict_black");
  if (c.offset < 0 || c.offset > 39 || c.port < 1 || c.port > 65535 || c.fps < 1 || c.fps > 60 || c.max_step < 1 || c.max_step > 255 || c.edge_depth < 5 || c.edge_depth > 20 || c.center_bleed < 0 || c.center_bleed > 50 || c.peak_mix < 0 || c.peak_mix > 35 || c.feather_depth < 0 || c.feather_depth > 20 || c.smooth < 0.05f || c.smooth > 1 || c.black_threshold < 0 || c.black_threshold > 0.1f || c.brightness < 0 || c.brightness > 2 || c.saturation_boost < 0 || c.saturation_boost > 3) fail("config value outside its supported range");
  if (c.corner != "bottom-center" && c.corner != "bottom-left" && c.corner != "top-left" && c.corner != "top-right" && c.corner != "bottom-right") fail("unknown config corner: " + c.corner);
  g_object_unref(parser);
}
void save_config(const Config &c) {
  char *dir = g_path_get_dirname(c.config_path.c_str());
  if (g_mkdir_with_parents(dir, 0700) != 0) { std::string path = dir; g_free(dir); fail("cannot create config directory: " + path); }
  g_free(dir);
  JsonObject *obj = json_object_new();
  json_object_set_string_member(obj, "ip", c.host.c_str());
  json_object_set_string_member(obj, "token", c.token.c_str());
  json_object_set_string_member(obj, "portal_restore_token", c.portal_restore_token.c_str());
  json_object_set_string_member(obj, "corner", c.corner.c_str());
  json_object_set_int_member(obj, "port", c.port);
  json_object_set_int_member(obj, "offset", c.offset);
  json_object_set_int_member(obj, "edge_depth", c.edge_depth);
  json_object_set_int_member(obj, "center_bleed", c.center_bleed);
  json_object_set_int_member(obj, "peak_mix", c.peak_mix);
  json_object_set_int_member(obj, "feather_depth", c.feather_depth);
  json_object_set_int_member(obj, "fps", c.fps);
  json_object_set_int_member(obj, "max_step", c.max_step);
  json_object_set_double_member(obj, "smooth", c.smooth);
  json_object_set_double_member(obj, "brightness", c.brightness);
  json_object_set_double_member(obj, "black_threshold", c.black_threshold);
  json_object_set_double_member(obj, "saturation_boost", c.saturation_boost);
  json_object_set_boolean_member(obj, "reverse", c.reverse);
  json_object_set_boolean_member(obj, "letterbox", c.letterbox);
  json_object_set_boolean_member(obj, "midtone_lift", c.midtone_lift);
  json_object_set_boolean_member(obj, "strict_black", c.strict_black);
  JsonNode *root = json_node_new(JSON_NODE_OBJECT); json_node_take_object(root, obj);
  JsonGenerator *generator = json_generator_new(); json_generator_set_root(generator, root);
  gsize length = 0; char *data = json_generator_to_data(generator, &length);
  GError *error = nullptr;
  gboolean saved = g_file_set_contents_full(c.config_path.c_str(), data, static_cast<gssize>(length), G_FILE_SET_CONTENTS_CONSISTENT, 0600, &error);
  g_free(data); g_object_unref(generator); json_node_free(root);
  if (!saved) { std::string message = error ? error->message : "write failed"; g_clear_error(&error); fail("cannot save config: " + message); }
  if (g_chmod(c.config_path.c_str(), 0600) != 0) fail("cannot restrict config file permissions");
}
void pair_device(Config &c) {
  std::fprintf(stderr, "Open the Nanoleaf 4D in the Nanoleaf app and enable Settings > Connect to API. Waiting up to 30 seconds...\n");
  auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(30);
  std::string url = "http://" + c.host + ":16021/api/v1/new";
  while (std::chrono::steady_clock::now() < deadline) {
    CURL *curl = curl_easy_init(); if (!curl) fail("libcurl initialization failed");
    HttpResult result;
    curl_easy_setopt(curl, CURLOPT_URL, url.c_str());
    curl_easy_setopt(curl, CURLOPT_CUSTOMREQUEST, "POST");
    curl_easy_setopt(curl, CURLOPT_WRITEFUNCTION, append_http);
    curl_easy_setopt(curl, CURLOPT_WRITEDATA, &result.body);
    curl_easy_setopt(curl, CURLOPT_CONNECTTIMEOUT, 1L);
    curl_easy_setopt(curl, CURLOPT_TIMEOUT, 2L);
    curl_easy_setopt(curl, CURLOPT_NOSIGNAL, 1L);
    CURLcode rc = curl_easy_perform(curl);
    if (rc == CURLE_OK) curl_easy_getinfo(curl, CURLINFO_RESPONSE_CODE, &result.status);
    curl_easy_cleanup(curl);
    if (rc == CURLE_OK && result.status >= 200 && result.status < 300) {
      JsonParser *parser = nullptr; JsonNode *root = parse_json(result.body, &parser);
      if (!JSON_NODE_HOLDS_OBJECT(root)) { g_object_unref(parser); fail("pairing response is not an object"); }
      const char *token = config_string(json_node_get_object(root), "auth_token");
      if (!token || !*token) { g_object_unref(parser); fail("pairing response has no auth_token"); }
      c.token = token; g_object_unref(parser); save_config(c);
      std::fprintf(stderr, "Paired with %s; credentials saved to %s\n", c.host.c_str(), c.config_path.c_str());
      return;
    }
    std::this_thread::sleep_for(std::chrono::milliseconds(500));
  }
  fail("pairing timed out; re-enable Connect to API in the Nanoleaf app and retry");
}
std::vector<PanelPosition> panel_layout(const Config &c) {
  HttpResult response = http(c, "/panelLayout/layout", "GET");
  JsonParser *parser = nullptr; JsonNode *root = parse_json(response.body, &parser);
  if (!JSON_NODE_HOLDS_OBJECT(root)) fail("Nanoleaf layout response is not an object");
  JsonObject *obj = json_node_get_object(root);
  JsonArray *positions = json_object_get_array_member(obj, "positionData");
  gint64 count = json_object_get_int_member(obj, "numPanels");
  if (!positions || count < 4 || count > kPanels || json_array_get_length(positions) != guint(count)) fail("Nanoleaf 4D layout must contain 4..40 segments");
  std::vector<PanelPosition> panels; panels.reserve(static_cast<size_t>(count)); bool seen[65536] = {};
  for (guint i = 0; i < guint(count); ++i) {
    JsonNode *node = json_array_get_element(positions, i);
    if (!JSON_NODE_HOLDS_OBJECT(node)) fail("malformed panel layout entry");
    JsonObject *entry = json_node_get_object(node);
    gint64 id = json_object_get_int_member(entry, "panelId");
    if (id < 0 || id > 65535 || seen[id]) fail("invalid or duplicate panel id in layout");
    seen[id] = true;
    if (!json_object_has_member(entry, "x") || !json_object_has_member(entry, "y")) fail("panel layout entry is missing coordinates");
    panels.push_back({static_cast<uint16_t>(id), json_object_get_int_member(entry, "x"), json_object_get_int_member(entry, "y")});
  }
  g_object_unref(parser); return panels;
}
int enable_stream(const Config &c) {
  HttpResult response = http(c, "/effects", "PUT", "{\"write\":{\"command\":\"display\",\"animType\":\"extControl\",\"extControlVersion\":\"v2\"}}");
  if (response.body.empty()) return c.port;
  JsonParser *parser = nullptr; JsonNode *root = parse_json(response.body, &parser);
  int port = c.port;
  if (JSON_NODE_HOLDS_OBJECT(root)) {
    JsonObject *obj = json_node_get_object(root);
    if (json_object_has_member(obj, "streamControlPort")) port = static_cast<int>(json_object_get_int_member(obj, "streamControlPort"));
  }
  g_object_unref(parser);
  if (port < 1 || port > 65535) fail("invalid Nanoleaf stream port");
  return port;
}

struct RequestWait { GMainLoop *loop; guint code = 2; GVariant *results = nullptr; };
void response_signal(GDBusConnection *, const gchar *, const gchar *, const gchar *, const gchar *, GVariant *parameters, gpointer data) {
  auto *wait = static_cast<RequestWait *>(data);
  g_variant_get(parameters, "(u@a{sv})", &wait->code, &wait->results);
  g_main_loop_quit(wait->loop);
}
GVariant *request_options(const char *token, const char *extra_key = nullptr, GVariant *extra = nullptr) {
  GVariantBuilder b; g_variant_builder_init(&b, G_VARIANT_TYPE_VARDICT);
  g_variant_builder_add(&b, "{sv}", "handle_token", g_variant_new_string(token));
  if (extra_key) g_variant_builder_add(&b, "{sv}", extra_key, extra);
  return g_variant_builder_end(&b);
}
std::string sender_path(GDBusConnection *bus) {
  std::string name = g_dbus_connection_get_unique_name(bus);
  if (!name.empty() && name[0] == ':') name.erase(0, 1);
  std::replace(name.begin(), name.end(), '.', '_');
  return name;
}
GVariant *portal_request(Portal &p, const char *method, GVariant *parameters, const char *handle_token) {
  std::string request_path = "/org/freedesktop/portal/desktop/request/" + sender_path(p.bus) + "/" + handle_token;
  RequestWait wait{g_main_loop_new(nullptr, FALSE)};
  guint sub = g_dbus_connection_signal_subscribe(p.bus, "org.freedesktop.portal.Desktop", "org.freedesktop.portal.Request", "Response", request_path.c_str(), nullptr, G_DBUS_SIGNAL_FLAGS_NONE, response_signal, &wait, nullptr);
  GError *error = nullptr;
  GVariant *reply = g_dbus_connection_call_sync(p.bus, "org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop", "org.freedesktop.portal.ScreenCast", method, parameters, G_VARIANT_TYPE("(o)"), G_DBUS_CALL_FLAGS_NONE, 30000, nullptr, &error);
  if (!reply) { g_dbus_connection_signal_unsubscribe(p.bus, sub); g_main_loop_unref(wait.loop); std::string msg = error ? error->message : "portal call failed"; g_clear_error(&error); fail("ScreenCast " + std::string(method) + ": " + msg); }
  const char *actual = nullptr; g_variant_get(reply, "(&o)", &actual);
  if (request_path != actual) {
    g_dbus_connection_signal_unsubscribe(p.bus, sub);
    sub = g_dbus_connection_signal_subscribe(p.bus, "org.freedesktop.portal.Desktop", "org.freedesktop.portal.Request", "Response", actual, nullptr, G_DBUS_SIGNAL_FLAGS_NONE, response_signal, &wait, nullptr);
  }
  g_variant_unref(reply);
  g_main_loop_run(wait.loop);
  g_dbus_connection_signal_unsubscribe(p.bus, sub); g_main_loop_unref(wait.loop);
  if (wait.code != 0) { if (wait.results) g_variant_unref(wait.results); fail("screen capture permission was cancelled or denied"); }
  return wait.results;
}
void register_portal_app(GDBusConnection *bus) {
  GVariantBuilder options; g_variant_builder_init(&options, G_VARIANT_TYPE_VARDICT);
  GError *error = nullptr;
  GVariant *reply = g_dbus_connection_call_sync(bus, "org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop", "org.freedesktop.host.portal.Registry", "Register", g_variant_new("(s@a{sv})", "org.nanoleaf.NanoleafSync", g_variant_builder_end(&options)), G_VARIANT_TYPE("()"), G_DBUS_CALL_FLAGS_NONE, 5000, nullptr, &error);
  if (!reply) { std::string message = error ? error->message : "portal app registration failed"; g_clear_error(&error); fail("register portal app identity: " + message); }
  g_variant_unref(reply);
}
GVariant *source_options(const char *token, const Config &cfg) {
  GVariantBuilder b; g_variant_builder_init(&b, G_VARIANT_TYPE_VARDICT);
  g_variant_builder_add(&b, "{sv}", "handle_token", g_variant_new_string(token));
  g_variant_builder_add(&b, "{sv}", "types", g_variant_new_uint32(1));
  g_variant_builder_add(&b, "{sv}", "persist_mode", g_variant_new_uint32(2));
  if (!cfg.portal_restore_token.empty()) g_variant_builder_add(&b, "{sv}", "restore_token", g_variant_new_string(cfg.portal_restore_token.c_str()));
  return g_variant_builder_end(&b);
}
Portal start_portal(Config &cfg, bool persist_restore_token = true) {
  Portal p; GError *error = nullptr; p.bus = g_bus_get_sync(G_BUS_TYPE_SESSION, nullptr, &error);
  if (!p.bus) { std::string msg = error ? error->message : "no session bus"; g_clear_error(&error); fail("connect to desktop portal: " + msg); }
  register_portal_app(p.bus);
  std::string token = "nl_" + std::to_string(getpid()) + "_" + std::to_string(g_random_int());
  GVariant *res = portal_request(p, "CreateSession", g_variant_new("(@a{sv})", request_options(token.c_str(), "session_handle_token", g_variant_new_string(token.c_str()))), token.c_str());
  const char *session = nullptr;
  if (!g_variant_lookup(res, "session_handle", "&s", &session)) fail("portal did not return a session handle");
  p.session = session; g_variant_unref(res);
  token += "_select";
  res = portal_request(p, "SelectSources", g_variant_new("(o@a{sv})", p.session.c_str(), source_options(token.c_str(), cfg)), token.c_str());
  g_variant_unref(res);
  token += "_start";
  res = portal_request(p, "Start", g_variant_new("(os@a{sv})", p.session.c_str(), "", request_options(token.c_str())), token.c_str());
  const char *restore = nullptr;
  cfg.portal_restore_token = g_variant_lookup(res, "restore_token", "&s", &restore) ? restore : "";
  if (persist_restore_token) save_config(cfg);
  GVariant *streams = g_variant_lookup_value(res, "streams", G_VARIANT_TYPE("a(ua{sv})"));
  if (!streams || g_variant_n_children(streams) == 0) fail("portal returned no screen streams");
  GVariant *stream = g_variant_get_child_value(streams, 0);
  GVariant *node = g_variant_get_child_value(stream, 0); p.node = g_variant_get_uint32(node); g_variant_unref(node);
  GVariant *props = g_variant_get_child_value(stream, 1);
  g_variant_lookup(props, "size", "(ii)", &p.width, &p.height);
  g_variant_unref(props); g_variant_unref(stream); g_variant_unref(streams); g_variant_unref(res);
  if (p.width <= 0 || p.height <= 0) { p.width = 1920; p.height = 1080; }
  GVariantBuilder empty; g_variant_builder_init(&empty, G_VARIANT_TYPE_VARDICT);
  GUnixFDList *out_fds = nullptr;
  res = g_dbus_connection_call_with_unix_fd_list_sync(p.bus, "org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop", "org.freedesktop.portal.ScreenCast", "OpenPipeWireRemote", g_variant_new("(o@a{sv})", p.session.c_str(), g_variant_builder_end(&empty)), G_VARIANT_TYPE("(h)"), G_DBUS_CALL_FLAGS_NONE, 30000, nullptr, &out_fds, nullptr, &error);
  if (!res) { std::string msg = error ? error->message : "could not open PipeWire remote"; g_clear_error(&error); fail(msg); }
  int handle = -1; g_variant_get(res, "(h)", &handle); p.fd = g_unix_fd_list_get(out_fds, handle, &error);
  g_variant_unref(res); g_object_unref(out_fds);
  if (p.fd < 0) { std::string msg = error ? error->message : "invalid PipeWire descriptor"; g_clear_error(&error); fail(msg); }
  return p;
}
void close_portal(Portal &p) {
  if (!p.session.empty()) g_dbus_connection_call(p.bus, "org.freedesktop.portal.Desktop", p.session.c_str(), "org.freedesktop.portal.Session", "Close", nullptr, nullptr, G_DBUS_CALL_FLAGS_NONE, 1000, nullptr, nullptr, nullptr);
  if (p.fd >= 0) close(p.fd);
  if (p.bus) g_object_unref(p.bus);
}

std::vector<Zone> make_zones(const std::vector<PanelPosition> &panels, const Config &c) {
  const size_t n = panels.size();
  if (n < 4 || n > kPanels) fail("unsupported Nanoleaf segment count");
  if (c.offset < 0 || c.offset >= kPanels) fail("offset must be in 0..39");
  if (c.edge_depth < 5 || c.edge_depth > 20) fail("edge depth must be in 5..20 percent");
  std::vector<Zone> zones(n);
  const float border = float(c.edge_depth) / 100.0f, corner = border * 1.5f;
  if (n == kPanels) {
    for (int i = 0; i < kPanels; ++i) {
      float x = 0, y = 0;
      if (i <= 7) { x = 350.0f - 50.0f * i; y = 0; }
      else if (i <= 15) { x = 0; y = 50.0f * (i - 7); }
      else if (i <= 28) { x = 50.0f * (i - 15); y = 400; }
      else if (i <= 35) { x = 650; y = 400.0f - 50.0f * (i - 28); }
      else { x = 600.0f - 50.0f * (i - 36); y = 50; }
      float x0, x1, y0, y1;
      if (i == 7) { x0 = 0; x1 = corner; y0 = 1 - corner; y1 = 1; }
      else if (i == 15) { x0 = 0; x1 = corner; y0 = 0; y1 = corner; }
      else if (i == 28) { x0 = 1 - corner; x1 = 1; y0 = 0; y1 = corner; }
      else if (i == 35) { x0 = 1 - corner; x1 = 1; y0 = 1 - corner; y1 = 1; }
      else if (x == 0) { float cy = 1 - y / 400; float span = (50.0f / 400.0f) * 0.7f; x0 = 0; x1 = border; y0 = std::max(0.0f, cy - span); y1 = std::min(1.0f, cy + span); }
      else if (y >= 390) { float cx = x / 650; float span = (50.0f / 650.0f) * 0.7f; x0 = std::max(0.0f, cx - span); x1 = std::min(1.0f, cx + span); y0 = 0; y1 = border; }
      else if (x >= 640) { float cy = 1 - y / 400; float span = (50.0f / 400.0f) * 0.7f; x0 = 1 - border; x1 = 1; y0 = std::max(0.0f, cy - span); y1 = std::min(1.0f, cy + span); }
      else { float cx = x / 650; float span = (50.0f / 650.0f) * 0.7f; x0 = std::max(0.0f, cx - span); x1 = std::min(1.0f, cx + span); y0 = 1 - border; y1 = 1; }
      zones[i] = {panels[i].id, x0, x1, y0, y1};
    }
  } else if (n == 34) {
    int64_t min_x = panels[0].x, max_x = panels[0].x, min_y = panels[0].y, max_y = panels[0].y;
    for (const auto &panel : panels) {
      min_x = std::min(min_x, panel.x); max_x = std::max(max_x, panel.x);
      min_y = std::min(min_y, panel.y); max_y = std::max(max_y, panel.y);
    }
    if (min_x == max_x || min_y == max_y) fail("34-segment layout has no coordinate extent");
    auto tangent = [&](size_t i, bool vertical) {
      if (vertical) return 1.0f - float(panels[i].y - min_y) / float(max_y - min_y);
      return float(panels[i].x - min_x) / float(max_x - min_x);
    };
    auto interval = [&](size_t first, size_t end, size_t i, bool vertical) {
      float center = tangent(i, vertical);
      float a = i == first ? center + (center - tangent(i + 1, vertical)) * 0.5f
                           : (tangent(i - 1, vertical) + center) * 0.5f;
      float b = i + 1 == end ? center + (center - tangent(i - 1, vertical)) * 0.5f
                             : (center + tangent(i + 1, vertical)) * 0.5f;
      return std::pair<float, float>{std::max(0.0f, std::min(a, b)), std::min(1.0f, std::max(a, b))};
    };
    for (size_t i = 0; i < 6; ++i) {
      auto [x0, x1] = interval(0, 6, i, false);
      zones[i] = {panels[i].id, x0, x1, 1 - border, 1};
    }
    for (size_t i = 6; i < 12; ++i) {
      auto [y0, y1] = interval(6, 12, i, true);
      zones[i] = {panels[i].id, 0, border, y0, y1};
    }
    for (size_t i = 12; i < 21; ++i) {
      auto [x0, x1] = interval(12, 21, i, false);
      zones[i] = {panels[i].id, x0, x1, 0, border};
    }
    for (size_t i = 21; i < 27; ++i) {
      auto [y0, y1] = interval(21, 27, i, true);
      zones[i] = {panels[i].id, 1 - border, 1, y0, y1};
    }
    for (size_t i = 27; i < 34; ++i) {
      auto [x0, x1] = interval(27, 34, i, false);
      zones[i] = {panels[i].id, x0, x1, 1 - border, 1};
    }
  } else {
    int left = std::max(1, int(std::lround(9.0 / 50.0 * n)));
    int top = std::max(1, int(std::lround(16.0 / 50.0 * n)));
    int right = std::max(1, int(std::lround(9.0 / 50.0 * n)));
    int bottom = std::max(1, int(n) - left - top - right);
    int index = 0;
    int bottom_left = bottom / 2;
    for (int i = 0; i < bottom_left; ++i) {
      float a = 0.5f - float(i) * 0.5f / bottom_left, b = 0.5f - float(i + 1) * 0.5f / bottom_left;
      zones[index] = {panels[index].id, std::min(a,b), std::max(a,b), 1-border, 1}; ++index;
    }
    for (int i = 0; i < left; ++i) {
      float a = 1.0f - float(i) / left, b = 1.0f - float(i + 1) / left;
      zones[index] = {panels[index].id, 0, border, std::min(a,b), std::max(a,b)}; ++index;
    }
    for (int i = 0; i < top; ++i) {
      float a = float(i)/top, b = float(i+1)/top;
      zones[index] = {panels[index].id, a, b, 0, border}; ++index;
    }
    for (int i = 0; i < right; ++i) {
      float a = float(i)/right, b = float(i+1)/right;
      zones[index] = {panels[index].id, 1-border, 1, a, b}; ++index;
    }
    int bottom_right = bottom - bottom_left;
    for (int i = 0; i < bottom_right; ++i) {
      float a = 1.0f - float(i) * 0.5f / bottom_right, b = 1.0f - float(i + 1) * 0.5f / bottom_right;
      zones[index] = {panels[index].id, std::min(a,b), std::max(a,b), 1-border, 1}; ++index;
    }
  }
  int corner_index = n == 34
      ? (c.corner == "bottom-left" ? 6 : c.corner == "top-left" ? 11 : c.corner == "top-right" ? 20 : c.corner == "bottom-right" ? 26 : 0)
      : (c.corner == "bottom-left" ? 7 : c.corner == "top-left" ? 15 : c.corner == "top-right" ? 28 : c.corner == "bottom-right" ? 35 : 0);
  int start = n == 34 ? corner_index : corner_index * int(n) / kPanels;
  for (size_t i = 0; i < n; ++i) {
    int source = (start + c.offset % int(n) + (c.reverse ? -int(i) : int(i))) % int(n);
    if (source < 0) source += int(n);
    zones[i].id = panels[source].id;
  }
  return zones;
}

Color pixel(const uint8_t *data, int stride, int x, int y) {
  const uint8_t *p = data + y * stride + x * 4;
  return {float(p[0]), float(p[1]), float(p[2])};
}
float luma(Color c) { return (0.2126f * c.r + 0.7152f * c.g + 0.0722f * c.b) / 255.0f; }
float saturation(Color c) { float hi = std::max({c.r, c.g, c.b}), lo = std::min({c.r, c.g, c.b}); return hi > 0.001f ? (hi - lo) / hi : 0; }
Color mix(Color a, Color b, float t) { return {a.r + (b.r-a.r)*t, a.g + (b.g-a.g)*t, a.b + (b.b-a.b)*t}; }
Color boost_saturation(Color c, float factor) {
  float hi = std::max({c.r,c.g,c.b}), lo = std::min({c.r,c.g,c.b});
  if (hi <= 0 || factor <= 1) return c;
  float delta = hi - lo, h = 0, s = delta / hi, v = hi;
  if (delta > 0) {
    if (hi == c.r) h = std::fmod((c.g-c.b)/delta, 6.0f);
    else if (hi == c.g) h = (c.b-c.r)/delta + 2;
    else h = (c.r-c.g)/delta + 4;
    h *= 60; if (h < 0) h += 360;
  }
  s = std::min(1.0f, s * factor); float chroma = v*s, x = chroma * (1 - std::fabs(std::fmod(h/60,2.0f)-1)), m = v-chroma;
  Color o;
  if (h < 60) o={chroma,x,0}; else if (h < 120) o={x,chroma,0}; else if (h < 180) o={0,chroma,x}; else if (h < 240) o={0,x,chroma}; else if (h < 300) o={x,0,chroma}; else o={chroma,0,x};
  return {o.r+m,o.g+m,o.b+m};
}
float row_luma(const uint8_t *data, int stride, int width, int y) {
  unsigned sum=0, n=0; for (int x=0; x<width; x+=4) { Color c=pixel(data,stride,x,y); sum += unsigned(luma(c)*255); ++n; } return n ? float(sum)/n : 0;
}
float col_luma(const uint8_t *data, int stride, int height, int x) {
  unsigned sum=0, n=0; for (int y=0; y<height; y+=4) { Color c=pixel(data,stride,x,y); sum += unsigned(luma(c)*255); ++n; } return n ? float(sum)/n : 0;
}
Rect detect_active_rect(const uint8_t *data, int stride, int w, int h) {
  int top=0,bottom=0,left=0,right=0;
  for (int y=0; y<h/3 && row_luma(data,stride,w,y)<=12; ++y) top=y+1;
  for (int i=0; i<h/3 && row_luma(data,stride,w,h-1-i)<=12; ++i) bottom=i+1;
  for (int x=0; x<w/3 && col_luma(data,stride,h,x)<=12; ++x) left=x+1;
  for (int i=0; i<w/3 && col_luma(data,stride,h,w-1-i)<=12; ++i) right=i+1;
  Rect r;
  if (top >= 4 && std::abs(top-bottom) <= 6) { r.y0=float(top)/h; r.y1=float(h-bottom)/h; }
  if (left >= 4 && std::abs(left-right) <= 6) { r.x0=float(left)/w; r.x1=float(w-right)/w; }
  return r;
}

struct Sampler {
  std::vector<Zone> zones;
  std::vector<Color> previous, out;
  Rect active;
  uint64_t frames=0;
  Color global{};
  int64_t last_us=0;
  const Config &cfg;
  explicit Sampler(std::vector<Zone> z, const Config &c): zones(std::move(z)), previous(zones.size()), out(zones.size()), cfg(c) {}
  const std::vector<Color> &sample(const uint8_t *data, int stride, int w, int h, bool *scene_cut, std::vector<Color> *raw_colors = nullptr) {
    if (raw_colors) raw_colors->resize(zones.size());
    ++frames; int64_t now=g_get_monotonic_time(); float elapsed=last_us ? float(now-last_us)/1000000.0f : 1.0f/30; last_us=now; elapsed=std::min(elapsed,0.1f);
    if (cfg.letterbox && (frames==1 || frames%15==0)) active=detect_active_rect(data,stride,w,h);
    float active_width=std::max(0.1f,active.x1-active.x0), active_height=std::max(0.1f,active.y1-active.y0);
    int cx0=std::clamp(int((active.x0+0.25f*active_width)*w),0,w-1), cx1=std::clamp(int((active.x0+0.75f*active_width)*w),cx0+1,w);
    int cy0=std::clamp(int((active.y0+0.25f*active_height)*h),0,h-1), cy1=std::clamp(int((active.y0+0.75f*active_height)*h),cy0+1,h);
    double sr=0,sg=0,sb=0; uint64_t count=0;
    for (int y=cy0; y<cy1; y+=4) for (int x=cx0; x<cx1; x+=4) { Color c=pixel(data,stride,x,y); sr+=c.r; sg+=c.g; sb+=c.b; ++count; }
    Color current_global = count ? Color{float(sr/count),float(sg/count),float(sb/count)} : Color{};
    *scene_cut = frames>1 && std::sqrt(std::pow((current_global.r-global.r)/255,2)+std::pow((current_global.g-global.g)/255,2)+std::pow((current_global.b-global.b)/255,2)) > 0.35f;
    global=current_global;
    const float feather=cfg.feather_depth/100.0f;
    auto fade=[](float t) { t=std::clamp(t,0.0f,1.0f); return 1.0f-t*t*(3.0f-2.0f*t); };
    for (size_t i=0; i<zones.size(); ++i) {
      const Zone &z=zones[i];
      const bool left=z.x0==0.0f, right=z.x1==1.0f, top=z.y0==0.0f, bottom=z.y1==1.0f;
      float sx0=z.x0, sx1=z.x1, sy0=z.y0, sy1=z.y1;
      if (feather>0) {
        if (left) sx1=std::min(1.0f,sx1+feather); else if (right) sx0=std::max(0.0f,sx0-feather);
        if (top) sy1=std::min(1.0f,sy1+feather); else if (bottom) sy0=std::max(0.0f,sy0-feather);
      }
      int x0=std::clamp(int((active.x0+sx0*active_width)*w),0,w-1), x1=std::clamp(int((active.x0+sx1*active_width)*w),x0+1,w);
      int y0=std::clamp(int((active.y0+sy0*active_height)*h),0,h-1), y1=std::clamp(int((active.y0+sy1*active_height)*h),y0+1,h);
      double r=0,g=0,b=0,total=0; Color peak{}; float peak_luma=0;
      for (int y=y0; y<y1; y+=2) for (int x=x0; x<x1; x+=2) {
        float spatial=1.0f;
        if (feather>0) {
          float nx=((float(x)+0.5f)/w-active.x0)/active_width, ny=((float(y)+0.5f)/h-active.y0)/active_height;
          if (left && nx>z.x1) spatial*=fade((nx-z.x1)/feather);
          else if (right && nx<z.x0) spatial*=fade((z.x0-nx)/feather);
          if (top && ny>z.y1) spatial*=fade((ny-z.y1)/feather);
          else if (bottom && ny<z.y0) spatial*=fade((z.y0-ny)/feather);
        }
        if (spatial<=0) continue;
        Color c=pixel(data,stride,x,y); float sat=saturation(c), lum=luma(c), weight=(1+cfg.saturation_boost*sat*sat)*spatial;
        r+=c.r*weight; g+=c.g*weight; b+=c.b*weight; total+=weight;
        float weighted_luma=lum*spatial;
        if (weighted_luma>peak_luma) { peak_luma=weighted_luma; peak={c.r*spatial,c.g*spatial,c.b*spatial}; }
      }
      Color target=total ? Color{float(r/total),float(g/total),float(b/total)} : Color{};
      if (peak_luma>0) target=mix(target,peak,float(cfg.peak_mix)/100.0f);
      target=mix(target,current_global,float(cfg.center_bleed)/100.0f);
      if (cfg.black_threshold>0) { float lum=luma(target); if (lum<=cfg.black_threshold) target={}; else { float ramp=std::clamp((lum-cfg.black_threshold)/cfg.black_threshold,0.0f,1.0f); target={target.r*ramp,target.g*ramp,target.b*ramp}; } }
      target=boost_saturation(target,cfg.saturation_boost);
      if (cfg.midtone_lift) { auto lift=[](float x){x/=255; return std::clamp(x/(x+0.25f)*1.25f,0.0f,1.0f)*255;}; target={lift(target.r),lift(target.g),lift(target.b)}; }
      target={std::clamp(target.r*cfg.brightness,0.0f,255.0f),std::clamp(target.g*cfg.brightness,0.0f,255.0f),std::clamp(target.b*cfg.brightness,0.0f,255.0f)};
      if (raw_colors) (*raw_colors)[i] = target;
      if (cfg.strict_black && target.r==0 && target.g==0 && target.b==0) { previous[i]={}; out[i]={}; continue; }
      float alpha = *scene_cut ? 1.0f : 1-std::pow(1-cfg.smooth,elapsed*30.0f);
      Color candidate=mix(previous[i],target,alpha); float step=cfg.max_step*elapsed*30;
      auto bounded=[step](float a,float b){ return a+std::clamp(b-a,-step,step); };
      previous[i]={bounded(previous[i].r,candidate.r),bounded(previous[i].g,candidate.g),bounded(previous[i].b,candidate.b)};
      out[i]=previous[i];
    }
    return out;
  }
  void remap(std::vector<Zone> next) {
    bool changed = next.size() != zones.size();
    for (size_t i=0; !changed && i<zones.size(); ++i)
      changed = next[i].id != zones[i].id || next[i].x0 != zones[i].x0 || next[i].x1 != zones[i].x1 || next[i].y0 != zones[i].y0 || next[i].y1 != zones[i].y1;
    zones = std::move(next);
    if (changed) { previous.assign(zones.size(), {}); out.assign(zones.size(), {}); }
  }
};

using Packet = std::array<uint8_t, 2 + kPanels * 8>;
void make_packet(const std::vector<Zone> &zones, const std::vector<Color> &colors, Packet &packet) {
  if (zones.size() != colors.size() || zones.size() > kPanels) fail("invalid Nanoleaf frame dimensions");
  packet[0]=0; packet[1]=static_cast<uint8_t>(zones.size());
  for (size_t i=0;i<zones.size();++i) {
    size_t o=2+i*8; packet[o]=zones[i].id>>8; packet[o+1]=zones[i].id;
    packet[o+2]=uint8_t(std::clamp(std::lround(colors[i].r),0L,255L)); packet[o+3]=uint8_t(std::clamp(std::lround(colors[i].g),0L,255L)); packet[o+4]=uint8_t(std::clamp(std::lround(colors[i].b),0L,255L)); packet[o+5]=0; packet[o+6]=0; packet[o+7]=0;
  }
}
void send_packet(int fd, const Packet &packet, size_t panel_count) { size_t length=2+panel_count*8; ssize_t n=send(fd,packet.data(),length,0); if(n<0 || size_t(n)!=length) fail("Nanoleaf UDP send failed"); }
int udp_socket(const std::string &host, int port) {
  addrinfo hints{}; hints.ai_family=AF_UNSPEC; hints.ai_socktype=SOCK_DGRAM; addrinfo *list=nullptr;
  std::string service=std::to_string(port); int rc=getaddrinfo(host.c_str(),service.c_str(),&hints,&list);
  if (rc) fail("resolve Nanoleaf UDP target: " + std::string(gai_strerror(rc)));
  int fd=-1;
  for (addrinfo *a=list;a;a=a->ai_next) { fd=socket(a->ai_family,a->ai_socktype,a->ai_protocol); if(fd>=0 && connect(fd,a->ai_addr,a->ai_addrlen)==0) break; if(fd>=0) close(fd); fd=-1; }
  freeaddrinfo(list); if(fd<0) fail("cannot open Nanoleaf UDP socket"); return fd;
}

struct Capture { Portal portal; GstElement *pipeline=nullptr, *rate=nullptr, *fps_caps=nullptr; GstAppSink *sink=nullptr; };
Capture start_capture(Config &cfg, bool persist_restore_token = true) {
  Capture c; c.portal=start_portal(cfg,persist_restore_token);
  int width=std::min(kSmallWidth,c.portal.width), height=std::max(1,int(std::lround(width*double(c.portal.height)/c.portal.width)));
  std::string desc="pipewiresrc fd="+std::to_string(c.portal.fd)+" path="+std::to_string(c.portal.node)+" do-timestamp=true ! videorate name=rate drop-only=true max-rate="+std::to_string(cfg.fps)+" ! capsfilter name=fps_caps caps=\"video/x-raw,framerate=(fraction)"+std::to_string(cfg.fps)+"/1\" ! videoconvert ! videoscale ! video/x-raw,format=RGBA,width="+std::to_string(width)+",height="+std::to_string(height)+" ! appsink name=frames max-buffers=1 drop=true sync=false";
  GError *error=nullptr; c.pipeline=gst_parse_launch(desc.c_str(),&error);
  if(!c.pipeline){ std::string msg=error?error->message:"failed to create GStreamer pipeline"; g_clear_error(&error); fail(msg); }
  GstElement *sink=gst_bin_get_by_name(GST_BIN(c.pipeline),"frames");
  if(!sink) fail("GStreamer appsink is missing");
  c.sink=GST_APP_SINK(sink);
  c.rate=gst_bin_get_by_name(GST_BIN(c.pipeline),"rate");
  c.fps_caps=gst_bin_get_by_name(GST_BIN(c.pipeline),"fps_caps");
  if(!c.rate || !c.fps_caps) fail("GStreamer FPS elements are missing");
  if(gst_element_set_state(c.pipeline,GST_STATE_PLAYING)==GST_STATE_CHANGE_FAILURE) fail("GStreamer could not start PipeWire capture");
  std::fprintf(stderr,"ScreenCast granted; sampling %dx%d at up to %d FPS\n",width,height,cfg.fps);
  return c;
}
void stop_capture(Capture &c) { if(c.pipeline){gst_element_set_state(c.pipeline,GST_STATE_NULL);gst_object_unref(c.sink);gst_object_unref(c.rate);gst_object_unref(c.fps_caps);gst_object_unref(c.pipeline);} close_portal(c.portal); }
int capture_test(Config &cfg) {
  std::signal(SIGINT,stop_signal); std::signal(SIGTERM,stop_signal);
  Capture cap=start_capture(cfg); int64_t started=g_get_monotonic_time(); int frames=0, width=0, height=0;
  while(running && frames<30 && g_get_monotonic_time()-started<5000000) {
    GstSample *sample=gst_app_sink_try_pull_sample(cap.sink,GST_SECOND);
    if(!sample) continue;
    GstCaps *caps=gst_sample_get_caps(sample); GstVideoInfo info; gst_video_info_init(&info);
    GstBuffer *buffer=gst_sample_get_buffer(sample);
    if(caps && buffer && gst_video_info_from_caps(&info,caps)) {
      GstVideoFrame frame;
      if(gst_video_frame_map(&frame,&info,buffer,GST_MAP_READ)) {
        width=GST_VIDEO_FRAME_WIDTH(&frame); height=GST_VIDEO_FRAME_HEIGHT(&frame); ++frames;
        gst_video_frame_unmap(&frame);
      }
    }
    gst_sample_unref(sample);
  }
  int64_t elapsed=g_get_monotonic_time()-started; stop_capture(cap);
  std::printf("PipeWire capture: %d frames at %dx%d in %.2f s (%.1f FPS)\n",frames,width,height,elapsed/1e6,elapsed?frames*1e6/elapsed:0.0);
  return frames>=30 ? 0 : 1;
}
int capture_debug(Config &cfg) {
  std::signal(SIGINT,stop_signal); std::signal(SIGTERM,stop_signal);
  auto panels=panel_layout(cfg); auto zones=make_zones(panels,cfg);
  Capture cap=start_capture(cfg,false); Sampler sampler(zones,cfg); std::vector<Color> raw(zones.size());
  std::vector<double> pts_intervals, arrival_intervals; pts_intervals.reserve(600); arrival_intervals.reserve(600);
  constexpr int64_t duration_us=10000000;
  const double expected_ms=1000.0/cfg.fps;
  int64_t started=g_get_monotonic_time(), previous_arrival=0;
  GstClockTime first_pts=GST_CLOCK_TIME_NONE, previous_pts=GST_CLOCK_TIME_NONE;
  size_t frames=0, pts_gaps=0, arrival_gaps=0, pts_regressions=0, scene_cuts=0, active_changes=0;
  Rect previous_active{}; bool have_active=false;
  std::puts("# capture-debug: no UDP frames sent; each zone is controller-ID:target-RGB>output-RGB");
  std::fprintf(stderr,"Recording 10 seconds of sampled colors/timing only; no desktop images are saved\n");
  while(running && g_get_monotonic_time()-started<duration_us) {
    GstSample *sample=gst_app_sink_try_pull_sample(cap.sink,100*GST_MSECOND);
    int64_t arrival=g_get_monotonic_time();
    if(!sample) continue;
    GstCaps *caps=gst_sample_get_caps(sample); GstVideoInfo info; gst_video_info_init(&info);
    GstBuffer *buffer=gst_sample_get_buffer(sample);
    if(!caps || !buffer || !gst_video_info_from_caps(&info,caps)) { gst_sample_unref(sample); continue; }
    GstVideoFrame frame;
    if(!gst_video_frame_map(&frame,&info,buffer,GST_MAP_READ)) { gst_sample_unref(sample); continue; }
    bool cut=false;
    const auto &colors=sampler.sample(static_cast<const uint8_t *>(GST_VIDEO_FRAME_PLANE_DATA(&frame,0)),
      GST_VIDEO_FRAME_PLANE_STRIDE(&frame,0),GST_VIDEO_FRAME_WIDTH(&frame),GST_VIDEO_FRAME_HEIGHT(&frame),&cut,&raw);
    GstClockTime pts=GST_BUFFER_PTS(buffer);
    double pts_ms=-1, pts_delta_ms=-1, arrival_delta_ms=-1;
    if(GST_CLOCK_TIME_IS_VALID(pts)) {
      if(!GST_CLOCK_TIME_IS_VALID(first_pts)) first_pts=pts;
      if(pts>=first_pts) pts_ms=double(pts-first_pts)/GST_MSECOND;
      if(GST_CLOCK_TIME_IS_VALID(previous_pts)) {
        if(pts>previous_pts) {
          pts_delta_ms=double(pts-previous_pts)/GST_MSECOND;
          pts_intervals.push_back(pts_delta_ms);
          if(pts_delta_ms>expected_ms*1.5) ++pts_gaps;
        } else ++pts_regressions;
      }
      previous_pts=pts;
    }
    if(previous_arrival) {
      arrival_delta_ms=double(arrival-previous_arrival)/1000.0;
      arrival_intervals.push_back(arrival_delta_ms);
      if(arrival_delta_ms>expected_ms*1.5) ++arrival_gaps;
    }
    previous_arrival=arrival;
    if(cut) ++scene_cuts;
    const Rect &active=sampler.active;
    if(have_active && (active.x0!=previous_active.x0 || active.x1!=previous_active.x1 || active.y0!=previous_active.y0 || active.y1!=previous_active.y1)) ++active_changes;
    previous_active=active; have_active=true;
    std::printf("frame=%04zu pts_ms=%.1f dpts_ms=%.1f arrival_ms=%.1f cut=%d active=%.3f,%.3f,%.3f,%.3f zones=",
      ++frames,pts_ms,pts_delta_ms,arrival_delta_ms,cut?1:0,active.x0,active.x1,active.y0,active.y1);
    for(size_t i=0;i<zones.size();++i) {
      const Color &r=raw[i], &o=colors[i];
      std::printf("%s%u:%ld,%ld,%ld>%ld,%ld,%ld",i?" ":"",unsigned(zones[i].id),
        std::lround(r.r),std::lround(r.g),std::lround(r.b),std::lround(o.r),std::lround(o.g),std::lround(o.b));
    }
    std::putchar('\n');
    gst_video_frame_unmap(&frame); gst_sample_unref(sample);
  }
  int64_t elapsed=g_get_monotonic_time()-started; stop_capture(cap);
  auto percentiles=[](std::vector<double> &v) {
    if(v.empty()) return std::pair<double,double>{0,0};
    std::sort(v.begin(),v.end());
    return std::pair<double,double>{v[(v.size()-1)/2],v[(v.size()-1)*95/100]};
  };
  auto pts=percentiles(pts_intervals), arrival=percentiles(arrival_intervals);
  std::printf("# summary frames=%zu seconds=%.2f fps=%.1f pts_ms_median/p95=%.2f/%.2f arrival_ms_median/p95=%.2f/%.2f pts_gaps=%zu arrival_gaps=%zu pts_regressions=%zu scene_cuts=%zu active_rect_changes=%zu smooth=%.2f max_step=%d brightness=%.2f edge_depth=%d%% center_bleed=%d%% peak_mix=%d%% feather_depth=%d%%\n",
    frames,elapsed/1e6,elapsed?frames*1e6/elapsed:0.0,pts.first,pts.second,arrival.first,arrival.second,
    pts_gaps,arrival_gaps,pts_regressions,scene_cuts,active_changes,cfg.smooth,cfg.max_step,cfg.brightness,cfg.edge_depth,cfg.center_bleed,cfg.peak_mix,cfg.feather_depth);
  return frames ? 0 : 1;
}
struct Control {
  Config &cfg;
  const std::vector<PanelPosition> &panels;
  std::vector<Zone> &zones;
  Sampler &sampler;
  Capture &capture;
  Config pending;
  bool has_pending = false;
  GDBusConnection *bus = nullptr;
  guint registration = 0;
};
constexpr const char *kBusName = "org.nanoleaf.NanoleafSync";
constexpr const char *kObjectPath = "/org/nanoleaf/NanoleafSync";
const char *kDbusXml =
  "<node><interface name='org.nanoleaf.NanoleafSync'>"
  "<method name='GetSettings'><arg type='a{sv}' direction='out'/></method>"
  "<method name='SetSettings'><arg type='a{sv}' direction='in'/></method>"
  "<method name='Stop'/>"
  "</interface></node>";
void add_settings(GVariantBuilder &b, const Config &c) {
  g_variant_builder_add(&b, "{sv}", "brightness", g_variant_new_double(c.brightness));
  g_variant_builder_add(&b, "{sv}", "saturation_boost", g_variant_new_double(c.saturation_boost));
  g_variant_builder_add(&b, "{sv}", "fps", g_variant_new_int32(c.fps));
  g_variant_builder_add(&b, "{sv}", "smooth", g_variant_new_double(c.smooth));
  g_variant_builder_add(&b, "{sv}", "black_threshold", g_variant_new_double(c.black_threshold));
  g_variant_builder_add(&b, "{sv}", "max_step", g_variant_new_int32(c.max_step));
  g_variant_builder_add(&b, "{sv}", "edge_depth", g_variant_new_int32(c.edge_depth));
  g_variant_builder_add(&b, "{sv}", "center_bleed", g_variant_new_int32(c.center_bleed));
  g_variant_builder_add(&b, "{sv}", "peak_mix", g_variant_new_int32(c.peak_mix));
  g_variant_builder_add(&b, "{sv}", "feather_depth", g_variant_new_int32(c.feather_depth));
  g_variant_builder_add(&b, "{sv}", "corner", g_variant_new_string(c.corner.c_str()));
  g_variant_builder_add(&b, "{sv}", "offset", g_variant_new_int32(c.offset));
  g_variant_builder_add(&b, "{sv}", "reverse", g_variant_new_boolean(c.reverse));
  g_variant_builder_add(&b, "{sv}", "letterbox", g_variant_new_boolean(c.letterbox));
  g_variant_builder_add(&b, "{sv}", "midtone_lift", g_variant_new_boolean(c.midtone_lift));
  g_variant_builder_add(&b, "{sv}", "strict_black", g_variant_new_boolean(c.strict_black));
}
void invalid_settings(GDBusMethodInvocation *invocation, const char *key) {
  std::string message = std::string("invalid or unsupported setting: ") + key;
  g_dbus_method_invocation_return_dbus_error(invocation, "org.nanoleaf.NanoleafSync.InvalidSettings", message.c_str());
}
void dbus_method_call(GDBusConnection *, const gchar *, const gchar *, const gchar *, const gchar *method,
                     GVariant *parameters, GDBusMethodInvocation *invocation, gpointer data) {
  auto &control = *static_cast<Control *>(data);
  if (g_str_equal(method, "GetSettings")) {
    GVariantBuilder b; g_variant_builder_init(&b, G_VARIANT_TYPE_VARDICT); add_settings(b, control.has_pending ? control.pending : control.cfg);
    g_dbus_method_invocation_return_value(invocation, g_variant_new("(@a{sv})", g_variant_builder_end(&b)));
    return;
  }
  if (g_str_equal(method, "Stop")) {
    running = 0;
    g_dbus_method_invocation_return_value(invocation, g_variant_new("()"));
    return;
  }
  Config next = control.has_pending ? control.pending : control.cfg;
  GVariant *settings = g_variant_get_child_value(parameters, 0);
  GVariantIter iter; g_variant_iter_init(&iter, settings);
  const gchar *key; GVariant *value; bool valid = true; std::string bad = "value";
  while (g_variant_iter_next(&iter, "{&sv}", &key, &value)) {
    bad = key;
    if (g_str_equal(key, "brightness") && g_variant_is_of_type(value, G_VARIANT_TYPE_DOUBLE)) next.brightness = static_cast<float>(g_variant_get_double(value));
    else if (g_str_equal(key, "saturation_boost") && g_variant_is_of_type(value, G_VARIANT_TYPE_DOUBLE)) next.saturation_boost = static_cast<float>(g_variant_get_double(value));
    else if (g_str_equal(key, "fps") && g_variant_is_of_type(value, G_VARIANT_TYPE_INT32)) next.fps = g_variant_get_int32(value);
    else if (g_str_equal(key, "smooth") && g_variant_is_of_type(value, G_VARIANT_TYPE_DOUBLE)) next.smooth = static_cast<float>(g_variant_get_double(value));
    else if (g_str_equal(key, "black_threshold") && g_variant_is_of_type(value, G_VARIANT_TYPE_DOUBLE)) next.black_threshold = static_cast<float>(g_variant_get_double(value));
    else if (g_str_equal(key, "max_step") && g_variant_is_of_type(value, G_VARIANT_TYPE_INT32)) next.max_step = g_variant_get_int32(value);
    else if (g_str_equal(key, "edge_depth") && g_variant_is_of_type(value, G_VARIANT_TYPE_INT32)) next.edge_depth = g_variant_get_int32(value);
    else if (g_str_equal(key, "center_bleed") && g_variant_is_of_type(value, G_VARIANT_TYPE_INT32)) next.center_bleed = g_variant_get_int32(value);
    else if (g_str_equal(key, "peak_mix") && g_variant_is_of_type(value, G_VARIANT_TYPE_INT32)) next.peak_mix = g_variant_get_int32(value);
    else if (g_str_equal(key, "feather_depth") && g_variant_is_of_type(value, G_VARIANT_TYPE_INT32)) next.feather_depth = g_variant_get_int32(value);
    else if (g_str_equal(key, "corner") && g_variant_is_of_type(value, G_VARIANT_TYPE_STRING)) next.corner = g_variant_get_string(value, nullptr);
    else if (g_str_equal(key, "offset") && g_variant_is_of_type(value, G_VARIANT_TYPE_INT32)) next.offset = g_variant_get_int32(value);
    else if (g_str_equal(key, "reverse") && g_variant_is_of_type(value, G_VARIANT_TYPE_BOOLEAN)) next.reverse = g_variant_get_boolean(value);
    else if (g_str_equal(key, "letterbox") && g_variant_is_of_type(value, G_VARIANT_TYPE_BOOLEAN)) next.letterbox = g_variant_get_boolean(value);
    else if (g_str_equal(key, "midtone_lift") && g_variant_is_of_type(value, G_VARIANT_TYPE_BOOLEAN)) next.midtone_lift = g_variant_get_boolean(value);
    else if (g_str_equal(key, "strict_black") && g_variant_is_of_type(value, G_VARIANT_TYPE_BOOLEAN)) next.strict_black = g_variant_get_boolean(value);
    else valid = false;
    g_variant_unref(value);
    if (!valid) break;
  }
  g_variant_unref(settings);
  if (valid) valid = next.center_bleed >= 0 && next.center_bleed <= 50 && next.peak_mix >= 0 && next.peak_mix <= 35 && next.feather_depth >= 0 && next.feather_depth <= 20;
  if (valid) valid = next.fps >= 1 && next.fps <= 60 && next.edge_depth >= 5 && next.edge_depth <= 20 && next.smooth >= 0.05f && next.smooth <= 1 &&
    std::isfinite(next.smooth) && next.black_threshold >= 0 && next.black_threshold <= 0.1f &&
    std::isfinite(next.black_threshold) && next.brightness >= 0 && next.brightness <= 2 &&
    std::isfinite(next.brightness) && next.saturation_boost >= 0 && next.saturation_boost <= 3 &&
    std::isfinite(next.saturation_boost) && next.max_step >= 1 && next.max_step <= 255 &&
    next.offset >= 0 && next.offset <= 39 &&
    (next.corner == "bottom-center" || next.corner == "bottom-left" || next.corner == "top-left" || next.corner == "top-right" || next.corner == "bottom-right");
  if (!valid) { invalid_settings(invocation, bad.c_str()); return; }
  control.pending = std::move(next); control.has_pending = true;
  g_dbus_method_invocation_return_value(invocation, g_variant_new("()"));
}
const GDBusInterfaceVTable kDbusVtable = {dbus_method_call, nullptr, nullptr, {nullptr}};
void claim_dbus_name(GDBusConnection *bus) {
  GError *error = nullptr;
  GVariant *reply = g_dbus_connection_call_sync(bus, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "RequestName",
      g_variant_new("(su)", kBusName, 4u), G_VARIANT_TYPE("(u)"), G_DBUS_CALL_FLAGS_NONE, 5000, nullptr, &error);
  if (!reply) { std::string message = error ? error->message : "cannot claim D-Bus name"; g_clear_error(&error); fail(message); }
  guint32 result; g_variant_get(reply, "(u)", &result); g_variant_unref(reply);
  if (result != 1) fail("another Nanoleaf Sync worker already owns the D-Bus service");
}
void apply_pending(Control &control) {
  if (!control.has_pending) return;
  Config next = std::move(control.pending); control.has_pending = false;
  int old_fps = control.cfg.fps;
  auto zones = make_zones(control.panels, next);
  control.sampler.remap(zones); control.zones = std::move(zones); control.cfg = std::move(next);
  if (control.cfg.fps != old_fps) {
    g_object_set(control.capture.rate, "max-rate", control.cfg.fps, nullptr);
    GstCaps *caps = gst_caps_new_simple("video/x-raw", "framerate", GST_TYPE_FRACTION, control.cfg.fps, 1, nullptr);
    g_object_set(control.capture.fps_caps, "caps", caps, nullptr); gst_caps_unref(caps);
    gst_element_send_event(control.capture.fps_caps, gst_event_new_reconfigure());
  }
  save_config(control.cfg);
}
void unregister_control(Control &control, GDBusNodeInfo *introspection) {
  if (control.registration) g_dbus_connection_unregister_object(control.bus, control.registration);
  g_dbus_connection_call_sync(control.bus, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus", "ReleaseName",
      g_variant_new("(s)", kBusName), G_VARIANT_TYPE("(u)"), G_DBUS_CALL_FLAGS_NONE, 2000, nullptr, nullptr);
  g_dbus_node_info_unref(introspection); g_object_unref(control.bus);
}
void run(Config &cfg) {
  std::signal(SIGINT,stop_signal); std::signal(SIGTERM,stop_signal);
  GError *error=nullptr; GDBusConnection *bus=g_bus_get_sync(G_BUS_TYPE_SESSION,nullptr,&error);
  if(!bus){std::string message=error?error->message:"session bus unavailable";g_clear_error(&error);fail(message);}
  auto panels=panel_layout(cfg); auto zones=make_zones(panels,cfg); Capture cap; Sampler sampler(zones,cfg);
  GDBusNodeInfo *introspection=g_dbus_node_info_new_for_xml(kDbusXml,&error);
  if(!introspection){std::string message=error?error->message:"invalid D-Bus introspection";g_clear_error(&error);fail(message);}
  Control control{cfg,panels,zones,sampler,cap,{},false,bus,0};
  control.registration=g_dbus_connection_register_object(bus,kObjectPath,introspection->interfaces[0],&kDbusVtable,&control,nullptr,&error);
  if(!control.registration){std::string message=error?error->message:"cannot export D-Bus interface";g_clear_error(&error);fail(message);}
  claim_dbus_name(bus);
  cap=start_capture(cfg);
  if(!running){stop_capture(cap);unregister_control(control,introspection);return;}
  int port=enable_stream(cfg); int out=udp_socket(cfg.host,port);
  int64_t interval=1000000/cfg.fps,next=0;
  std::fprintf(stderr,"Streaming %zu Nanoleaf 4D segments to %s:%d; Ctrl-C or D-Bus Stop to stop\n",zones.size(),cfg.host.c_str(),port);
  Packet packet;
  while(running){
    GstSample *sample=gst_app_sink_try_pull_sample(cap.sink,100*GST_MSECOND);
    while(g_main_context_iteration(nullptr,FALSE)) {}
    int old_fps=cfg.fps; apply_pending(control);
    if(cfg.fps!=old_fps){interval=1000000/cfg.fps;next=0;}
    if(!sample) continue;
    int64_t now=g_get_monotonic_time(); if(now<next){gst_sample_unref(sample);continue;} next=now+interval;
    GstCaps *caps=gst_sample_get_caps(sample); GstVideoInfo info; gst_video_info_init(&info);
    GstBuffer *buffer=gst_sample_get_buffer(sample);
    if(!caps || !buffer || !gst_video_info_from_caps(&info,caps)){gst_sample_unref(sample);continue;}
    GstVideoFrame frame; if(!gst_video_frame_map(&frame,&info,buffer,GST_MAP_READ)){gst_sample_unref(sample);continue;}
    auto *data=static_cast<const uint8_t *>(GST_VIDEO_FRAME_PLANE_DATA(&frame,0));
    bool cut=false; const auto &colors=sampler.sample(data,GST_VIDEO_FRAME_PLANE_STRIDE(&frame,0),GST_VIDEO_FRAME_WIDTH(&frame),GST_VIDEO_FRAME_HEIGHT(&frame),&cut);
    gst_video_frame_unmap(&frame); gst_sample_unref(sample);
    make_packet(zones,colors,packet); send_packet(out,packet,zones.size());
  }
  stop_capture(cap); close(out); unregister_control(control,introspection);
}

void self_test() {
  Config defaults;
  assert(defaults.brightness==1.5f && defaults.saturation_boost==1.5f && defaults.fps==30 && defaults.smooth==0.35f && defaults.black_threshold==0.01f && defaults.max_step==12);
  assert(defaults.edge_depth==8 && defaults.center_bleed==0 && defaults.peak_mix==15 && defaults.feather_depth==5);
  assert(defaults.corner=="bottom-center" && defaults.offset==0 && !defaults.reverse && defaults.letterbox && !defaults.midtone_lift && !defaults.strict_black);
  Config c; c.smooth=1; c.max_step=255; c.black_threshold=0.02f;
  std::vector<PanelPosition> panels40(kPanels); for(int i=0;i<kPanels;++i) panels40[i]={uint16_t(i+100),0,0};
  auto z=make_zones(panels40,c); assert(z.size()==40 && z[0].id==100 && z[7].id==107);
  assert(c.edge_depth==8 && std::abs(z[0].y0-0.92f)<0.001f);
  Config narrow=c; narrow.edge_depth=5; auto zn=make_zones(panels40,narrow); assert(std::abs(zn[0].y0-0.95f)<0.001f);
  Config wide=c; wide.edge_depth=20; auto zw=make_zones(panels40,wide); assert(std::abs(zw[0].y0-0.80f)<0.001f);
  c.corner="top-left"; c.offset=2; c.reverse=true; auto a=make_zones(panels40,c); assert(a[0].id==117 && a[1].id==116);
  std::vector<PanelPosition> panels34(34);
  for(int i=0;i<7;++i) panels34[i]={uint16_t(i),300-50*i,50};
  for(int i=7;i<12;++i) panels34[i]={uint16_t(i),0,50+50*(i-6)};
  for(int i=12;i<21;++i) panels34[i]={uint16_t(i),50*(i-11),300};
  for(int i=21;i<27;++i) panels34[i]={uint16_t(i),450,300-50*(i-20)};
  for(int i=27;i<34;++i) panels34[i]={uint16_t(i),450-50*(i-26),0};
  Config c34_default; auto z34_default=make_zones(panels34,c34_default);
  assert(z34_default[0].id==0 && std::abs((z34_default[0].x0+z34_default[0].x1)*0.5f-2.0f/3.0f)<0.001f);
  Config c34; c34.corner="bottom-left"; auto z34=make_zones(panels34,c34);
  assert(z34.size()==34 && z34[0].id==6 && z34[33].id==5 && std::abs(z34[0].y0-0.92f)<0.001f);
  assert(z34[6].x0==0 && z34[11].x0==0 && z34[12].y0==0 && z34[20].y0==0 && z34[21].x0>0.8f && z34[26].x0>0.8f && z34[27].y0>0.8f);
  Config c34_tl=c34; c34_tl.corner="top-left"; assert(make_zones(panels34,c34_tl)[0].id==11);
  Config c34_tr=c34; c34_tr.corner="top-right"; assert(make_zones(panels34,c34_tr)[0].id==20);
  Config c34_br=c34; c34_br.corner="bottom-right"; assert(make_zones(panels34,c34_br)[0].id==26);
  c34.offset=39; auto z34_offset=make_zones(panels34,c34); assert(z34_offset[0].id==11 && z34_offset[33].id==10);
  constexpr int w=320,h=180,stride=w*4; std::vector<uint8_t> frame(stride*h);
  for(int y=0;y<h;++y) for(int x=0;x<w;++x){size_t o=y*stride+x*4; frame[o]=x<w/2?220:20; frame[o+1]=20; frame[o+2]=x<w/2?20:220; frame[o+3]=255;}
  Sampler s(z,c); bool cut=false; std::vector<Color> raw(z.size());
  const auto &colors=s.sample(frame.data(),stride,w,h,&cut,&raw); assert(!cut); assert(colors[11].r>colors[11].b); assert(colors[30].b>colors[30].r); assert(raw[11].r==colors[11].r && raw[30].b==colors[30].b);
  std::this_thread::sleep_for(std::chrono::milliseconds(40)); std::fill(frame.begin(),frame.end(),0); s.sample(frame.data(),stride,w,h,&cut,&raw); assert(raw[11].r==0 && raw[30].b==0 && colors[11].r==0 && colors[30].b==0);
  assert(c.edge_depth==8 && c.center_bleed==0 && c.peak_mix==15 && c.feather_depth==5);
  Config synthetic; synthetic.letterbox=false; synthetic.smooth=1; synthetic.max_step=255; synthetic.black_threshold=0; synthetic.saturation_boost=0; synthetic.brightness=1;
  std::vector<Zone> corner_zone{{900,0,0.15f,0,0.15f}};
  std::fill(frame.begin(),frame.end(),0);
  for(int y=h/4;y<3*h/4;++y) for(int x=w/4;x<3*w/4;++x){size_t o=y*stride+x*4; frame[o]=100; frame[o+1]=40; frame[o+2]=10;}
  Config center_cfg=synthetic; center_cfg.center_bleed=20; center_cfg.peak_mix=0; center_cfg.feather_depth=0;
  Sampler center_sampler(corner_zone,center_cfg); std::vector<Color> center_raw(1); center_sampler.sample(frame.data(),stride,w,h,&cut,&center_raw);
  assert(std::abs(center_raw[0].r-20)<0.1f && std::abs(center_raw[0].g-8)<0.1f && std::abs(center_raw[0].b-2)<0.1f);
  std::fill(frame.begin(),frame.end(),0); frame[0]=frame[1]=frame[2]=255;
  Config peak_off=synthetic; peak_off.center_bleed=0; peak_off.peak_mix=0; peak_off.feather_depth=0;
  Sampler peak_off_sampler(corner_zone,peak_off); std::vector<Color> peak_off_raw(1); peak_off_sampler.sample(frame.data(),stride,w,h,&cut,&peak_off_raw);
  Config peak_on=peak_off; peak_on.peak_mix=15;
  Sampler peak_on_sampler(corner_zone,peak_on); std::vector<Color> peak_on_raw(1); peak_on_sampler.sample(frame.data(),stride,w,h,&cut,&peak_on_raw);
  assert(peak_off_raw[0].r<1 && peak_on_raw[0].r>38 && peak_on_raw[0].r<40);
  std::vector<Zone> side_zone{{901,0,0.15f,0.4f,0.6f}};
  Config feather_cfg=synthetic; feather_cfg.center_bleed=0; feather_cfg.peak_mix=0; feather_cfg.feather_depth=10;
  std::fill(frame.begin(),frame.end(),0); for(int y=72;y<108;y+=2){size_t o=y*stride+64*4; frame[o]=frame[o+1]=frame[o+2]=255;}
  Sampler feather_mid_sampler(side_zone,feather_cfg); std::vector<Color> feather_mid(1); feather_mid_sampler.sample(frame.data(),stride,w,h,&cut,&feather_mid);
  std::fill(frame.begin(),frame.end(),0); for(int y=72;y<108;y+=2){size_t o=y*stride+78*4; frame[o]=frame[o+1]=frame[o+2]=255;}
  Sampler feather_outer_sampler(side_zone,feather_cfg); std::vector<Color> feather_outer(1); feather_outer_sampler.sample(frame.data(),stride,w,h,&cut,&feather_outer);
  Config no_feather=feather_cfg; no_feather.feather_depth=0;
  Sampler hard_edge_sampler(side_zone,no_feather); std::vector<Color> hard_edge(1); hard_edge_sampler.sample(frame.data(),stride,w,h,&cut,&hard_edge);
  assert(feather_mid[0].r>feather_outer[0].r && feather_outer[0].r>0 && hard_edge[0].r==0);
  std::vector<Zone> corner_zone_for_feather{{902,0,0.15f,0,0.15f}};
  std::fill(frame.begin(),frame.end(),0); size_t horizontal_pixel=8*stride+64*4; frame[horizontal_pixel]=frame[horizontal_pixel+1]=frame[horizontal_pixel+2]=255;
  Sampler corner_horizontal_sampler(corner_zone_for_feather,feather_cfg); std::vector<Color> corner_horizontal(1); corner_horizontal_sampler.sample(frame.data(),stride,w,h,&cut,&corner_horizontal);
  std::fill(frame.begin(),frame.end(),0); size_t diagonal_pixel=36*stride+64*4; frame[diagonal_pixel]=frame[diagonal_pixel+1]=frame[diagonal_pixel+2]=255;
  Sampler corner_diagonal_sampler(corner_zone_for_feather,feather_cfg); std::vector<Color> corner_diagonal(1); corner_diagonal_sampler.sample(frame.data(),stride,w,h,&cut,&corner_diagonal);
  assert(corner_horizontal[0].r>corner_diagonal[0].r*1.8f && corner_diagonal[0].r>0);
  Packet packet{};
  std::vector<Color> colors34(34,Color{12,34,56}); make_packet(z34,colors34,packet); assert(packet[1]==34);
  int recvfd=socket(AF_INET,SOCK_DGRAM,0); assert(recvfd>=0); sockaddr_in addr{}; addr.sin_family=AF_INET; addr.sin_addr.s_addr=htonl(INADDR_LOOPBACK); assert(bind(recvfd,reinterpret_cast<sockaddr*>(&addr),sizeof(addr))==0); socklen_t len=sizeof(addr); assert(getsockname(recvfd,reinterpret_cast<sockaddr*>(&addr),&len)==0);
  timeval timeout{1,0}; setsockopt(recvfd,SOL_SOCKET,SO_RCVTIMEO,&timeout,sizeof(timeout));
  int sendfd=udp_socket("127.0.0.1",ntohs(addr.sin_port)); std::vector<Color> wire_colors(kPanels,Color{12,34,56}); make_packet(z,wire_colors,packet); send_packet(sendfd,packet,z.size());
  std::array<uint8_t,2+kPanels*8> received{}; ssize_t bytes=recv(recvfd,received.data(),received.size(),0); assert(bytes==322 && received[1]==40);
  make_packet(z34,colors34,packet); send_packet(sendfd,packet,z34.size()); bytes=recv(recvfd,received.data(),received.size(),0); assert(bytes==274 && received[1]==34);
  close(sendfd); close(recvfd); std::puts("self-test: edge depth, center blend, peak attenuation, feather falloff, 40/34-zone alignment, RGB/black sampling, and UDP frames OK");
}
}

int main(int argc,char **argv){
  Config initial; initial.config_path = std::string(g_get_user_config_dir()) + "/nanoleaf-sync/config.json";
  bool self_test_arg = false;
  for (int i = 1; i < argc; ++i) {
    if (std::string(argv[i]) == "--self-test") self_test_arg = true;
    if (std::string(argv[i]) == "--config" && i + 1 < argc) initial.config_path = argv[i + 1];
  }
  if (!self_test_arg) load_config(initial);
  bool test=false, capture_only=false, capture_debug_arg=false, pair=false;
  Config c=arguments(argc,argv,&test,&capture_only,&capture_debug_arg,&pair,initial);
  if(test){self_test();return 0;}
  gst_init(&argc,&argv);
  if(capture_only) return capture_test(c);
  if(capture_debug_arg) {
    if(c.host.empty() || c.token.empty()) fail("config or --ip and --token are required for --capture-debug");
    if(curl_global_init(CURL_GLOBAL_DEFAULT)!=CURLE_OK) fail("libcurl initialization failed");
    int result=capture_debug(c); curl_global_cleanup(); return result;
  }
  if(curl_global_init(CURL_GLOBAL_DEFAULT)!=CURLE_OK) fail("libcurl initialization failed");
  if(pair) {
    if(c.host.empty()) { usage(argv[0]); fail("--ip is required to pair"); }
    pair_device(c); curl_global_cleanup(); return 0;
  }
  if(c.host.empty() || c.token.empty()) { usage(argv[0]); fail("config or --ip and --token are required"); }
  run(c); curl_global_cleanup(); return 0;
}
