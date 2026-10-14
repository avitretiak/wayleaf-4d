#include <QDBusConnection>
#include <QDBusConnectionInterface>
#include <QDBusInterface>
#include <QDBusMessage>
#include <QDBusPendingCallWatcher>
#include <QDBusPendingReply>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonDocument>
#include <functional>
#include <QJsonObject>
#include <QJsonParseError>
#include <QProcess>
#include <QQmlExtensionPlugin>
#include <QSaveFile>
#include <QStandardPaths>
#include <QTimer>
#include <QDBusObjectPath>
#include <arpa/inet.h>
#include <QVariantMap>
#include <QtQml>
#include <cmath>

namespace {
constexpr auto Service = "io.github.avitretiak.Wayleaf4D";
constexpr auto ObjectPath = "/io/github/avitretiak/Wayleaf4D";
constexpr auto Interface = "io.github.avitretiak.Wayleaf4D";
constexpr auto AvahiService = "org.freedesktop.Avahi";
constexpr auto AvahiServerPath = "/";
constexpr auto AvahiServerInterface = "org.freedesktop.Avahi.Server";
constexpr auto AvahiBrowserInterface = "org.freedesktop.Avahi.ServiceBrowser";
constexpr auto NanoleafServiceType = "_nanoleafapi._tcp";

QVariantMap defaults() {
  return {{"brightness", 1.5}, {"saturation_boost", 1.5}, {"fps", 10},
          {"smooth", 0.35}, {"black_threshold", 0.01}, {"max_step", 12}, {"edge_depth", 8},
          {"center_bleed", 0}, {"peak_mix", 15}, {"feather_depth", 5},
          {"corner", "bottom-center"}, {"offset", 0}, {"reverse", false},
          {"letterbox", true}, {"midtone_lift", false}, {"strict_black", false}};
}
const QStringList boolKeys{"reverse", "letterbox", "midtone_lift", "strict_black"};
const QStringList intKeys{"fps", "max_step", "offset", "edge_depth", "center_bleed", "peak_mix", "feather_depth"};
const QStringList doubleKeys{"brightness", "saturation_boost", "smooth", "black_threshold"};
const QStringList settingKeys = boolKeys + intKeys + doubleKeys + QStringList{"corner"};
QString configPath() {
  return QStandardPaths::writableLocation(QStandardPaths::ConfigLocation) + "/nanoleaf-sync/config.json";
}

class Backend : public QObject {
  Q_OBJECT
  Q_PROPERTY(QVariantMap settings READ settings NOTIFY settingsChanged)
  Q_PROPERTY(QString status READ status NOTIFY statusChanged)
  Q_PROPERTY(QString error READ error NOTIFY errorChanged)
  Q_PROPERTY(bool enabled READ enabled NOTIFY statusChanged)
  Q_PROPERTY(bool busy READ busy NOTIFY statusChanged)
  Q_PROPERTY(bool deviceConnected READ deviceConnected NOTIFY deviceChanged)
  Q_PROPERTY(bool deviceBusy READ deviceBusy NOTIFY deviceChanged)
  Q_PROPERTY(QString deviceHost READ deviceHost NOTIFY deviceChanged)
  Q_PROPERTY(QVariantMap deviceInfo READ deviceInfo NOTIFY deviceChanged)
  Q_PROPERTY(QVariantList discoveredDevices READ discoveredDevices NOTIFY deviceChanged)
  Q_PROPERTY(QString deviceMessage READ deviceMessage NOTIFY deviceChanged)

 public:
  explicit Backend(QObject *parent = nullptr) : QObject(parent), process_(this), serviceProbe_(this), deviceProcess_(this), discoveryTimer_(this) {
    settings_ = readSettings();
    loadDeviceConfig();
    poll_.setInterval(2000);
    saveTimer_.setSingleShot(true);
    connect(&saveTimer_, &QTimer::timeout, this, [this] {
      if (busy_ || deviceBusy_ || (status_ == "starting" && !workerPresent_)) return;
      if (workerPresent_) applyLiveSettings();
      else if (pendingSettings_) saveSettings();
    });
    connect(&poll_, &QTimer::timeout, this, &Backend::refresh);
    connect(&serviceProbe_, &QProcess::finished, this, [this](int code, QProcess::ExitStatus exit) {
      if (!enabled_ || busy_) return;
      if (exit == QProcess::NormalExit && code == 0) setStatus("starting");
      else {
        setStatus("error");
        setError("Wayleaf 4D service failed; inspect journalctl --user -u nanoleaf-sync.service");
      }
    });
    connect(&process_, &QProcess::finished, this, &Backend::systemctlFinished);
    discoveryTimer_.setSingleShot(true);
    connect(&discoveryTimer_, &QTimer::timeout, this, &Backend::finishDiscovery);
    connect(&deviceProcess_, &QProcess::finished, this, &Backend::deviceProcessFinished);
    connect(&deviceProcess_, &QProcess::errorOccurred, this, &Backend::deviceProcessError);
    connect(&process_, &QProcess::errorOccurred, this, [this](QProcess::ProcessError) {
      if (process_.state() == QProcess::NotRunning && process_.error() == QProcess::FailedToStart) {
        commandDone_ = {};
        fail("Could not start systemctl");
      }
    });
    poll_.start();
    refresh();
    runSystemctl({"--user", "is-enabled", "nanoleaf-sync.service"}, [this](int code, const QString &out) {
      if (!userSetEnabled_) setEnabledState(code == 0 && out.trimmed() == "enabled");
    });
  }

  QVariantMap settings() const { return settings_; }
  QString status() const { return status_; }
  QString error() const { return error_; }
  bool enabled() const { return enabled_; }
  bool busy() const { return busy_; }
  bool deviceConnected() const { return deviceConnected_; }
  bool deviceBusy() const { return deviceBusy_; }
  QString deviceHost() const { return deviceHost_; }
  QVariantMap deviceInfo() const { return deviceInfo_; }
  QVariantList discoveredDevices() const { return discoveredDevices_; }
  QString deviceMessage() const { return deviceMessage_; }

  Q_INVOKABLE void clearError() { setError({}); }
  Q_INVOKABLE void refresh() {
    if (pollInFlight_) return;
    pollInFlight_ = true;
    const auto bus = QDBusConnection::sessionBus();
    QDBusMessage ownerQuery = QDBusMessage::createMethodCall(
        QStringLiteral("org.freedesktop.DBus"), QStringLiteral("/org/freedesktop/DBus"),
        QStringLiteral("org.freedesktop.DBus"), QStringLiteral("NameHasOwner"));
    ownerQuery << QString::fromLatin1(Service);
    auto *watcher = new QDBusPendingCallWatcher(bus.asyncCall(ownerQuery), this);
    connect(watcher, &QDBusPendingCallWatcher::finished, this, [this, bus](QDBusPendingCallWatcher *w) {
      QDBusPendingReply<bool> reply = *w;
      w->deleteLater();
      if (reply.isError()) { pollInFlight_ = false; return; }
      workerPresent_ = reply.value();
      if (!workerPresent_) {
        pollInFlight_ = false;
        if (busy_) return;
        if (enabled_ && serviceProbe_.state() == QProcess::NotRunning)
          serviceProbe_.start(QStringLiteral("systemctl"), {"--user", "is-active", "nanoleaf-sync.service"});
        else if (!enabled_) setStatus("stopped");
        return;
      }
      loadLiveSettings(bus);
      QDBusMessage msg = QDBusMessage::createMethodCall(Service, ObjectPath, Interface, "GetStatus");
      auto *statusWatcher = new QDBusPendingCallWatcher(bus.asyncCall(msg), this);
      connect(statusWatcher, &QDBusPendingCallWatcher::finished, this, [this](QDBusPendingCallWatcher *sw) {
        QDBusPendingReply<QString> statusReply = *sw;
        sw->deleteLater();
        pollInFlight_ = false;
        if (busy_) return;
        if (statusReply.isValid()) {
          const QString s = statusReply.value();
          if (s == "starting" || s == "running" || s == "stopping") {
            setStatus(s);
            if (pendingSettings_ && s == "running") applyLiveSettings();
          } else {
            setStatus("error");
            setError("Worker reported an unknown status; inspect journalctl --user -u nanoleaf-sync.service");
          }
        } else if (statusReply.error().name() == "org.freedesktop.DBus.Error.UnknownMethod") {
          setStatus("starting");
        } else {
          setStatus("error");
          setError("Could not query worker status; inspect journalctl --user -u nanoleaf-sync.service");
        }
      });
    });
  }

  Q_INVOKABLE void setSetting(const QString &key, const QVariant &value) {
    if (busy_ || deviceBusy_ || !settingKeys.contains(key)) return;
    QVariantMap next = settings_;
    next.insert(key, value);
    next = normalize(next);
    if (next == settings_) return;
    settings_ = next;
    emit settingsChanged();
    pendingSettings_ = true;
    ++settingsGeneration_;
    saveTimer_.start(250);
  }

  Q_INVOKABLE void setEnabled(bool on) {
    if (busy_ || deviceBusy_ || (on && on == enabled_)) return;
    clearError();
    userSetEnabled_ = true;
    busy_ = true;
    setStatus(on ? "starting" : "stopping");
    emit statusChanged();
    if (!on) {
      saveTimer_.stop();
      stopWorkerThenDisable();
      return;
    }
    // A legacy worker can own the name before the systemd unit is installed/enabled.
    queryWorker([this](bool present) {
      if (present) runSystemctl({"--user", "enable", "nanoleaf-sync.service"}, [this](int code, const QString &) {
        if (code) fail("Could not enable nanoleaf-sync.service");
        else { setEnabledState(true); finishOperation(true); refresh(); }
      });
      else runSystemctl({"--user", "enable", "--now", "nanoleaf-sync.service"}, [this](int code, const QString &) {
        if (code) fail("Could not enable and start nanoleaf-sync.service");
        else { setEnabledState(true); finishOperation(true); refresh(); }
      });
    });
  }
  Q_INVOKABLE void changeCapture() {
    if (busy_ || deviceBusy_ || !enabled_ || (status_ != "running" && status_ != "error")) return;
    clearError();
    bool valid;
    loadJson(&valid);
    if (!valid) { fail("Cannot change capture because config.json is invalid or unreadable"); return; }
    saveTimer_.stop();
    busy_ = true;
    setStatus("stopping");
    emit statusChanged();
    queryWorker([this](bool present) {
      workerPresent_ = present;
      if (pendingSettings_ && present) {
        applyLiveSettings([this](bool success) {
          if (success) stopForCaptureChange(true);
          else finishOperation(false);
        });
      } else {
        if (pendingSettings_ && !saveSettings()) { finishOperation(false); return; }
        stopForCaptureChange(present);
      }
    });
  }
  Q_INVOKABLE void discoverDevices() { startDiscovery(); }
  Q_INVOKABLE void pairDevice(const QString &host) { startPairing(host); }
  Q_INVOKABLE void refreshDeviceInfo() { startDeviceInfo(); }
  Q_INVOKABLE void forgetDevice();

 signals:
  void settingsChanged();
  void statusChanged();
  void errorChanged();
  void deviceChanged();

 private slots:
  void avahiItemNew(int interface, int protocol, const QString &name, const QString &type, const QString &domain, uint flags);
 private:
  QVariantMap normalize(const QVariantMap &source) const {
    QVariantMap v = defaults();
    for (auto it = v.begin(); it != v.end(); ++it) if (source.contains(it.key())) it.value() = source.value(it.key());
    auto clampDouble = [&v](const char *key, double low, double high) {
      bool ok = false; double n = v.value(key).toDouble(&ok);
      if (!ok || !std::isfinite(n)) n = defaults().value(key).toDouble();
      v.insert(key, qBound(low, n, high));
    };
    clampDouble("brightness", 0, 2); clampDouble("saturation_boost", 0, 3);
    clampDouble("smooth", .05, 1); clampDouble("black_threshold", 0, .1);
    auto clampInt = [&v](const char *key, int low, int high) {
      bool ok = false; const int n = v.value(key).toInt(&ok);
      v.insert(key, ok ? qBound(low, n, high) : defaults().value(key).toInt());
    };
    clampInt("fps", 1, 10); clampInt("max_step", 1, 255); clampInt("offset", 0, 39);
    clampInt("edge_depth", 5, 20); clampInt("center_bleed", 0, 50); clampInt("peak_mix", 0, 35);
    clampInt("feather_depth", 0, 20);
    static const QStringList corners{"bottom-center", "bottom-left", "top-left", "top-right", "bottom-right"};
    if (!corners.contains(v.value("corner").toString())) v.insert("corner", "bottom-center");
    for (const auto &key : boolKeys) v.insert(key, v.value(key).toBool());
    return v;
  }
  QJsonObject loadJson(bool *valid) const {
    *valid = true;
    QFile file(configPath());
    if (!file.exists()) return {};
    if (!file.open(QIODevice::ReadOnly)) { *valid = false; return {}; }
    QJsonParseError parse;
    const auto doc = QJsonDocument::fromJson(file.readAll(), &parse);
    if (parse.error != QJsonParseError::NoError || !doc.isObject()) { *valid = false; return {}; }
    return doc.object();
  }
  QVariantMap readSettings() {
    QVariantMap result = defaults();
    bool valid;
    const QJsonObject object = loadJson(&valid);
    if (!valid) { setError("Invalid or unreadable config.json; settings were not overwritten"); return result; }
    for (const auto &key : settingKeys) if (object.contains(key)) result.insert(key, object.value(key).toVariant());
    return normalize(result);
  }
  bool writeConfig(const QJsonObject &object) {
    const QString path = configPath();
    if (!QDir().mkpath(QFileInfo(path).absolutePath())) { setError("Could not create config directory"); return false; }
    QSaveFile file(path);
    if (!file.open(QIODevice::WriteOnly)) { setError("Could not save settings"); return false; }
    if (!file.setPermissions(QFileDevice::ReadOwner | QFileDevice::WriteOwner)) {
      file.cancelWriting(); setError("Could not secure settings permissions"); return false;
    }
    const QByteArray data = QJsonDocument(object).toJson(QJsonDocument::Indented);
    if (file.write(data) != data.size() || !file.commit()) { setError("Could not save settings"); return false; }
    return true;
  }
  bool saveSettings() {
    bool valid;
    QJsonObject object = loadJson(&valid);
    if (!valid) { setError("Refusing to overwrite invalid config.json"); return false; }
    const QVariantMap safe = normalize(settings_);
    for (auto it = safe.cbegin(); it != safe.cend(); ++it) object.insert(it.key(), QJsonValue::fromVariant(it.value()));
    if (!writeConfig(object)) return false;
    pendingSettings_ = false;
    return true;
  }
  bool validIpv4(const QString &host) const {
    const QByteArray address = host.toLatin1();
    in_addr parsed{};
    return !address.isEmpty() && inet_pton(AF_INET, address.constData(), &parsed) == 1;
  }
  void startPairing(const QString &host) {
    if (busy_ || deviceBusy_ || enabled_ || status_ != "stopped") {
      deviceMessage_ = "Turn sync off before pairing or forgetting a device."; emit deviceChanged(); return;
    }
    const QString address = host.trimmed();
    if (!validIpv4(address)) { deviceMessage_ = "Enter a valid IPv4 address."; emit deviceChanged(); return; }
    if (pendingSettings_ && !saveSettings()) return;
    deviceInfo_.clear(); deviceBusy_ = true; deviceMessage_ = "Pairing. Hold the 4D power button for 5 seconds first…";
    emit deviceChanged();
    startDeviceCommand("pair", {"--pair", "--ip", address});
  }
  void startDeviceInfo() {
    if (!deviceConnected_ || deviceBusy_) return;
    deviceInfo_.clear(); deviceBusy_ = true; deviceMessage_ = "Loading device details…"; emit deviceChanged();
    startDeviceCommand("info", {"--device-info"});
  }
  void startDeviceCommand(const QString &action, const QStringList &args) {
    const QString executable = QStandardPaths::findExecutable(QStringLiteral("nanoleaf-sync"));
    if (executable.isEmpty()) {
      deviceBusy_ = false;
      deviceAction_.clear();
      deviceMessage_ = "Could not find nanoleaf-sync on PATH."; emit deviceChanged();
      if (pendingSettings_) saveTimer_.start(250);
      return;
    }
    QStringList commandArgs{"--config", configPath()}; commandArgs += args;
    deviceAction_ = action;
    deviceProcess_.start(executable, commandArgs);
  }
  void startDiscovery() {
    if (busy_ || deviceBusy_) return;
    auto bus = QDBusConnection::systemBus();
    if (!bus.isConnected()) { deviceMessage_ = "System mDNS is unavailable. Enter the device IP manually."; emit deviceChanged(); return; }
    deviceAction_ = "discover"; deviceBusy_ = true; discoveredDevices_.clear();
    deviceMessage_ = "Searching the local network for Nanoleaf devices…"; emit deviceChanged();
    const quint64 generation = ++discoveryGeneration_;
    QDBusInterface avahi(AvahiService, AvahiServerPath, AvahiServerInterface, bus);
    if (!avahi.isValid()) { failDiscovery("Avahi is unavailable. Enter the device IP manually."); return; }
    if (!bus.connect(AvahiService, QString(), AvahiBrowserInterface, "ItemNew", QStringLiteral("iisssu"),
                     this, SLOT(avahiItemNew(int,int,QString,QString,QString,uint)))) {
      failDiscovery("Could not listen for mDNS results. Enter the device IP manually."); return;
    }
    auto *watcher = new QDBusPendingCallWatcher(
        avahi.asyncCall("ServiceBrowserNew", -1, -1, QString::fromLatin1(NanoleafServiceType), QStringLiteral("local"), uint(0)), this);
    connect(watcher, &QDBusPendingCallWatcher::finished, this, [this, bus, generation](QDBusPendingCallWatcher *w) {
      QDBusPendingReply<QDBusObjectPath> reply = *w;
      w->deleteLater();
      if (generation != discoveryGeneration_ || deviceAction_ != "discover") {
        if (reply.isValid()) sendAvahiFree(bus, reply.value().path(), AvahiBrowserInterface);
        return;
      }
      if (!reply.isValid()) { failDiscovery("Avahi discovery failed: " + reply.error().message() + ". Enter the device IP manually."); return; }
      browserPath_ = reply.value().path();
      discoveryTimer_.start(5000);
    });
  }
  void resolveService(int interface, int protocol, const QString &name, const QString &type, const QString &domain) {
    const quint64 generation = discoveryGeneration_;
    const auto bus = QDBusConnection::systemBus();
    QDBusMessage message = QDBusMessage::createMethodCall(AvahiService, AvahiServerPath, AvahiServerInterface, "ResolveService");
    message << interface << protocol << name << type << domain << 0 << uint(0);
    auto *watcher = new QDBusPendingCallWatcher(bus.asyncCall(message), this);
    connect(watcher, &QDBusPendingCallWatcher::finished, this, [this, generation](QDBusPendingCallWatcher *w) {
      const QDBusMessage reply = w->reply();
      w->deleteLater();
      if (generation != discoveryGeneration_ || deviceAction_ != "discover" || reply.type() != QDBusMessage::ReplyMessage) return;
      const QVariantList values = reply.arguments();
      if (values.size() >= 9) addDiscoveredDevice(values.at(2).toString(), values.at(7).toString());
    });
  }
  void addDiscoveredDevice(const QString &name, const QString &address) {
    if (!validIpv4(address)) return;
    for (const QVariant &entry : discoveredDevices_) if (entry.toMap().value("host").toString() == address) return;
    discoveredDevices_.append(QVariantMap{{"name", name}, {"host", address}});
    emit deviceChanged();
  }
  void finishDiscovery() {
    if (deviceAction_ != "discover") return;
    releaseDiscoveryObjects();
    deviceAction_.clear(); deviceBusy_ = false;
    deviceMessage_ = discoveredDevices_.isEmpty() ? "No Nanoleaf devices found. Enter the device IP manually."
                                                   : "Select your Nanoleaf 4D from the results.";
    emit deviceChanged();
    if (pendingSettings_) saveTimer_.start(250);
  }
  void failDiscovery(const QString &message) {
    releaseDiscoveryObjects();
    deviceAction_.clear(); deviceBusy_ = false; deviceMessage_ = message; emit deviceChanged();
    if (pendingSettings_) saveTimer_.start(250);
  }
  void releaseDiscoveryObjects() {
    ++discoveryGeneration_; discoveryTimer_.stop();
    auto bus = QDBusConnection::systemBus();
    bus.disconnect(AvahiService, QString(), AvahiBrowserInterface, "ItemNew", QStringLiteral("iisssu"),
                   this, SLOT(avahiItemNew(int,int,QString,QString,QString,uint)));
    if (!browserPath_.isEmpty()) {
      sendAvahiFree(bus, browserPath_, AvahiBrowserInterface); browserPath_.clear();
    }
  }
  void sendAvahiFree(const QDBusConnection &bus, const QString &path, const char *interface) {
    QDBusMessage message = QDBusMessage::createMethodCall(AvahiService, path, QString::fromLatin1(interface), "Free");
    bus.asyncCall(message);
  }
  void deviceProcessFinished(int exitCode, QProcess::ExitStatus exitStatus) {
    const QString action = deviceAction_;
    deviceAction_.clear();
    if (action.isEmpty()) return;
    const QByteArray output = deviceProcess_.readAllStandardOutput();
    const QString error = QString::fromLocal8Bit(deviceProcess_.readAllStandardError()).trimmed();
    const bool success = exitStatus == QProcess::NormalExit && exitCode == 0;
    if (action == "pair" && success) {
      loadDeviceConfig();
      if (deviceConnected_) { deviceMessage_ = "Paired. Loading device details…"; startDeviceCommand("info", {"--device-info"}); }
      else { deviceBusy_ = false; deviceMessage_ = "Pairing finished but no device credentials were saved."; }
    } else if (action == "info") {
      deviceBusy_ = false;
      QJsonParseError parse;
      const QJsonDocument document = success ? QJsonDocument::fromJson(output, &parse) : QJsonDocument();
      if (!success) deviceMessage_ = deviceConnected_ ? "Device is saved, but its details could not be loaded. " + error : "Could not load device details. " + error;
      else if (parse.error != QJsonParseError::NoError || !document.isObject()) deviceMessage_ = "Device details response was invalid.";
      else { deviceInfo_ = document.object().toVariantMap(); deviceMessage_ = "Device details updated."; }
    } else {
      deviceBusy_ = false;
      deviceMessage_ = success ? "Pairing completed." : (error.isEmpty() ? "Pairing failed." : error);
    }
    emit deviceChanged();
    if (pendingSettings_) saveTimer_.start(250);
  }
  void deviceProcessError(QProcess::ProcessError error) {
    if (error != QProcess::FailedToStart || deviceAction_.isEmpty() || deviceAction_ == "discover") return;
    deviceAction_.clear(); deviceBusy_ = false; deviceMessage_ = "Could not start nanoleaf-sync.";
    emit deviceChanged();
    if (pendingSettings_) saveTimer_.start(250);
  }
  void loadDeviceConfig() {
    bool valid;
    const QJsonObject object = loadJson(&valid);
    if (!valid) return;
    deviceHost_ = object.value("ip").toString();
    deviceConnected_ = !deviceHost_.isEmpty() && !object.value("token").toString().isEmpty();
  }
  void applyLiveSettings(std::function<void(bool)> done = {}) {
    const quint64 generation = settingsGeneration_;
    const QVariantMap snapshot = settings_;
    QDBusMessage message = QDBusMessage::createMethodCall(Service, ObjectPath, Interface, "SetSettings");
    message << snapshot;
    auto *watcher = new QDBusPendingCallWatcher(QDBusConnection::sessionBus().asyncCall(message), this);
    connect(watcher, &QDBusPendingCallWatcher::finished, this, [this, generation, done](QDBusPendingCallWatcher *w) {
      const bool success = !w->isError();
      w->deleteLater();
      if (!success) setError("Worker rejected settings");
      else if (generation == settingsGeneration_) pendingSettings_ = false;
      if (done) done(success);
      else if (pendingSettings_ && workerPresent_) saveTimer_.start(250);
    });
  }
  void loadLiveSettings(const QDBusConnection &bus) {
    QDBusMessage message = QDBusMessage::createMethodCall(Service, ObjectPath, Interface, "GetSettings");
    auto *watcher = new QDBusPendingCallWatcher(bus.asyncCall(message), this);
    connect(watcher, &QDBusPendingCallWatcher::finished, this, [this](QDBusPendingCallWatcher *w) {
      QDBusPendingReply<QVariantMap> reply = *w;
      w->deleteLater();
      if (reply.isValid() && workerPresent_ && !pendingSettings_) {
        const QVariantMap next = normalize(reply.value());
        if (next != settings_) { settings_ = next; emit settingsChanged(); }
      }
    });
  }
  void queryWorker(std::function<void(bool)> done) {
    QDBusMessage query = QDBusMessage::createMethodCall(
        QStringLiteral("org.freedesktop.DBus"), QStringLiteral("/org/freedesktop/DBus"),
        QStringLiteral("org.freedesktop.DBus"), QStringLiteral("NameHasOwner"));
    query << QString::fromLatin1(Service);
    auto *watcher = new QDBusPendingCallWatcher(QDBusConnection::sessionBus().asyncCall(query), this);
    connect(watcher, &QDBusPendingCallWatcher::finished, this, [done](QDBusPendingCallWatcher *w) mutable {
      QDBusPendingReply<bool> reply = *w;
      w->deleteLater();
      done(reply.isValid() && reply.value());
    });
  }
  void stopWorkerThenDisable() {
    queryWorker([this](bool present) {
      workerPresent_ = present;
      if (present) {
        if (pendingSettings_) {
          applyLiveSettings([this](bool success) {
            if (success) stopWorker();
            else finishOperation(false);
          });
        } else stopWorker();
      } else {
        if (pendingSettings_ && !saveSettings()) { finishOperation(false); return; }
        disableService();
      }
    });
  }
  void stopWorker() {
    QDBusMessage message = QDBusMessage::createMethodCall(Service, ObjectPath, Interface, "Stop");
    auto *watcher = new QDBusPendingCallWatcher(QDBusConnection::sessionBus().asyncCall(message), this);
    connect(watcher, &QDBusPendingCallWatcher::finished, this, [this](QDBusPendingCallWatcher *w) {
      const bool success = !w->isError();
      w->deleteLater();
      if (!success) setError("Could not stop existing worker");
      disableService(success);
    });
  }
  void stopForCaptureChange(bool workerPresent) {
    if (!workerPresent) { stopCaptureService(); return; }
    QDBusMessage message = QDBusMessage::createMethodCall(Service, ObjectPath, Interface, "Stop");
    auto *watcher = new QDBusPendingCallWatcher(QDBusConnection::sessionBus().asyncCall(message), this);
    connect(watcher, &QDBusPendingCallWatcher::finished, this, [this](QDBusPendingCallWatcher *w) {
      const bool success = !w->isError();
      w->deleteLater();
      if (!success) { fail("Could not stop the current capture worker"); return; }
      stopCaptureService();
    });
  }
  void stopCaptureService() {
    runSystemctl({"--user", "stop", "nanoleaf-sync.service"}, [this](int code, const QString &) {
      if (code) { fail("Could not stop nanoleaf-sync.service before changing capture"); return; }
      workerPresent_ = false;
      bool valid;
      QJsonObject object = loadJson(&valid);
      if (!valid) { fail("Cannot clear the saved capture source because config.json is invalid or unreadable"); return; }
      object.remove("portal_restore_token");
      if (!writeConfig(object)) { finishOperation(false); return; }
      setStatus("starting");
      runSystemctl({"--user", "start", "nanoleaf-sync.service"}, [this](int startCode, const QString &) {
        if (startCode) fail("Could not start nanoleaf-sync.service with a fresh capture selection");
        else finishOperation(true);
      });
    });
  }
  void disableService(bool stopSucceeded = true) {
    runSystemctl({"--user", "disable", "--now", "nanoleaf-sync.service"}, [this, stopSucceeded](int code, const QString &) {
      if (code) fail("Could not disable nanoleaf-sync.service");
      else {
        setEnabledState(false);
        workerPresent_ = false;
        finishOperation(stopSucceeded);
        if (!stopSucceeded) setError("Could not stop existing worker");
      }
    });
  }
  void runSystemctl(const QStringList &args, std::function<void(int, QString)> done) {
    if (process_.state() != QProcess::NotRunning) { fail("Another systemctl operation is in progress"); return; }
    commandDone_ = std::move(done);
    process_.start(QStringLiteral("systemctl"), args);
  }
  void systemctlFinished(int code, QProcess::ExitStatus exit) {
    auto done = std::move(commandDone_);
    if (done) done(exit == QProcess::NormalExit ? code : -1, QString::fromLocal8Bit(process_.readAllStandardOutput()));
  }
  void finishOperation(bool success) {
    busy_ = false;
    if (!success) setStatus("error");
    else refresh();
    emit statusChanged();
  }
  void fail(const QString &message) { setError(message); finishOperation(false); }
  void setEnabledState(bool value) { if (enabled_ != value) { enabled_ = value; emit statusChanged(); } }
  void setStatus(const QString &value) { if (status_ != value) { status_ = value; emit statusChanged(); } }
  void setError(const QString &value) { if (error_ != value) { error_ = value; emit errorChanged(); } }

  QVariantMap settings_;
  QString status_ = "stopped", error_;
  bool enabled_ = false, busy_ = false, pollInFlight_ = false, pendingSettings_ = false;
  bool workerPresent_ = false, userSetEnabled_ = false;
  quint64 settingsGeneration_ = 0;
  QTimer poll_, saveTimer_;
  QProcess process_, serviceProbe_;
  QProcess deviceProcess_;
  QTimer discoveryTimer_;
  QString deviceHost_, deviceMessage_, browserPath_, deviceAction_;
  QVariantMap deviceInfo_;
  QVariantList discoveredDevices_;
  bool deviceConnected_ = false, deviceBusy_ = false;
  quint64 discoveryGeneration_ = 0;
  std::function<void(int, QString)> commandDone_;
};

void Backend::avahiItemNew(int interface, int protocol, const QString &name, const QString &type, const QString &domain, uint) {
  if (deviceAction_ == "discover" && type == QString::fromLatin1(NanoleafServiceType)) resolveService(interface, protocol, name, type, domain);
}
void Backend::forgetDevice() {
  if (busy_ || deviceBusy_ || enabled_ || status_ != "stopped") {
    deviceMessage_ = "Turn sync off before pairing or forgetting a device."; emit deviceChanged(); return;
  }
  if (pendingSettings_ && !saveSettings()) return;
  bool valid;
  QJsonObject object = loadJson(&valid);
  if (!valid) { deviceMessage_ = "Cannot forget the device because config.json is invalid or unreadable."; emit deviceChanged(); return; }
  object.remove("ip"); object.remove("token");
  if (!writeConfig(object)) return;
  deviceConnected_ = false; deviceHost_.clear(); deviceInfo_.clear(); discoveredDevices_.clear();
  deviceMessage_ = "Saved credentials removed from this computer. API access on the controller was not revoked.";
  emit deviceChanged();
}
class NanoleafBackendPlugin : public QQmlExtensionPlugin {
  Q_OBJECT
  Q_PLUGIN_METADATA(IID QQmlExtensionInterface_iid)
 public:
  void registerTypes(const char *uri) override { qmlRegisterType<Backend>(uri, 1, 0, "Backend"); }
};
} // namespace

#include "backend.moc"
