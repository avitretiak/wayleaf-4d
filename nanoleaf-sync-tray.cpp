#include <QApplication>
#include <QCheckBox>
#include <QComboBox>
#include <QCoreApplication>
#include <QDBusConnection>
#include <QDBusConnectionInterface>
#include <QDBusInterface>
#include <QDBusReply>
#include <QDBusServiceWatcher>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QIcon>
#include <QJsonDocument>
#include <QJsonObject>
#include <QLabel>
#include <QMenu>
#include <QMessageBox>
#include <QProcess>
#include <QPushButton>
#include <QSaveFile>
#include <QSlider>
#include <QSpinBox>
#include <QDoubleSpinBox>
#include <QStyle>
#include <QTabWidget>
#include <QStandardPaths>
#include <QSystemTrayIcon>
#include <QTimer>
#include <QVBoxLayout>
#include <QWidget>
#include <QMap>
#include <QVariantMap>
#include <functional>
#include <cmath>

namespace {
constexpr auto Service = "org.nanoleaf.NanoleafSync";
constexpr auto ObjectPath = "/org/nanoleaf/NanoleafSync";
constexpr auto Interface = "org.nanoleaf.NanoleafSync";

QVariantMap defaults() {
  return {{"brightness", 1.5}, {"saturation_boost", 1.5}, {"fps", 30},
          {"smooth", 0.35}, {"black_threshold", 0.01}, {"max_step", 12}, {"edge_depth", 8},
          {"center_bleed", 0}, {"peak_mix", 15}, {"feather_depth", 5},
          {"corner", "bottom-center"}, {"offset", 0}, {"reverse", false},
          {"letterbox", true}, {"midtone_lift", false}, {"strict_black", false}};
}
const QStringList boolKeys{"reverse", "letterbox", "midtone_lift", "strict_black"};
const QStringList intKeys{"fps", "max_step", "offset", "edge_depth", "center_bleed", "peak_mix", "feather_depth"};
const QStringList doubleKeys{"brightness", "saturation_boost", "smooth", "black_threshold"};

QString configPath() { return QStandardPaths::writableLocation(QStandardPaths::ConfigLocation) + "/nanoleaf-sync/config.json"; }

class TrayApp : public QObject {

 public:
  TrayApp() : QObject(qApp), bus_(QDBusConnection::sessionBus()), proc_(this), watcher_(this) {
    tray_.setIcon(QApplication::style()->standardIcon(QStyle::SP_ComputerIcon));
    tray_.setToolTip("Nanoleaf Sync");
    auto *menu = new QMenu;
    startAction_ = menu->addAction("Start");
    stopAction_ = menu->addAction("Stop");
    menu->addSeparator();
    menu->addAction("Settings…", this, &TrayApp::showSettings);
    menu->addSeparator();
    menu->addAction("Quit", this, &TrayApp::quitTray);
    tray_.setContextMenu(menu);
    connect(&tray_, &QSystemTrayIcon::activated, this, [this](QSystemTrayIcon::ActivationReason r) {
      if (r == QSystemTrayIcon::Trigger) showSettings();
    });
    connect(startAction_, &QAction::triggered, this, &TrayApp::startWorker);
    connect(stopAction_, &QAction::triggered, this, &TrayApp::stopWorker);
    connect(&proc_, &QProcess::errorOccurred, this, [this](QProcess::ProcessError) {
      if (proc_.error() != QProcess::FailedToStart) return;
      report("Could not start nanoleaf-sync: " + proc_.errorString()); updateStatus();
    });
    connect(&proc_, &QProcess::finished, this, [this](int, QProcess::ExitStatus) { updateStatus(); });
    watcher_.setConnection(bus_);
    watcher_.setWatchMode(QDBusServiceWatcher::WatchForOwnerChange);
    watcher_.addWatchedService(Service);
    connect(&watcher_, &QDBusServiceWatcher::serviceOwnerChanged, this,
            [this](const QString &, const QString &, const QString &) { serviceChanged(); });
    tray_.show();
    serviceChanged();
  }

 private:
  void serviceChanged() {
    const bool now = serviceRunning();
    if (now != serviceWasRunning_) {
      serviceWasRunning_ = now;
      if (dialog_ && dialog_->isVisible()) loadSettings();
    }
    updateStatus();
  }
  void startWorker() {
    if (serviceRunning() || proc_.state() != QProcess::NotRunning) return;
    const QString exe = QCoreApplication::applicationDirPath() + "/nanoleaf-sync";
    proc_.setProgram(exe);
    proc_.setArguments({});
    proc_.start();
    updateStatus();
  }
  void stopWorker() {
    if (!serviceRunning()) {
      if (proc_.state() != QProcess::NotRunning) {
        proc_.terminate();
        if (!proc_.waitForFinished(2000)) proc_.kill();
      }
      return;
    }
    QDBusInterface api(Service, ObjectPath, Interface, bus_);
    const QDBusReply<void> reply = api.call("Stop");
    if (!reply.isValid()) report("Could not stop worker: " + reply.error().message());
  }
  void quitTray() {
    if (proc_.state() != QProcess::NotRunning) {
      if (serviceRunning()) {
        QDBusInterface api(Service, ObjectPath, Interface, bus_);
        api.call("Stop");
      } else {
        proc_.terminate();
      }
      if (!proc_.waitForFinished(2000)) proc_.kill();
    }
    qApp->quit();
  }
  void showSettings() {
    if (!dialog_) buildDialog();
    loadSettings();
    dialog_->show(); dialog_->raise(); dialog_->activateWindow();
  }
  void applyPending() {
    if (!dialog_) return;
    QVariantMap values;
    for (auto it = controls_.cbegin(); it != controls_.cend(); ++it) values.insert(it.key(), it.value()());
    if (serviceRunning()) {
      QDBusInterface api(Service, ObjectPath, Interface, bus_);
      const QDBusReply<void> reply = api.call("SetSettings", values);
      if (!reply.isValid()) { report("Settings not applied: " + reply.error().message()); return; }
    } else if (!saveSettings(values)) return;
    status_->setText(serviceRunning() ? "Running · settings applied" : "Stopped · settings saved");
  }

 private:
  bool serviceRunning() const {
    auto *iface = bus_.interface();
    return iface && iface->isServiceRegistered(Service);
  }
  void updateStatus() {
    const bool running = serviceRunning();
    startAction_->setEnabled(!running && proc_.state() == QProcess::NotRunning);
    stopAction_->setEnabled(running || proc_.state() != QProcess::NotRunning);
    tray_.setToolTip(running ? "Nanoleaf Sync · running" : proc_.state() != QProcess::NotRunning ? "Nanoleaf Sync · starting" : "Nanoleaf Sync · stopped");
    if (status_ && (!dialog_ || !dialog_->isVisible())) status_->setText(running ? "Running" : proc_.state() != QProcess::NotRunning ? "Starting…" : "Stopped");
  }
  void report(const QString &message) {
    tray_.showMessage("Nanoleaf Sync", message, QSystemTrayIcon::Warning, 5000);
    if (status_) status_->setText(message);
  }
  void buildDialog() {
    dialog_ = new QWidget;
    dialog_->setWindowTitle("Nanoleaf Sync Settings");
    dialog_->setAttribute(Qt::WA_DeleteOnClose, false);
    auto *layout = new QVBoxLayout(dialog_);
    status_ = new QLabel;
    layout->addWidget(status_);
    auto *tabs = new QTabWidget(dialog_);
    layout->addWidget(tabs);

    auto *general = new QWidget(tabs);
    auto *generalLayout = new QVBoxLayout(general);
    auto *form = new QFormLayout;
    generalLayout->addLayout(form);
    debounce_.setSingleShot(true); debounce_.setInterval(300);
    connect(&debounce_, &QTimer::timeout, this, &TrayApp::applyPending);
    addDouble(form, "Brightness", "brightness", 0, 2, 1, 1.0, "Scales final LED brightness; 1.0 is neutral.");
    addDouble(form, "Saturation boost", "saturation_boost", 0, 3, 2, 1.5, "Higher values emphasize saturated pixels and boost color intensity.");
    addSpin(form, "FPS", "fps", 1, 60, "Requested maximum sample/send rate; PipeWire may deliver fewer frames.");
    addDouble(form, "Smoothing", "smooth", 0.05, 1, 2, 0.35, "Lower values smooth flicker but respond more slowly; higher values follow changes faster.");
    addDouble(form, "Black threshold", "black_threshold", 0, 0.1, 3, 0.02, "Suppresses near-black capture noise; increase it if dark areas glow.");
    addSpin(form, "Maximum step", "max_step", 1, 255, "Limits each frame's color change; lower values are smoother but add lag.");
    addSpin(form, "Edge sample depth (%)", "edge_depth", 5, 20, "How far inward from each screen edge to sample; higher values pick up nearby content sooner.");
    addSpin(form, "Center color blend (%)", "center_bleed", 0, 50, "Mix this share of the central picture's average color into every edge segment.");
    addSpin(form, "Brightest-pixel blend (%)", "peak_mix", 0, 35, "Mix this share of the brightest-luma pixel in each zone; lower values suppress tiny highlights.");
    addSpin(form, "Inward feather depth (%)", "feather_depth", 0, 20, "Extend each edge zone inward with a smooth taper to zero; 0 disables the feather.");
    addCheck(form, "Letterbox detection", "letterbox", "Detects black bars and samples within the active picture.");
    addCheck(form, "Midtone lift", "midtone_lift", "Optional legacy lift for dim and midtone content.");
    addCheck(form, "Strict black", "strict_black", "Sends exact black immediately instead of smoothing dark transitions.");
    tabs->addTab(general, "General");

    auto *alignment = new QWidget(tabs);
    auto *alignmentLayout = new QVBoxLayout(alignment);
    auto *help = new QLabel("Choose the screen corner that matches the strip's starting segment. Use Reverse if the lighting order runs backwards; use Offset to shift the segment mapping.", alignment);
    help->setWordWrap(true);
    alignmentLayout->addWidget(help);
    auto *alignmentForm = new QFormLayout;
    alignmentLayout->addLayout(alignmentForm);
    auto *corner = new QComboBox(alignment);
    corner->setProperty("settingKey", "corner");
    corner->addItems({"bottom-center", "bottom-left", "top-left", "top-right", "bottom-right"});
    corner->setToolTip("Choose the screen corner used to align the strip's starting segment.");
    controls_.insert("corner", [corner] { return QVariant(corner->currentText()); });
    connect(corner, &QComboBox::currentTextChanged, this, [this] { debounce_.start(); });
    auto *cornerLabel = new QLabel("Start corner", alignment);
    cornerLabel->setToolTip(corner->toolTip());
    alignmentForm->addRow(cornerLabel, corner);
    addSpin(alignmentForm, "Offset", "offset", 0, 39, "Rotates the panel mapping by this many segments.");
    addCheck(alignmentForm, "Reverse direction", "reverse", "Reverses the order used to map screen zones to strip segments.");
    tabs->addTab(alignment, "4D alignment");

    auto *buttons = new QHBoxLayout;
    auto *start = new QPushButton("Start", dialog_);
    auto *stop = new QPushButton("Stop", dialog_);
    buttons->addWidget(start); buttons->addWidget(stop); buttons->addStretch();
    layout->addLayout(buttons);
    connect(start, &QPushButton::clicked, this, &TrayApp::startWorker);
    connect(stop, &QPushButton::clicked, this, &TrayApp::stopWorker);
    connect(dialog_, &QWidget::destroyed, this, [this] { dialog_ = nullptr; status_ = nullptr; controls_.clear(); });
    updateStatus();
  }
  void addDouble(QFormLayout *form, const QString &label, const QString &key, double min, double max, int decimals, double step, const QString &tip) {
    auto *row = new QWidget(form->parentWidget()); auto *h = new QHBoxLayout(row); h->setContentsMargins(0,0,0,0);
    auto *slider = new QSlider(Qt::Horizontal, row); const int scale = decimals == 3 ? 1000 : decimals == 2 ? 100 : 10;
    slider->setProperty("settingKey", key); slider->setToolTip(tip);
    slider->setRange(qRound(min * scale), qRound(max * scale));
    auto *spin = new QDoubleSpinBox(row); spin->setRange(min, max); spin->setDecimals(decimals); spin->setSingleStep(step); spin->setMinimumWidth(90);
    spin->setProperty("settingKey", key); spin->setToolTip(tip);
    h->addWidget(slider, 1); h->addWidget(spin);
    connect(slider, &QSlider::valueChanged, this, [spin, scale](int v) { spin->setValue(double(v) / scale); });
    connect(spin, qOverload<double>(&QDoubleSpinBox::valueChanged), this, [slider, scale](double v) { slider->setValue(qRound(v * scale)); });
    controls_.insert(key, [spin] { return QVariant(spin->value()); });
    connect(spin, qOverload<double>(&QDoubleSpinBox::valueChanged), this, [this] { debounce_.start(); });
    auto *labelWidget = new QLabel(label, form->parentWidget()); labelWidget->setToolTip(tip);
    form->addRow(labelWidget, row);
  }
  void addSpin(QFormLayout *form, const QString &label, const QString &key, int min, int max, const QString &tip) {
    auto *spin = new QSpinBox(form->parentWidget()); spin->setProperty("settingKey", key); spin->setRange(min, max); spin->setToolTip(tip);
    controls_.insert(key, [spin] { return QVariant(spin->value()); });
    connect(spin, qOverload<int>(&QSpinBox::valueChanged), this, [this] { debounce_.start(); });
    auto *labelWidget = new QLabel(label, form->parentWidget()); labelWidget->setToolTip(tip);
    form->addRow(labelWidget, spin);
  }
  void addCheck(QFormLayout *form, const QString &label, const QString &key, const QString &tip) {
    auto *check = new QCheckBox(form->parentWidget()); check->setProperty("settingKey", key); check->setToolTip(tip);
    controls_.insert(key, [check] { return QVariant(check->isChecked()); });
    connect(check, &QCheckBox::toggled, this, [this] { debounce_.start(); });
    auto *labelWidget = new QLabel(label, form->parentWidget()); labelWidget->setToolTip(tip);
    form->addRow(labelWidget, check);
  }
  QVariantMap readSettings() {
    QVariantMap value = defaults();
    if (serviceRunning()) {
      QDBusInterface api(Service, ObjectPath, Interface, bus_);
      const QDBusReply<QVariantMap> reply = api.call("GetSettings");
      if (!reply.isValid()) { report("Could not load settings: " + reply.error().message()); return value; }
      value = reply.value();
    } else {
      QFile f(configPath());
      if (f.exists()) {
        if (!f.open(QIODevice::ReadOnly)) { report("Could not read settings: " + f.errorString()); return value; }
        QJsonParseError error;
        const auto doc = QJsonDocument::fromJson(f.readAll(), &error);
        if (error.error != QJsonParseError::NoError || !doc.isObject()) { report("Invalid config.json: " + error.errorString()); return value; }
        const auto obj = doc.object();
        for (const auto &key : boolKeys + intKeys + doubleKeys + QStringList{"corner"})
          if (obj.contains(key)) value.insert(key, obj.value(key).toVariant());
      }
    }
    return normalize(value);
  }
  QVariantMap normalize(const QVariantMap &source) {
    QVariantMap v = defaults();
    for (auto it = v.begin(); it != v.end(); ++it) if (source.contains(it.key())) it.value() = source.value(it.key());
    auto clampDouble = [&v](const char *key, double low, double high) {
      bool ok = false; double n = v.value(key).toDouble(&ok);
      if (!ok || !std::isfinite(n)) n = defaults().value(key).toDouble();
      v.insert(key, qBound(low, n, high));
    };
    clampDouble("brightness", 0, 2); clampDouble("saturation_boost", 0, 3);
    clampDouble("smooth", .05, 1); clampDouble("black_threshold", 0, .1);
    auto clampInt = [&v](const char *key, int low, int high) { bool ok = false; int n = v.value(key).toInt(&ok); v.insert(key, ok ? qBound(low, n, high) : defaults().value(key).toInt()); };
    clampInt("fps", 1, 60); clampInt("max_step", 1, 255); clampInt("offset", 0, 39); clampInt("edge_depth", 5, 20);
    clampInt("center_bleed", 0, 50); clampInt("peak_mix", 0, 35); clampInt("feather_depth", 0, 20);
    static const QStringList corners{"bottom-center", "bottom-left", "top-left", "top-right", "bottom-right"};
    if (!corners.contains(v.value("corner").toString())) v.insert("corner", "bottom-center");
    for (const auto &key : boolKeys) v.insert(key, v.value(key).toBool());
    return v;
  }
  bool saveSettings(const QVariantMap &values) {
    const QString path = configPath();
    const QString dirPath = QFileInfo(path).absolutePath();
    QDir dir;
    if (!dir.mkpath(dirPath)) { report("Could not create config directory"); return false; }
    QJsonObject obj;
    QFile in(path);
    if (in.exists()) {
      if (!in.open(QIODevice::ReadOnly)) { report("Could not read config: " + in.errorString()); return false; }
      QJsonParseError error;
      const auto doc = QJsonDocument::fromJson(in.readAll(), &error);
      if (error.error != QJsonParseError::NoError || !doc.isObject()) { report("Refusing to overwrite invalid config.json"); return false; }
      obj = doc.object();
    }
    const QVariantMap safe = normalize(values);
    for (auto it = safe.cbegin(); it != safe.cend(); ++it) obj.insert(it.key(), QJsonValue::fromVariant(it.value()));
    QSaveFile out(path);
    if (!out.open(QIODevice::WriteOnly)) { report("Could not save config: " + out.errorString()); return false; }
    if (!out.setPermissions(QFileDevice::ReadOwner | QFileDevice::WriteOwner)) {
      report("Could not secure config permissions: " + out.errorString()); out.cancelWriting(); return false;
    }
    const QByteArray json = QJsonDocument(obj).toJson(QJsonDocument::Indented);
    if (out.write(json) != json.size() || !out.commit()) { report("Could not save config: " + out.errorString()); return false; }
    return true;
  }
  void loadSettings() {
    const QVariantMap v = readSettings();
    for (QObject *child : dialog_->findChildren<QObject *>()) {
      const QString key = child->property("settingKey").toString();
      if (key.isEmpty()) continue;
      if (auto *spin = qobject_cast<QDoubleSpinBox *>(child)) spin->setValue(v.value(key).toDouble());
      else if (auto *spin = qobject_cast<QSpinBox *>(child)) spin->setValue(v.value(key).toInt());
      else if (auto *check = qobject_cast<QCheckBox *>(child)) check->setChecked(v.value(key).toBool());
      else if (auto *slider = qobject_cast<QSlider *>(child)) {
        const int scale = key == "black_threshold" ? 1000 : key == "brightness" ? 10 : 100;
        slider->setValue(qRound(v.value(key).toDouble() * scale));
      } else if (auto *combo = qobject_cast<QComboBox *>(child)) combo->setCurrentText(v.value(key).toString());
    }
    debounce_.stop(); updateStatus();
  }
  QDBusConnection bus_;
  QProcess proc_;
  QDBusServiceWatcher watcher_;
  QSystemTrayIcon tray_;
  QAction *startAction_ = nullptr, *stopAction_ = nullptr;
  QWidget *dialog_ = nullptr;
  QLabel *status_ = nullptr;
  QMap<QString, std::function<QVariant()>> controls_;
  QTimer debounce_;
  bool serviceWasRunning_ = false;
};
} // namespace

int main(int argc, char **argv) {
  QApplication app(argc, argv);
  QApplication::setQuitOnLastWindowClosed(false);
  QApplication::setApplicationName("Nanoleaf Sync");
  if (!QSystemTrayIcon::isSystemTrayAvailable()) {
    QMessageBox::critical(nullptr, "Nanoleaf Sync", "No system tray is available.");
    return 1;
  }
  TrayApp tray;
  return app.exec();
}

