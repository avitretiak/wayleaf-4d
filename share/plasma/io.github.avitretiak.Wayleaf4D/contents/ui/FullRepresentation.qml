pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Window
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import org.kde.plasma.plasmoid

Item {
    id: root
    required property var backend
    required property bool expanded

    property var customSettings: ({})
    property bool customSettingsPersisted: false
    property bool userTouchedSettings: false
    property string selectedPreset: "Custom"
    readonly property var presetKeys: ["saturation_boost", "smooth", "max_step", "edge_depth", "center_bleed", "peak_mix", "fps"]
    readonly property real pageHeightLimit: Math.max(Kirigami.Units.gridUnit * 8, Screen.height * 0.56)
    readonly property real popupHeightLimit: Screen.height * 0.88

    implicitWidth: Kirigami.Units.gridUnit * 25
    implicitHeight: Math.min(content.implicitHeight + Kirigami.Units.smallSpacing * 4, popupHeightLimit)
    Accessible.name: "Wayleaf 4D controls"
    clip: true

    function presetValues(name) {
        if (name === "Subtle")
            return ({
                    saturation_boost: 1.0,
                    smooth: 0.2,
                    max_step: 6,
                    edge_depth: 8,
                    center_bleed: 0,
                    peak_mix: 10,
                    fps: 10
                });
        if (name === "Balanced")
            return ({
                    saturation_boost: 1.5,
                    smooth: 0.35,
                    max_step: 12,
                    edge_depth: 8,
                    center_bleed: 0,
                    peak_mix: 15,
                    fps: 10
                });
        if (name === "Vivid")
            return ({
                    saturation_boost: 2.2,
                    smooth: 0.7,
                    max_step: 24,
                    edge_depth: 12,
                    center_bleed: 5,
                    peak_mix: 25,
                    fps: 10
                });
        return null;
    }

    function valuesFromSettings(settings) {
        const values = ({});
        for (const key of presetKeys)
            values[key] = Number(settings[key]);
        return values;
    }

    function samePreset(values, preset) {
        const expected = presetValues(preset);
        if (!expected)
            return false;
        for (const key of presetKeys) {
            if (Number(values[key]) !== Number(expected[key]))
                return false;
        }
        return true;
    }

    function validCustomSettings(values) {
        const bounds = {
            saturation_boost: [0, 3],
            smooth: [0.05, 1],
            max_step: [1, 255],
            edge_depth: [5, 20],
            center_bleed: [0, 50],
            peak_mix: [0, 35],
            fps: [1, 10]
        };
        if (!values || typeof values !== "object" || Array.isArray(values))
            return false;
        for (const key of presetKeys) {
            const value = Number(values[key]);
            if (!Number.isFinite(value) || value < bounds[key][0] || value > bounds[key][1])
                return false;
        }
        return true;
    }

    function loadCustomSettings() {
        try {
            const saved = JSON.parse(String(Plasmoid.configuration.customSettings || ""));
            if (!validCustomSettings(saved))
                return false;
            customSettings = valuesFromSettings(saved);
            return true;
        } catch (e) {
            return false;
        }
    }

    function saveCustomSettings(values) {
        customSettings = values;
        Plasmoid.configuration.customSettings = JSON.stringify(values);
        customSettingsPersisted = true;
    }

    function syncPreview() {
        backend.setPreviewEnabled(expanded && backend.running);
    }

    function captureInitialSettings() {
        if (userTouchedSettings)
            return;
        const current = valuesFromSettings(backend.settings);
        if (!customSettingsPersisted)
            customSettings = current;
        selectedPreset = samePreset(current, "Balanced") ? "Balanced" : samePreset(current, "Subtle") ? "Subtle" : samePreset(current, "Vivid") ? "Vivid" : "Custom";
    }

    function noteFineTuneEdit(key, value) {
        if (presetKeys.indexOf(key) < 0)
            return;
        const next = valuesFromSettings(backend.settings);
        next[key] = value;
        saveCustomSettings(next);
        userTouchedSettings = true;
        selectedPreset = "Custom";
    }

    function choosePreset(name) {
        userTouchedSettings = true;
        selectedPreset = name;
        if (name === "Custom") {
            for (const key of presetKeys)
                backend.setSetting(key, customSettings[key]);
            return;
        }
        const values = presetValues(name);
        for (const key of presetKeys)
            backend.setSetting(key, values[key]);
    }

    Component.onCompleted: {
        customSettingsPersisted = loadCustomSettings();
        captureInitialSettings();
        syncPreview();
    }

    onExpandedChanged: syncPreview()
    Connections {
        target: root.backend
        function onRunningChanged() {
            root.syncPreview();
        }
        function onSettingsChanged() {
            root.captureInitialSettings();
        }
    }
    Component.onDestruction: backend.setPreviewEnabled(false)

    ColumnLayout {
        id: content
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        anchors.margins: Kirigami.Units.smallSpacing * 2
        spacing: Kirigami.Units.smallSpacing

        RowLayout {
            Layout.fillWidth: true
            Rectangle {
                Layout.alignment: Qt.AlignVCenter
                width: 9
                height: 9
                radius: width / 2
                color: root.backend.status === "error" || root.backend.error.length > 0 ? Kirigami.Theme.negativeTextColor : root.backend.status === "starting" || root.backend.status === "stopping" ? Kirigami.Theme.neutralTextColor : root.backend.running ? Kirigami.Theme.positiveTextColor : Kirigami.Theme.disabledTextColor
                Accessible.ignored: true
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 1
                Label {
                    text: root.backend.status === "error" || root.backend.error.length > 0 ? "Synchronization needs attention" : root.backend.status === "starting" ? "Starting synchronization…" : root.backend.status === "stopping" ? "Stopping synchronization…" : root.backend.running ? "Screen synchronization is on" : "Screen synchronization is off"
                    font.bold: true
                    Accessible.name: text
                }
                Label {
                    text: root.backend.running ? root.backend.selectedMonitor + " · " + Number(root.backend.settings.fps) + " Hz" : root.backend.deviceConnected ? root.backend.deviceHost : "No Nanoleaf 4D paired"
                    color: Kirigami.Theme.disabledTextColor
                    font: Kirigami.Theme.smallFont
                    elide: Text.ElideMiddle
                    Layout.fillWidth: true
                }
            }
            Switch {
                id: syncSwitch
                objectName: "sync-switch"
                checked: root.backend.running
                enabled: root.backend.deviceConnected && root.backend.selectedMonitor.length > 0 && !root.backend.busy && !root.backend.deviceBusy && root.backend.status !== "starting" && root.backend.status !== "stopping"
                onClicked: {
                    if (checked !== root.backend.running)
                        checked ? root.backend.run() : root.backend.stop();
                }
                Accessible.name: "Screen synchronization"
                Accessible.description: checked ? "On. Toggle to stop synchronization." : !root.backend.deviceConnected ? "Disabled until a Nanoleaf 4D is paired." : root.backend.selectedMonitor.length === 0 ? "Disabled until a capture monitor is selected." : "Off. Toggle to start synchronization."
                ToolTip.text: Accessible.description
                ToolTip.visible: hovered
            }
            ToolButton {
                id: overflowButton
                icon.name: "application-menu"
                display: AbstractButton.IconOnly
                Accessible.name: "More options"
                ToolTip.text: "More options"
                ToolTip.visible: hovered
                onClicked: overflowMenu.open()
                Menu {
                    id: overflowMenu
                    MenuItem {
                        text: "Device details…"
                        enabled: root.backend.deviceConnected || root.backend.deviceInfoStale || Object.keys(root.backend.deviceInfo).length > 0
                        onTriggered: deviceDialog.open()
                    }
                    MenuItem {
                        text: "Start at login"
                        objectName: "start-at-login-switch"
                        checkable: true
                        checked: root.backend.startAtLogin
                        enabled: !root.backend.busy && !root.backend.deviceBusy
                        onTriggered: root.backend.setStartAtLogin(checked)
                    }
                    MenuItem {
                        text: "About capture…"
                        onTriggered: aboutDialog.open()
                    }
                    MenuItem {
                        text: "Diagnostics and help…"
                        onTriggered: diagnosticsDialog.open()
                    }
                    MenuSeparator {}
                    MenuItem {
                        text: root.backend.running ? "Stop sync and disconnect…" : "Disconnect…"
                        enabled: root.backend.deviceConnected && !root.backend.busy && !root.backend.deviceBusy
                        onTriggered: disconnectDialog.open()
                    }
                }
            }
        }

        Label {
            Layout.fillWidth: true
            visible: !syncSwitch.enabled
            text: !root.backend.deviceConnected ? "Pair a Nanoleaf 4D to enable synchronization." : root.backend.selectedMonitor.length === 0 ? "Choose a capture monitor to enable synchronization." : ""
            wrapMode: Text.Wrap
            color: Kirigami.Theme.disabledTextColor
        }
        Label {
            Layout.fillWidth: true
            visible: root.backend.error.length > 0
            text: "The control service needs attention. Open Diagnostics and help for recovery steps."
            wrapMode: Text.Wrap
            color: Kirigami.Theme.negativeTextColor
            Accessible.role: Accessible.AlertMessage
        }
        Label {
            Layout.fillWidth: true
            visible: root.backend.settingsPending && root.backend.settingsError.length > 0
            text: "Settings are not confirmed. Retry them from Fine tune."
            wrapMode: Text.Wrap
            color: Kirigami.Theme.negativeTextColor
            Accessible.role: Accessible.AlertMessage
        }
        Label {
            Layout.fillWidth: true
            visible: root.backend.settingsPending && root.backend.settingsError.length === 0
            text: "Applying settings…"
            wrapMode: Text.Wrap
            color: Kirigami.Theme.disabledTextColor
        }
        Label {
            Layout.fillWidth: true
            visible: root.backend.deviceMessage.startsWith("Warning:")
            text: root.backend.deviceMessage
            wrapMode: Text.Wrap
            color: Kirigami.Theme.neutralTextColor
            Accessible.role: Accessible.AlertMessage
            Accessible.name: text
        }

        SettingSlider {
            backend: root.backend
            settingKey: "brightness"
            label: "Brightness"
            description: "Scales final LED brightness. 50% is neutral; use the reset button to restore the default."
            minimum: 0
            maximum: 2
            decimals: 0
            valueMultiplier: 50
            displayMultiplier: 50
            suffix: "%"
            defaultValue: 1.5
            objectName: "control-brightness"
        }

        TabBar {
            id: pageTabs
            Layout.fillWidth: true
            TabButton {
                text: "Main"
                Accessible.name: "Main controls"
            }
            TabButton {
                text: "Fine tune"
                Accessible.name: "Fine-tune controls"
            }
        }
        StackLayout {
            id: pages
            currentIndex: pageTabs.currentIndex
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.preferredHeight: Math.min(pageTabs.currentIndex === 0 ? mainPageContent.implicitHeight : fineTuneContent.implicitHeight, root.pageHeightLimit)
            ScrollView {
                id: mainScroll
                Accessible.name: "Main controls"
                Layout.fillWidth: true
                Layout.fillHeight: true
                contentWidth: availableWidth
                contentHeight: mainPageContent.implicitHeight
                clip: true
                ScrollBar.vertical.policy: ScrollBar.AsNeeded
                ScrollBar.vertical.interactive: true
                ColumnLayout {
                    id: mainPageContent
                    width: mainScroll.availableWidth
                    spacing: Kirigami.Units.smallSpacing
                    RowLayout {
                        Layout.fillWidth: true
                        Label {
                            text: "Preset"
                            Accessible.name: "Color response preset"
                        }
                        ComboBox {
                            id: presetBox
                            objectName: "response-preset"
                            Layout.fillWidth: true
                            enabled: !root.backend.busy && !root.backend.deviceBusy
                            model: ["Subtle", "Balanced", "Vivid", "Custom"]
                            currentIndex: model.indexOf(root.selectedPreset)
                            onActivated: root.choosePreset(currentText)
                            Accessible.name: "Color response preset"
                            Accessible.description: "Presets change saturation, response, maximum step, sample depth, center blend, peak blend, and rate only."
                        }
                    }

                    Item {
                        id: bezelPreview
                        objectName: "live-preview"
                        Layout.fillWidth: true
                        Layout.preferredHeight: Math.max(Kirigami.Units.gridUnit * 5, width * 0.30)
                        Accessible.name: "Live screen-edge color preview"
                        Accessible.description: "Colors are positioned by each zone's screen edge and coordinates; the number is the controller zone ID."
                        Rectangle {
                            id: screen
                            anchors.fill: parent
                            anchors.margins: 7
                            color: Kirigami.Theme.alternateBackgroundColor
                            border.color: Kirigami.Theme.disabledTextColor
                            border.width: 1
                            radius: Kirigami.Units.smallSpacing
                        }
                        Repeater {
                            model: root.backend.previewZones
                            delegate: Rectangle {
                                required property var modelData
                                readonly property real segmentSpan: Math.max(0, Number(modelData.span))
                                readonly property string edge: String(modelData.edge)
                                readonly property real position: Math.max(0, Math.min(1, Number(modelData.position)))
                                readonly property real segmentStart: Math.max(0, position - segmentSpan / 2)
                                readonly property real segmentEnd: Math.min(1, position + segmentSpan / 2)
                                readonly property real actualSpan: Math.max(0, segmentEnd - segmentStart)
                                readonly property color zoneColor: Qt.rgba(Number(modelData.r) / 255, Number(modelData.g) / 255, Number(modelData.b) / 255, 1)
                                readonly property real strip: 6
                                x: edge === "top" || edge === "bottom" ? 7 + (bezelPreview.width - 14) * segmentStart : edge === "right" ? bezelPreview.width - strip - 1 : 1
                                y: edge === "left" || edge === "right" ? 7 + (bezelPreview.height - 14) * segmentStart : edge === "bottom" ? bezelPreview.height - strip - 1 : 1
                                width: edge === "top" || edge === "bottom" ? Math.max(2, (bezelPreview.width - 14) * actualSpan) : strip
                                height: edge === "left" || edge === "right" ? Math.max(2, (bezelPreview.height - 14) * actualSpan) : strip
                                color: zoneColor
                                radius: 2
                                border.color: Kirigami.Theme.backgroundColor
                                border.width: 1
                                z: 1
                                ToolTip.visible: zoneHover.hovered
                                ToolTip.text: "Zone " + modelData.id + " · " + edge + " · " + Math.round(position * 100) + "%"
                                HoverHandler {
                                    id: zoneHover
                                }
                                Accessible.name: "Controller zone " + modelData.id + " at " + edge + " edge"
                            }
                        }
                        Label {
                            anchors.centerIn: parent
                            visible: root.backend.previewZones.length === 0
                            text: root.backend.running ? "Waiting for preview data…" : ""
                            color: Kirigami.Theme.disabledTextColor
                            z: 2
                        }
                    }
                    ColumnLayout {
                        Layout.fillWidth: true
                        visible: !root.backend.deviceConnected
                        spacing: Kirigami.Units.smallSpacing
                        Label {
                            text: "Pair a Nanoleaf 4D"
                            font.bold: true
                        }
                        Label {
                            Layout.fillWidth: true
                            text: "Search with the system's Avahi mDNS service, or enter the controller's IPv4 address. Hold the 4D power button for 5 seconds before pairing; the pairing window lasts up to 30 seconds."
                            wrapMode: Text.Wrap
                        }
                        Button {
                            text: root.backend.deviceBusy ? "Searching…" : "Find devices on this network"
                            enabled: !root.backend.busy && !root.backend.deviceBusy
                            onClicked: {
                                deviceList.currentIndex = -1;
                                root.backend.discoverDevices();
                            }
                        }
                        Label {
                            Layout.fillWidth: true
                            visible: !root.backend.deviceBusy && root.backend.discoveredDevices.length === 0
                            text: "No devices found yet. Search the network or enter the controller's IPv4 address."
                            wrapMode: Text.Wrap
                            color: Kirigami.Theme.disabledTextColor
                        }
                        ListView {
                            id: deviceList
                            Layout.fillWidth: true
                            Layout.preferredHeight: Math.min(contentHeight, Kirigami.Units.gridUnit * 6)
                            visible: count > 0
                            clip: true
                            model: root.backend.discoveredDevices
                            currentIndex: -1
                            keyNavigationEnabled: true
                            keyNavigationWraps: false
                            activeFocusOnTab: true
                            Accessible.name: "Discovered Nanoleaf devices"
                            Accessible.description: "Use arrow keys to browse the discovered controllers and Enter to choose one."
                            onActiveFocusChanged: {
                                if (activeFocus && currentIndex < 0 && count > 0)
                                    currentIndex = 0;
                            }
                            Keys.onReturnPressed: {
                                if (currentIndex >= 0)
                                    manualIp.text = root.backend.discoveredDevices[currentIndex].host;
                            }
                            Keys.onEnterPressed: {
                                if (currentIndex >= 0)
                                    manualIp.text = root.backend.discoveredDevices[currentIndex].host;
                            }
                            ScrollBar.vertical: ScrollBar {}
                            delegate: ItemDelegate {
                                required property int index
                                required property var modelData
                                width: deviceList.width
                                text: modelData.name + " — " + modelData.host
                                highlighted: deviceList.currentIndex === index
                                onClicked: {
                                    deviceList.currentIndex = index;
                                    manualIp.text = modelData.host;
                                }
                                Accessible.name: modelData.name + " at " + modelData.host
                            }
                        }
                        TextField {
                            id: manualIp
                            Layout.fillWidth: true
                            placeholderText: "Controller IPv4 address (e.g. 192.168.1.50)"
                            validator: RegularExpressionValidator {
                                regularExpression: /^(?:25[0-5]|2[0-4][0-9]|1?[0-9]?[0-9])(?:\.(?:25[0-5]|2[0-4][0-9]|1?[0-9]?[0-9])){3}$/
                            }
                            Accessible.name: "Controller IPv4 address"
                            Accessible.description: manualIp.text.length > 0 && !manualIp.acceptableInput ? "Invalid address. Enter four decimal IPv4 octets from 0 to 255, separated by dots." : "Enter an address or choose a discovered device."
                        }
                        Label {
                            Layout.fillWidth: true
                            visible: manualIp.text.trim().length > 0 && !manualIp.acceptableInput
                            text: "Invalid IPv4 address. Enter four decimal octets from 0 to 255, separated by dots."
                            wrapMode: Text.Wrap
                            color: Kirigami.Theme.negativeTextColor
                            Accessible.role: Accessible.AlertMessage
                        }
                        Button {
                            text: root.backend.deviceBusy ? "Pairing…" : "Pair device"
                            enabled: !root.backend.busy && !root.backend.deviceBusy && !root.backend.running && root.backend.status === "stopped" && (manualIp.text.trim().length > 0 ? manualIp.acceptableInput : deviceList.currentIndex >= 0)
                            onClicked: root.backend.pairDevice(manualIp.text.trim().length > 0 ? manualIp.text.trim() : root.backend.discoveredDevices[deviceList.currentIndex].host)
                        }
                        Label {
                            Layout.fillWidth: true
                            text: "Pairing stores the API token on this computer with owner-only file permissions."
                            wrapMode: Text.Wrap
                            color: Kirigami.Theme.disabledTextColor
                        }
                    }
                    RowLayout {
                        Layout.fillWidth: true
                        Label {
                            text: "Capture monitor"
                        }
                        ComboBox {
                            id: footerMonitor
                            objectName: "capture-monitor"
                            Layout.fillWidth: true
                            model: root.backend.monitors
                            textRole: "name"
                            currentIndex: {
                                for (let i = 0; i < model.length; ++i) {
                                    if (model[i].name === root.backend.selectedMonitor)
                                        return i;
                                }
                                return -1;
                            }
                            onActivated: root.backend.selectMonitor(model[currentIndex].name)
                            Accessible.name: "Capture monitor"
                            Accessible.description: "Changes the KMS capture display immediately."
                        }
                        Button {
                            text: "Refresh"
                            enabled: !root.backend.busy && !root.backend.deviceBusy
                            onClicked: root.backend.poll()
                            Accessible.name: "Refresh monitor list"
                        }
                    }
                    Label {
                        Layout.fillWidth: true
                        visible: root.backend.monitors.length === 0
                        text: "No KMS displays are available. Connect a display or check the DRM device."
                        wrapMode: Text.Wrap
                        color: Kirigami.Theme.disabledTextColor
                    }
                    Label {
                        Layout.fillWidth: true
                        visible: root.backend.monitorError.length > 0
                        text: "Could not refresh displays; the last-known monitor list is shown. Open Diagnostics and help for recovery steps."
                        wrapMode: Text.Wrap
                        color: Kirigami.Theme.negativeTextColor
                        Accessible.role: Accessible.AlertMessage
                    }
                }
            }
            ScrollView {
                id: fineTuneScroll
                Accessible.name: "Fine tune controls"
                Layout.fillWidth: true
                Layout.fillHeight: true
                contentWidth: availableWidth
                contentHeight: fineTuneContent.implicitHeight
                clip: true
                ScrollBar.vertical.policy: ScrollBar.AsNeeded
                ScrollBar.vertical.interactive: true
                ColumnLayout {
                    id: fineTuneContent
                    width: fineTuneScroll.availableWidth
                    spacing: Kirigami.Units.smallSpacing
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: Kirigami.Units.smallSpacing
                        enabled: !root.backend.busy && !root.backend.deviceBusy && root.backend.status !== "starting" && root.backend.status !== "stopping"

                        Label {
                            Layout.fillWidth: true
                            text: "Use the reset button beside a value to restore that control's default."
                            wrapMode: Text.Wrap
                            color: Kirigami.Theme.disabledTextColor
                        }
                        SettingSlider {
                            backend: root.backend
                            settingKey: "saturation_boost"
                            label: "Saturation boost"
                            description: "Higher values emphasize saturated pixels and boost color intensity."
                            minimum: 0
                            maximum: 3
                            decimals: 2
                            defaultValue: 1.5
                            onValueEdited: (key, value) => root.noteFineTuneEdit(key, value)
                            objectName: "control-saturation_boost"
                        }
                        SettingSlider {
                            backend: root.backend
                            settingKey: "smooth"
                            label: "Response (higher = faster)"
                            description: "Lower values smooth flicker but respond more slowly; higher values follow changes faster."
                            minimum: 0.05
                            maximum: 1
                            decimals: 2
                            defaultValue: 0.35
                            onValueEdited: (key, value) => root.noteFineTuneEdit(key, value)
                            objectName: "control-smooth"
                        }
                        SettingSlider {
                            backend: root.backend
                            settingKey: "max_step"
                            label: "Maximum step"
                            description: "Limits each frame's color change; lower values are smoother but add lag."
                            minimum: 1
                            maximum: 255
                            defaultValue: 12
                            onValueEdited: (key, value) => root.noteFineTuneEdit(key, value)
                            objectName: "control-max_step"
                        }
                        SettingSlider {
                            backend: root.backend
                            settingKey: "edge_depth"
                            label: "Edge sample depth (%)"
                            description: "How far inward from each screen edge to sample; higher values pick up nearby content sooner."
                            minimum: 5
                            maximum: 20
                            defaultValue: 8
                            onValueEdited: (key, value) => root.noteFineTuneEdit(key, value)
                            objectName: "control-edge_depth"
                        }
                        SettingSlider {
                            backend: root.backend
                            settingKey: "center_bleed"
                            label: "Center color blend (%)"
                            description: "Mix this share of the central picture's average color into every edge segment."
                            minimum: 0
                            maximum: 50
                            defaultValue: 0
                            onValueEdited: (key, value) => root.noteFineTuneEdit(key, value)
                            objectName: "control-center_bleed"
                        }
                        SettingSlider {
                            backend: root.backend
                            settingKey: "peak_mix"
                            label: "Brightest-pixel blend (%)"
                            description: "Mix this share of the brightest-luma pixel in each zone; lower values suppress tiny highlights."
                            minimum: 0
                            maximum: 35
                            defaultValue: 15
                            onValueEdited: (key, value) => root.noteFineTuneEdit(key, value)
                            objectName: "control-peak_mix"
                        }
                        SettingSlider {
                            backend: root.backend
                            settingKey: "fps"
                            label: "Capture/send rate (10 Hz ceiling)"
                            description: "Samples and sends at the selected rate, within the controller's documented 10 Hz limit."
                            minimum: 1
                            maximum: 10
                            defaultValue: 10
                            onValueEdited: (key, value) => root.noteFineTuneEdit(key, value)
                            objectName: "control-fps"
                        }
                        SettingSlider {
                            backend: root.backend
                            settingKey: "feather_depth"
                            label: "Inward feather depth (%)"
                            description: "Extend each edge zone inward with a smooth taper to zero; 0 disables the feather."
                            minimum: 0
                            maximum: 20
                            defaultValue: 5
                            onValueEdited: (key, value) => root.noteFineTuneEdit(key, value)
                            objectName: "control-feather_depth"
                        }
                        SettingSlider {
                            backend: root.backend
                            settingKey: "black_threshold"
                            label: "Black threshold"
                            description: "Suppresses near-black capture noise; increase it if dark areas glow."
                            minimum: 0
                            maximum: 0.1
                            decimals: 3
                            defaultValue: 0.01
                            onValueEdited: (key, value) => root.noteFineTuneEdit(key, value)
                            objectName: "control-black_threshold"
                        }
                        CheckBox {
                            objectName: "setting-letterbox"
                            text: "Letterbox detection"
                            checked: Boolean(root.backend.settings.letterbox)
                            onClicked: {
                                root.noteFineTuneEdit("letterbox", checked);
                                root.backend.setSetting("letterbox", checked);
                            }
                            Accessible.description: "Detects black bars and samples within the active picture. Default: on."
                            ToolTip.text: Accessible.description
                            ToolTip.visible: hovered
                        }
                        CheckBox {
                            objectName: "setting-midtone_lift"
                            text: "Midtone lift"
                            checked: Boolean(root.backend.settings.midtone_lift)
                            onClicked: {
                                root.noteFineTuneEdit("midtone_lift", checked);
                                root.backend.setSetting("midtone_lift", checked);
                            }
                            Accessible.description: "Optional legacy lift for dim and midtone content. Default: off."
                            ToolTip.text: Accessible.description
                            ToolTip.visible: hovered
                        }
                        CheckBox {
                            objectName: "setting-strict_black"
                            text: "Strict black"
                            checked: Boolean(root.backend.settings.strict_black)
                            onClicked: {
                                root.noteFineTuneEdit("strict_black", checked);
                                root.backend.setSetting("strict_black", checked);
                            }
                            Accessible.description: "Sends exact black immediately instead of smoothing dark transitions. Default: off."
                            ToolTip.text: Accessible.description
                            ToolTip.visible: hovered
                        }
                        Button {
                            text: root.backend.settingsRetryAvailable ? "Retry settings" : "Settings applying…"
                            visible: root.backend.settingsPending && root.backend.settingsError.length > 0
                            enabled: root.backend.settingsRetryAvailable
                            onClicked: root.backend.retrySettings()
                            Accessible.name: "Retry pending settings"
                        }
                    }
                }
            }
        }
    }

    Timer {
        id: previewTimer
        interval: 100
        repeat: true
        running: root.expanded && root.backend.running && root.backend.previewReady
        onTriggered: root.backend.getPreview()
    }

    Dialog {
        id: deviceDialog
        title: "Nanoleaf 4D details"
        modal: true
        standardButtons: Dialog.Close
        contentItem: ColumnLayout {
            width: Kirigami.Units.gridUnit * 22
            Label {
                Layout.fillWidth: true
                visible: root.backend.deviceInfoStale
                text: "Details are stale; refresh to retry."
                wrapMode: Text.Wrap
                color: Kirigami.Theme.neutralTextColor
            }
            Repeater {
                model: [
                    {
                        label: "Name",
                        key: "name"
                    },
                    {
                        label: "Model",
                        key: "model"
                    },
                    {
                        label: "Serial",
                        key: "serialNo"
                    },
                    {
                        label: "Firmware",
                        key: "firmwareVersion"
                    },
                    {
                        label: "Address",
                        key: "host"
                    }
                ]
                delegate: TextField {
                    required property var modelData
                    Layout.fillWidth: true
                    visible: modelData.key === "host" ? root.backend.deviceHost.length > 0 : String(root.backend.deviceInfo[modelData.key] || "").length > 0
                    text: modelData.label + ": " + (modelData.key === "host" ? root.backend.deviceHost : String(root.backend.deviceInfo[modelData.key] || ""))
                    readOnly: true
                    selectByMouse: true
                    Accessible.name: modelData.label
                    Accessible.description: "Read-only device detail; select the value and press Control+C to copy."
                }
            }
            Button {
                Layout.alignment: Qt.AlignLeft
                text: root.backend.deviceBusy ? "Refreshing…" : "Refresh details"
                enabled: !root.backend.deviceBusy
                onClicked: root.backend.refreshDeviceInfo()
            }
        }
    }

    Dialog {
        id: aboutDialog
        title: "About capture"
        modal: true
        standardButtons: Dialog.Close
        contentItem: Label {
            width: Kirigami.Units.gridUnit * 22
            text: "Wayleaf 4D reads the selected display directly through KMS. It bypasses the KDE portal and its privacy indicator, and does not save images."
            wrapMode: Text.Wrap
            padding: Kirigami.Units.largeSpacing
        }
    }

    Dialog {
        id: diagnosticsDialog
        title: "Diagnostics and help"
        modal: true
        standardButtons: Dialog.Close
        contentItem: ColumnLayout {
            width: Kirigami.Units.gridUnit * 22
            Label {
                Layout.fillWidth: true
                text: "Status: " + (root.backend.running ? "synchronizing" : root.backend.status === "error" || root.backend.error.length > 0 ? "needs attention" : root.backend.status)
                wrapMode: Text.Wrap
            }
            Label {
                Layout.fillWidth: true
                text: "If the control service is unavailable, check that wayleaf-4d is installed and inspect the user journal. For pairing, hold the controller's power button for 5 seconds."
                wrapMode: Text.Wrap
            }
            RowLayout {
                Button {
                    text: "Retry connection"
                    onClicked: root.backend.poll()
                }
                Button {
                    text: "Clear error"
                    enabled: root.backend.error.length > 0
                    onClicked: root.backend.clearError()
                }
                Button {
                    text: "Open project help"
                    onClicked: Qt.openUrlExternally("https://github.com/avitretiak/wayleaf-4d")
                }
            }
        }
    }

    Dialog {
        id: disconnectDialog
        title: root.backend.running ? "Stop sync and disconnect?" : "Disconnect this device?"
        modal: true
        standardButtons: Dialog.Cancel | Dialog.Ok
        contentItem: Label {
            width: Kirigami.Units.gridUnit * 22
            text: (root.backend.running ? "Synchronization will stop and the device will be asked to power off. " : "Synchronization is already stopped. ") + "The saved address and token will be removed from this computer. Controller authorization remains enabled."
            wrapMode: Text.Wrap
            padding: Kirigami.Units.largeSpacing
        }
        onAccepted: root.backend.disconnectDevice()
    }
}
