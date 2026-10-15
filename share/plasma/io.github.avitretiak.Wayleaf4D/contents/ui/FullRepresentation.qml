pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Window
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Dialogs
import QtQuick.Effects
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
    property bool fineTuneExpanded: Boolean(Plasmoid.configuration.fineTuneExpanded)
    readonly property var presetKeys: ["saturation_boost", "smooth", "max_step", "edge_depth", "feather_depth", "center_bleed", "peak_mix", "black_threshold", "fps"]
    readonly property var legacyPresetKeys: ["saturation_boost", "smooth", "max_step", "edge_depth", "center_bleed", "peak_mix", "fps"]
    readonly property var settingDefaults: ({
        saturation_boost: 1.5, smooth: 0.35, max_step: 12, edge_depth: 8,
        feather_depth: 5, center_bleed: 0, peak_mix: 15, black_threshold: 0.01,
        fps: 10, letterbox: true, midtone_lift: false, strict_black: false
    })
    readonly property var swatches: [
        { name: "Warm white", color: "#ffc58f" },
        { name: "Neutral white", color: "#fff4e5" },
        { name: "Cool white", color: "#eaf2ff" },
        { name: "Red", color: "#ff3b30" },
        { name: "Orange", color: "#ff9500" },
        { name: "Green", color: "#34c759" },
        { name: "Blue", color: "#007aff" },
        { name: "Purple", color: "#af52de" },
        { name: "Pink", color: "#ff2d55" }
    ]
    readonly property string mode: String(backend.settings.mode || "4D")
    readonly property color staticColor: Qt.color(String(backend.settings.static_color || "#ffffff"))
    readonly property real popupHeightLimit: Screen.height * 0.7
    readonly property real bodyHeightLimit: Math.max(Kirigami.Units.gridUnit * 5, Screen.height * 0.44)
    readonly property var fineTuneKeys: mode === "1D" ? []
        : mode === "2D" ? ["saturation_boost", "smooth", "fps"]
            : mode === "3D" ? ["saturation_boost", "smooth", "max_step", "edge_depth", "peak_mix", "fps"]
                : ["saturation_boost", "smooth", "midtone_lift", "edge_depth", "feather_depth", "max_step", "center_bleed", "peak_mix", "black_threshold", "letterbox", "strict_black", "fps"]
    readonly property int fineTuneCount: fineTuneKeys.length

    implicitWidth: Kirigami.Units.gridUnit * 20
    implicitHeight: Math.min(content.implicitHeight, popupHeightLimit)
    Accessible.name: "Wayleaf 4D controls"
    clip: true

    function presetValues(name) {
        if (name === "Subtle")
            return ({ saturation_boost: 1.15, smooth: 0.25, max_step: 8, edge_depth: 8,
                feather_depth: 5, center_bleed: 0, peak_mix: 5, black_threshold: 0.01, fps: 10 })
        if (name === "Balanced")
            return ({ saturation_boost: 1.5, smooth: 0.35, max_step: 12, edge_depth: 8,
                feather_depth: 5, center_bleed: 0, peak_mix: 15, black_threshold: 0.01, fps: 10 })
        if (name === "Vivid")
            return ({ saturation_boost: 1.9, smooth: 0.5, max_step: 18, edge_depth: 8,
                feather_depth: 5, center_bleed: 0, peak_mix: 25, black_threshold: 0.01, fps: 10 })
        return null
    }

    function valuesFromSettings(settings) {
        const values = ({})
        for (const key of presetKeys)
            values[key] = Number(settings[key])
        return values
    }

    function samePreset(values, preset) {
        const expected = presetValues(preset)
        if (!expected)
            return false
        for (const key of presetKeys) {
            if (Number(values[key]) !== Number(expected[key]))
                return false
        }
        return true
    }

    function validCustomSettings(values) {
        if (!values || typeof values !== "object" || Array.isArray(values))
            return false
        const bounds = {
            saturation_boost: [0, 3], smooth: [0.05, 1], max_step: [1, 255],
            edge_depth: [5, 20], feather_depth: [0, 20], center_bleed: [0, 50],
            peak_mix: [0, 35], black_threshold: [0, 0.1], fps: [1, 10]
        }
        for (const key of legacyPresetKeys) {
            const value = Number(values[key])
            if (!Number.isFinite(value) || value < bounds[key][0] || value > bounds[key][1])
                return false
        }
        for (const key of ["feather_depth", "black_threshold"]) {
            if (Object.prototype.hasOwnProperty.call(values, key)) {
                const value = Number(values[key])
                if (!Number.isFinite(value) || value < bounds[key][0] || value > bounds[key][1])
                    return false
            }
        }
        return true
    }

    function loadCustomSettings() {
        try {
            const saved = JSON.parse(String(Plasmoid.configuration.customSettings || ""))
            if (!validCustomSettings(saved))
                return false
            customSettings = Object.assign(valuesFromSettings(backend.settings), saved)
            return true
        } catch (e) {
            return false
        }
    }

    function saveCustomSettings(values) {
        customSettings = values
        Plasmoid.configuration.customSettings = JSON.stringify(values)
        customSettingsPersisted = true
    }

    function colorName() {
        const color = String(backend.settings.static_color || "#ffffff").toLowerCase()
        for (const swatch of swatches) {
            if (swatch.color === color)
                return swatch.name
        }
        return "Custom"
    }

    function modeDescription(value) {
        if (value === "1D") return "Static color ambience"
        if (value === "2D") return "Dominant color from the screen"
        if (value === "3D") return "Several colors from the screen"
        return "Full immersive edge mirroring"
    }

    function statusText() {
        if (backend.error.length > 0 || backend.status === "error")
            return backend.error.length > 0 ? "Error · " + backend.error : "Synchronization needs attention"
        if (backend.status === "starting") return "Starting synchronization…"
        if (backend.status === "stopping") return "Stopping synchronization…"
        if (backend.running && mode === "1D") return "On · " + colorName()
        if (backend.running) return "Syncing · " + Number(backend.settings.fps) + " Hz · " + backend.selectedMonitor
        return backend.deviceConnected ? "Off · " + modeDescription(mode) : "No Nanoleaf 4D paired"
    }

    function syncPreview() {
        backend.setPreviewEnabled(expanded && backend.running && mode !== "1D")
    }

    function captureInitialSettings() {
        if (userTouchedSettings)
            return
        const current = valuesFromSettings(backend.settings)
        if (!customSettingsPersisted)
            customSettings = current
        selectedPreset = samePreset(current, "Balanced") ? "Balanced"
            : samePreset(current, "Subtle") ? "Subtle"
                : samePreset(current, "Vivid") ? "Vivid" : "Custom"
    }

    function noteFineTuneEdit(key, value) {
        if (presetKeys.indexOf(key) < 0)
            return
        const next = valuesFromSettings(backend.settings)
        next[key] = value
        saveCustomSettings(next)
        userTouchedSettings = true
        selectedPreset = "Custom"
    }

    function choosePreset(name) {
        userTouchedSettings = true
        selectedPreset = name
        if (name === "Custom") {
            for (const key of presetKeys)
                backend.setSetting(key, customSettings[key])
            return
        }
        const values = presetValues(name)
        for (const key of presetKeys)
            backend.setSetting(key, values[key])
    }

    function settingModified(key) {
        const current = backend.settings[key]
        const original = settingDefaults[key]
        return typeof original === "boolean" ? Boolean(current) !== original : Math.abs(Number(current) - original) > 0.000001
    }

    function groupModified(keys) {
        return keys.some(function(key) { return settingModified(key) })
    }

    function resetGroup(keys) {
        const next = valuesFromSettings(backend.settings)
        for (const key of keys) {
            if (presetKeys.indexOf(key) >= 0)
                next[key] = settingDefaults[key]
        }
        saveCustomSettings(next)
        userTouchedSettings = true
        selectedPreset = "Custom"
        for (const key of keys)
            backend.setSetting(key, settingDefaults[key])
    }

    function resetAllFineTune() {
        const values = valuesFromSettings(backend.settings)
        for (const key of fineTuneKeys) {
            if (presetKeys.indexOf(key) >= 0)
                values[key] = settingDefaults[key]
        }
        saveCustomSettings(values)
        userTouchedSettings = true
        selectedPreset = samePreset(values, "Balanced") ? "Balanced" : "Custom"
        for (const key of fineTuneKeys)
            backend.setSetting(key, settingDefaults[key])
    }

    function uniformZones() {
        const live = mode !== "1D" && backend.previewZones.length > 0 ? backend.previewZones[0] : null
        const fallback = mode !== "1D" && !live
        const channels = live ? [Number(live.r), Number(live.g), Number(live.b)]
            : [Math.round(staticColor.r * 255), Math.round(staticColor.g * 255), Math.round(staticColor.b * 255)]
        return ["top", "right", "bottom", "left"].map(function(edge) {
            return { edge: edge, position: 0.5, span: 1, r: channels[0], g: channels[1], b: channels[2], fallback: fallback }
        })
    }

    function edgeZones(counts) {
        const zones = []
        const addEdge = function(edge, count) {
            for (let i = 0; i < count; ++i)
                zones.push({ edge: edge, position: (i + 0.5) / count, span: 1 / count,
                    r: 0, g: 0, b: 0, fallback: true })
        }
        addEdge("top", counts[0])
        addEdge("right", counts[1])
        addEdge("bottom", counts[2])
        addEdge("left", counts[3])
        return zones
    }

    function staticZones() {
        return edgeZones([8, 6, 8, 6])
    }

    function coarseZones() {
        return edgeZones([3, 2, 3, 2])
    }

    function colorHex(color) {
        const channel = function(value) { return ("0" + Math.round(value * 255).toString(16)).slice(-2) }
        return ("#" + channel(color.r) + channel(color.g) + channel(color.b)).toUpperCase()

    }
    Component.onCompleted: {
        customSettingsPersisted = loadCustomSettings()
        captureInitialSettings()
        syncPreview()
    }

    onExpandedChanged: syncPreview()
    onFineTuneExpandedChanged: Plasmoid.configuration.fineTuneExpanded = fineTuneExpanded
    Connections {
        target: root.backend
        function onRunningChanged() { root.syncPreview() }
        function onSettingsChanged() {
            root.captureInitialSettings()
            root.syncPreview()
        }
    }
    Component.onDestruction: backend.setPreviewEnabled(false)

    ColumnLayout {
        id: content
        anchors.fill: parent
        anchors.margins: Kirigami.Units.smallSpacing * 2
        spacing: Kirigami.Units.smallSpacing

        Item {
            id: glowFrame
            objectName: "live-preview"
            Layout.fillWidth: true
            Layout.preferredHeight: headerContent.implicitHeight + 32
            implicitHeight: headerContent.implicitHeight + 32
            property var visibleZones: !root.expanded ? []
                : root.mode === "1D" || root.mode === "2D" ? root.uniformZones()
                    : root.mode === "3D" ? (root.backend.previewZones.length > 0 ? root.backend.previewZones : root.coarseZones())
                        : root.backend.previewZones.length > 0 ? root.backend.previewZones : root.staticZones()
            Item {
                id: glowStrips
                anchors.fill: parent
                Accessible.ignored: true
                opacity: root.backend.running ? 0.95 : 0.3
                visible: root.expanded
                layer.enabled: root.expanded
                layer.effect: MultiEffect {
                    blurEnabled: true
                    blur: 0.35
                    blurMax: 2
                }
                Repeater {
                    model: glowFrame.visibleZones
                    delegate: Rectangle {
                        required property var modelData
                        readonly property string edge: String(modelData.edge)
                        readonly property real segmentSpan: Math.max(0, Number(modelData.span))
                        readonly property real position: Math.max(0, Math.min(1, Number(modelData.position)))
                        readonly property real start: Math.max(0, position - segmentSpan / 2)
                        readonly property real end: Math.min(1, position + segmentSpan / 2)
                        readonly property real span: Math.max(0, end - start)
                        readonly property real edgeLength: edge === "top" || edge === "bottom" ? glowFrame.width - 32 : glowFrame.height - 32
                        readonly property real brightness: Math.max(0, Math.min(1.35, Number(root.backend.settings.brightness) / 1.5))
                        readonly property bool liveZone: !Boolean(modelData.fallback)
                        readonly property color colorAtBrightness: Qt.rgba(
                            Math.min(1, Number(modelData.r) / 255 * brightness),
                            Math.min(1, Number(modelData.g) / 255 * brightness),
                            Math.min(1, Number(modelData.b) / 255 * brightness), 1)
                        x: edge === "top" || edge === "bottom" ? 16 + edgeLength * start : edge === "right" ? glowFrame.width - 14 : 8
                        y: edge === "left" || edge === "right" ? 16 + edgeLength * start : edge === "bottom" ? glowFrame.height - 14 : 8
                        width: edge === "top" || edge === "bottom" ? Math.max(2, edgeLength * span - 2) : 6
                        height: edge === "left" || edge === "right" ? Math.max(2, edgeLength * span - 2) : 6
                        color: !root.backend.running || (root.mode !== "1D" && !liveZone)
                            ? Kirigami.Theme.disabledTextColor
                            : root.mode === "1D"
                                ? Qt.rgba(Math.min(1, root.staticColor.r * brightness), Math.min(1, root.staticColor.g * brightness), Math.min(1, root.staticColor.b * brightness), 1)
                                : colorAtBrightness
                        radius: 3
                        ToolTip.visible: root.mode === "4D" && zoneHover.hovered
                        ToolTip.text: "Controller zone " + modelData.id + " · " + edge + " · " + Math.round(position * 100) + "%"
                        HoverHandler { id: zoneHover }
                        Behavior on color {
                            ColorAnimation { duration: Kirigami.Units.longDuration === 0 ? 0 : 80 }
                        }
                    }
                }
            }
            ColumnLayout {
                id: headerContent
                anchors.fill: parent
                anchors.margins: 16
                spacing: Kirigami.Units.smallSpacing

                RowLayout {
                    Layout.fillWidth: true
                    spacing: Kirigami.Units.smallSpacing
                    Rectangle {
                        Layout.alignment: Qt.AlignVCenter
                        Layout.preferredWidth: 9
                        Layout.preferredHeight: 9
                        radius: width / 2
                        color: root.backend.error.length > 0 || root.backend.status === "error" ? Kirigami.Theme.negativeTextColor
                            : root.backend.status === "starting" || root.backend.status === "stopping" ? Kirigami.Theme.neutralTextColor
                                : root.backend.running ? Kirigami.Theme.positiveTextColor : Kirigami.Theme.disabledTextColor
                        Accessible.ignored: true
                    }
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 0
                        Label { text: "Wayleaf 4D"; font.bold: true }
                        Label {
                            Layout.fillWidth: true
                            text: root.statusText()
                            color: root.backend.error.length > 0 || root.backend.status === "error" ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.disabledTextColor
                            font: Kirigami.Theme.smallFont
                            elide: Text.ElideMiddle
                            Accessible.name: text
                            Accessible.role: root.backend.error.length > 0 ? Accessible.AlertMessage : Accessible.StaticText
                        }
                    }
                    Switch {
                        id: syncSwitch
                        objectName: "sync-switch"
                        text: root.backend.running ? "Stop" : "Start"
                        checked: root.backend.running
                        enabled: root.backend.deviceConnected && (root.mode === "1D" || root.backend.selectedMonitor.length > 0)
                            && !root.backend.busy && !root.backend.deviceBusy
                            && root.backend.status !== "starting" && root.backend.status !== "stopping"
                        onClicked: {
                            if (checked !== root.backend.running)
                                checked ? root.backend.run() : root.backend.stop()
                        }
                        Accessible.name: "Start or stop output"
                        Accessible.description: checked ? "On. Toggle to stop output." : !root.backend.deviceConnected
                            ? "Disabled until a Nanoleaf 4D is paired."
                            : root.mode !== "1D" && root.backend.selectedMonitor.length === 0
                                ? "Disabled until a capture monitor is selected." : "Off. Toggle to start output."
                        ToolTip.text: Accessible.description
                        ToolTip.visible: hovered
                    }
                    ToolButton {
                        icon.name: "overflow-menu"
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
                                text: "Retry settings"
                                visible: root.backend.settingsPending && root.backend.settingsError.length > 0
                                enabled: root.backend.settingsRetryAvailable
                                onTriggered: root.backend.retrySettings()
                            }
                            MenuItem {
                                text: "Start at login"
                                objectName: "start-at-login-switch"
                                checkable: true
                                checked: root.backend.startAtLogin
                                enabled: !root.backend.busy && !root.backend.deviceBusy
                                onTriggered: root.backend.setStartAtLogin(checked)
                            }
                            MenuItem { text: "About capture…"; onTriggered: aboutDialog.open() }
                            MenuItem { text: "Diagnostics and help…"; onTriggered: diagnosticsDialog.open() }
                            MenuSeparator {}
                            MenuItem {
                                text: root.backend.running ? "Stop sync and disconnect…" : "Disconnect…"
                                enabled: true
                                onTriggered: disconnectDialog.open()
                            }
                        }
                    }
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: Kirigami.Units.smallSpacing
                    ButtonGroup { id: modeGroup; exclusive: true }
                    Repeater {
                        id: modeRepeater
                        model: ["1D", "2D", "3D", "4D"]
                        delegate: Button {
                            required property string modelData
                            readonly property int modeIndex: ["1D", "2D", "3D", "4D"].indexOf(modelData)
                            ButtonGroup.group: modeGroup
                            Layout.fillWidth: true
                            text: modelData
                            checkable: true
                            checked: root.mode === modelData
                            highlighted: checked
                            enabled: !root.backend.busy && !root.backend.deviceBusy
                            onClicked: root.backend.setSetting("mode", modelData)
                            Accessible.name: modelData + " mode"
                            Accessible.description: root.modeDescription(modelData)
                            ToolTip.text: Accessible.description
                            ToolTip.visible: hovered || activeFocus
                            Keys.onLeftPressed: {
                                if (modeIndex > 0) {
                                    root.backend.setSetting("mode", ["1D", "2D", "3D", "4D"][modeIndex - 1])
                                    modeRepeater.itemAt(modeIndex - 1).forceActiveFocus()
                                }
                            }
                            Keys.onRightPressed: {
                                if (modeIndex < 3) {
                                    root.backend.setSetting("mode", ["1D", "2D", "3D", "4D"][modeIndex + 1])
                                    modeRepeater.itemAt(modeIndex + 1).forceActiveFocus()
                                }
                            }
                        }
                    }
                }
                Label {
                    Layout.fillWidth: true
                    text: root.modeDescription(root.mode)
                    color: Kirigami.Theme.disabledTextColor
                    font: Kirigami.Theme.smallFont
                }
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing
            Kirigami.Icon {
                source: "brightness-high"
                implicitWidth: Kirigami.Units.iconSizes.small
                implicitHeight: implicitWidth
                Accessible.ignored: true
            }
            SettingSlider {
                backend: root.backend
                Layout.fillWidth: true
                settingKey: "brightness"
                label: "Brightness"
                description: "Scales the output brightness."
                minimum: 0
                maximum: 2
                decimals: 0
                valueMultiplier: 50
                displayMultiplier: 50
                suffix: "%"
                defaultValue: 1.5
                objectName: "control-brightness"
            }
        }

        ScrollView {
            id: bodyScroll
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.minimumHeight: 0
            Layout.preferredHeight: Math.min(bodyContent.implicitHeight, root.bodyHeightLimit)
            Layout.maximumHeight: root.bodyHeightLimit
            contentWidth: availableWidth
            contentHeight: bodyContent.implicitHeight
            clip: true
            ScrollBar.vertical.policy: ScrollBar.AsNeeded
            ScrollBar.vertical.interactive: true
            Accessible.name: "Mode and fine-tune controls"

            ColumnLayout {
                id: bodyContent
                width: bodyScroll.availableWidth
                spacing: Kirigami.Units.smallSpacing


                ColumnLayout {
                    Layout.fillWidth: true
                    visible: root.mode === "1D"
                    spacing: Kirigami.Units.smallSpacing
                    Label { text: "Color"; font.bold: true }
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 6
                        Repeater {
                            model: root.swatches
                            delegate: ToolButton {
                                id: swatchButton
                                required property var modelData
                                Layout.preferredWidth: 22
                                Layout.preferredHeight: 22
                                padding: 0
                                enabled: !root.backend.busy && !root.backend.deviceBusy
                                checkable: true
                                checked: String(root.backend.settings.static_color).toLowerCase() === modelData.color
                                Accessible.name: modelData.name
                                Accessible.description: "Set static output color to " + modelData.name
                                ToolTip.text: modelData.name
                                ToolTip.visible: hovered
                                onClicked: root.backend.setSetting("static_color", modelData.color)
                                background: Rectangle {
                                    anchors.fill: parent
                                    anchors.margins: swatchButton.checked ? -2 : 0
                                    radius: width / 2
                                    color: swatchButton.checked ? "transparent" : modelData.color
                                    border.width: swatchButton.checked ? 2 : 1
                                    border.color: swatchButton.checked ? Kirigami.Theme.highlightColor : Kirigami.Theme.textColor
                                    Rectangle {
                                        visible: swatchButton.checked
                                        anchors.fill: parent
                                        anchors.margins: 4
                                        radius: width / 2
                                        color: modelData.color
                                    }
                                }
                            }
                        }
                        ToolButton {
                            id: customColorButton
                            Layout.preferredWidth: 22
                            Layout.preferredHeight: 22
                            padding: 0
                            enabled: !root.backend.busy && !root.backend.deviceBusy
                            checkable: true
                            checked: root.colorName() === "Custom"
                            text: "+"
                            Accessible.name: "Choose a custom color"
                            ToolTip.text: "Choose a custom color"
                            ToolTip.visible: hovered
                            onClicked: {
                                colorDialog.selectedColor = root.staticColor
                                colorDialog.open()
                            }
                            background: Rectangle {
                                anchors.fill: parent
                                anchors.margins: customColorButton.checked ? -2 : 0
                                radius: width / 2
                                color: customColorButton.checked ? "transparent" : root.staticColor
                                border.width: customColorButton.checked ? 2 : 1
                                border.color: customColorButton.checked ? Kirigami.Theme.highlightColor : Kirigami.Theme.textColor
                                Rectangle {
                                    visible: customColorButton.checked
                                    anchors.fill: parent
                                    anchors.margins: 4
                                    radius: width / 2
                                    color: root.staticColor
                                }
                            }
                        }
                    }
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    visible: root.mode !== "1D"
                    spacing: Kirigami.Units.smallSpacing
                    Label { text: "Look"; font.bold: true }
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: Kirigami.Units.smallSpacing / 2
                        ButtonGroup { id: lookGroup; exclusive: true }
                        Repeater {
                            model: ["Subtle", "Balanced", "Vivid", "Custom"]
                            delegate: Button {
                                required property string modelData
                                ButtonGroup.group: lookGroup
                                Layout.fillWidth: true
                                text: modelData
                                checkable: true
                                checked: root.selectedPreset === modelData
                                highlighted: checked
                                enabled: !root.backend.busy && !root.backend.deviceBusy
                                onClicked: root.choosePreset(modelData)
                                Accessible.name: modelData + " look"
                            }
                        }
                    }
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    visible: root.mode !== "1D"
                    spacing: Kirigami.Units.smallSpacing
                    RowLayout {
                        Layout.fillWidth: true
                        ToolButton {
                            Layout.fillWidth: true
                            text: (root.fineTuneExpanded ? "▾" : "▸") + "  Fine tune · " + root.fineTuneCount + " settings"
                            flat: true
                            onClicked: root.fineTuneExpanded = !root.fineTuneExpanded
                            Accessible.name: "Fine tune, " + root.fineTuneCount + " settings"
                            Accessible.description: root.fineTuneExpanded ? "Hide fine-tune settings" : "Show fine-tune settings"
                        }
                        ToolButton {
                            text: "Reset all"
                            visible: root.groupModified(root.fineTuneKeys)
                            enabled: !root.backend.busy && !root.backend.deviceBusy
                            onClicked: root.resetAllFineTune()
                            Accessible.name: "Reset all fine-tune settings"
                        }
                    }
                    ColumnLayout {
                        Layout.fillWidth: true
                        visible: root.fineTuneExpanded
                        enabled: !root.backend.busy && !root.backend.deviceBusy && root.backend.status !== "starting" && root.backend.status !== "stopping"
                        spacing: Kirigami.Units.smallSpacing

                        ColumnLayout {
                            Layout.fillWidth: true
                            visible: root.mode !== "1D"
                            RowLayout {
                                Layout.fillWidth: true
                                Label { text: "Color"; color: Kirigami.Theme.disabledTextColor; font: Kirigami.Theme.smallFont }
                                Item { Layout.fillWidth: true }
                                ToolButton {
                                    text: "Reset"
                                    visible: root.groupModified(root.mode === "4D" ? ["saturation_boost", "smooth", "midtone_lift"] : ["saturation_boost", "smooth"])
                                    enabled: !root.backend.busy && !root.backend.deviceBusy
                                    onClicked: root.resetGroup(root.mode === "4D" ? ["saturation_boost", "smooth", "midtone_lift"] : ["saturation_boost", "smooth"])
                                }
                            }
                            SettingSlider {
                                backend: root.backend; Layout.fillWidth: true; settingKey: "saturation_boost"
                                label: "Saturation"; description: "Emphasizes saturated screen colors."
                                minimum: 0; maximum: 3; decimals: 2; defaultValue: 1.5
                                onValueEdited: (key, value) => root.noteFineTuneEdit(key, value)
                            }
                            SettingSlider {
                                backend: root.backend; Layout.fillWidth: true; settingKey: "smooth"
                                label: "Response"; description: "Higher values follow screen changes faster."
                                minimum: 0.05; maximum: 1; decimals: 2; defaultValue: 0.35
                                onValueEdited: (key, value) => root.noteFineTuneEdit(key, value)
                            }
                            RowLayout {
                                Layout.fillWidth: true
                                visible: root.mode === "4D"
                                Label { text: "Midtone lift"; Layout.fillWidth: true }
                                Switch {
                                    objectName: "setting-midtone_lift"
                                    checked: Boolean(root.backend.settings.midtone_lift)
                                    onClicked: root.backend.setSetting("midtone_lift", checked)
                                    Accessible.name: "Midtone lift"
                                    Accessible.description: "Optional lift for dim and midtone colors."
                                }
                            }
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            visible: root.mode === "3D" || root.mode === "4D"
                            RowLayout {
                                Layout.fillWidth: true
                                Label { text: "Edges"; color: Kirigami.Theme.disabledTextColor; font: Kirigami.Theme.smallFont }
                                Item { Layout.fillWidth: true }
                                ToolButton {
                                    text: "Reset"
                                    visible: root.groupModified(root.mode === "4D" ? ["edge_depth", "feather_depth", "max_step"] : ["edge_depth", "max_step"])
                                    enabled: !root.backend.busy && !root.backend.deviceBusy
                                    onClicked: root.resetGroup(root.mode === "4D" ? ["edge_depth", "feather_depth", "max_step"] : ["edge_depth", "max_step"])
                                }
                            }
                            SettingSlider {
                                backend: root.backend; Layout.fillWidth: true; settingKey: "edge_depth"
                                label: "Sample depth"; description: "How far inward from each edge to sample."
                                minimum: 5; maximum: 20; defaultValue: 8; suffix: "%"
                                onValueEdited: (key, value) => root.noteFineTuneEdit(key, value)
                            }
                            SettingSlider {
                                backend: root.backend; Layout.fillWidth: true; settingKey: "feather_depth"
                                visible: root.mode === "4D"
                                label: "Inward feather"; description: "Tapers edge sampling inward."
                                minimum: 0; maximum: 20; defaultValue: 5; suffix: "%"
                                onValueEdited: (key, value) => root.noteFineTuneEdit(key, value)
                            }
                            SettingSlider {
                                backend: root.backend; Layout.fillWidth: true; settingKey: "max_step"
                                label: "Max step"; description: "Limits each update's color change."
                                minimum: 1; maximum: 255; defaultValue: 12
                                onValueEdited: (key, value) => root.noteFineTuneEdit(key, value)
                            }
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            visible: root.mode === "3D" || root.mode === "4D"
                            RowLayout {
                                Layout.fillWidth: true
                                Label { text: "Blend"; color: Kirigami.Theme.disabledTextColor; font: Kirigami.Theme.smallFont }
                                Item { Layout.fillWidth: true }
                                ToolButton {
                                    text: "Reset"
                                    visible: root.groupModified(root.mode === "4D" ? ["center_bleed", "peak_mix"] : ["peak_mix"])
                                    enabled: !root.backend.busy && !root.backend.deviceBusy
                                    onClicked: root.resetGroup(root.mode === "4D" ? ["center_bleed", "peak_mix"] : ["peak_mix"])
                                }
                            }
                            SettingSlider {
                                backend: root.backend; Layout.fillWidth: true; settingKey: "center_bleed"
                                visible: root.mode === "4D"
                                label: "Center color"; description: "Blends the central picture color into edge zones."
                                minimum: 0; maximum: 50; defaultValue: 0; suffix: "%"
                                onValueEdited: (key, value) => root.noteFineTuneEdit(key, value)
                            }
                            SettingSlider {
                                backend: root.backend; Layout.fillWidth: true; settingKey: "peak_mix"
                                label: "Brightest pixel"; description: "Blends the brightest pixel from each zone."
                                minimum: 0; maximum: 35; defaultValue: 15; suffix: "%"
                                onValueEdited: (key, value) => root.noteFineTuneEdit(key, value)
                            }
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            visible: root.mode === "4D"
                            RowLayout {
                                Layout.fillWidth: true
                                Label { text: "Black handling"; color: Kirigami.Theme.disabledTextColor; font: Kirigami.Theme.smallFont }
                                Item { Layout.fillWidth: true }
                                ToolButton {
                                    text: "Reset"
                                    visible: root.groupModified(["black_threshold", "letterbox", "strict_black"])
                                    enabled: !root.backend.busy && !root.backend.deviceBusy
                                    onClicked: root.resetGroup(["black_threshold", "letterbox", "strict_black"])
                                }
                            }
                            SettingSlider {
                                backend: root.backend; Layout.fillWidth: true; settingKey: "black_threshold"
                                label: "Threshold"; description: "Suppresses near-black capture noise."
                                minimum: 0; maximum: 0.1; decimals: 3; defaultValue: 0.01
                                onValueEdited: (key, value) => root.noteFineTuneEdit(key, value)
                            }
                            RowLayout {
                                Layout.fillWidth: true
                                Label { text: "Letterbox detection"; Layout.fillWidth: true }
                                Switch {
                                    objectName: "setting-letterbox"
                                    checked: Boolean(root.backend.settings.letterbox)
                                    onClicked: root.backend.setSetting("letterbox", checked)
                                    Accessible.name: "Letterbox detection"
                                    Accessible.description: "Detects black bars and samples the active picture."
                                }
                            }
                            RowLayout {
                                Layout.fillWidth: true
                                Label { text: "Strict black"; Layout.fillWidth: true }
                                Switch {
                                    objectName: "setting-strict_black"
                                    checked: Boolean(root.backend.settings.strict_black)
                                    onClicked: root.backend.setSetting("strict_black", checked)
                                    Accessible.name: "Strict black"
                                    Accessible.description: "Sends exact black immediately instead of smoothing dark transitions."
                                }
                            }
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            visible: root.mode !== "1D"
                            RowLayout {
                                Layout.fillWidth: true
                                Label { text: "Performance"; color: Kirigami.Theme.disabledTextColor; font: Kirigami.Theme.smallFont }
                                Item { Layout.fillWidth: true }
                                ToolButton {
                                    text: "Reset"
                                    visible: root.settingModified("fps")
                                    enabled: !root.backend.busy && !root.backend.deviceBusy
                                    onClicked: root.resetGroup(["fps"])
                            }
                            }
                            SettingSlider {
                                backend: root.backend; Layout.fillWidth: true; settingKey: "fps"
                                label: "Send rate"; description: "Samples and sends at up to 10 Hz."
                                minimum: 1; maximum: 10; defaultValue: 10; suffix: " Hz"
                                onValueEdited: (key, value) => root.noteFineTuneEdit(key, value)
                            }
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

                ColumnLayout {
                    Layout.fillWidth: true
                    visible: !root.backend.deviceConnected
                    spacing: Kirigami.Units.smallSpacing
                    Label { text: "Pair a Nanoleaf 4D"; font.bold: true }
                    Label {
                        Layout.fillWidth: true
                        text: "Search for a controller or enter its IPv4 address. Hold the 4D power button for 5 seconds before pairing; the pairing window lasts up to 30 seconds."
                        wrapMode: Text.Wrap
                    }
                    RowLayout {
                        Layout.fillWidth: true
                        Button {
                            text: root.backend.deviceBusy ? "Searching…" : "Find devices"
                            enabled: !root.backend.busy && !root.backend.deviceBusy
                            onClicked: {
                                deviceList.currentIndex = -1
                                root.backend.discoverDevices()
                            }
                        }
                        Label {
                            Layout.fillWidth: true
                            text: root.backend.deviceMessage
                            visible: root.backend.deviceMessage.length > 0
                            color: root.backend.deviceMessage.startsWith("Warning:") ? Kirigami.Theme.neutralTextColor : Kirigami.Theme.disabledTextColor
                            wrapMode: Text.Wrap
                        }
                    }
                    ListView {
                        id: deviceList
                        Layout.fillWidth: true
                        Layout.preferredHeight: Math.min(contentHeight, Kirigami.Units.gridUnit * 5)
                        visible: count > 0
                        clip: true
                        model: root.backend.discoveredDevices
                        currentIndex: -1
                        keyNavigationEnabled: true
                        keyNavigationWraps: false
                        activeFocusOnTab: true
                        Accessible.name: "Discovered Nanoleaf devices"
                        Accessible.description: "Use arrow keys to browse devices and Enter to select one."
                        onActiveFocusChanged: if (activeFocus && currentIndex < 0 && count > 0) currentIndex = 0
                        Keys.onReturnPressed: if (currentIndex >= 0) manualIp.text = root.backend.discoveredDevices[currentIndex].host
                        Keys.onEnterPressed: if (currentIndex >= 0) manualIp.text = root.backend.discoveredDevices[currentIndex].host
                        ScrollBar.vertical: ScrollBar {}
                        delegate: ItemDelegate {
                            required property int index
                            required property var modelData
                            width: deviceList.width
                            text: modelData.name + " — " + modelData.host
                            highlighted: deviceList.currentIndex === index
                            onClicked: { deviceList.currentIndex = index; manualIp.text = modelData.host }
                            Accessible.name: modelData.name + " at " + modelData.host
                        }
                    }
                    TextField {
                        id: manualIp
                        Layout.fillWidth: true
                        placeholderText: "Controller IPv4 address"
                        validator: RegularExpressionValidator {
                            regularExpression: /^(?:25[0-5]|2[0-4][0-9]|1?[0-9]?[0-9])(?:\.(?:25[0-5]|2[0-4][0-9]|1?[0-9]?[0-9])){3}$/
                        }
                        Accessible.name: "Controller IPv4 address"
                    }
                    Label {
                        Layout.fillWidth: true
                        visible: manualIp.text.trim().length > 0 && !manualIp.acceptableInput
                        text: "Enter four IPv4 octets from 0 to 255, separated by dots."
                        wrapMode: Text.Wrap
                        color: Kirigami.Theme.negativeTextColor
                        Accessible.role: Accessible.AlertMessage
                    }
                    Button {
                        text: root.backend.deviceBusy ? "Pairing…" : "Pair device"
                        enabled: !root.backend.busy && !root.backend.deviceBusy && !root.backend.running
                            && root.backend.status === "stopped"
                            && (manualIp.text.trim().length > 0 ? manualIp.acceptableInput : deviceList.currentIndex >= 0)
                        onClicked: root.backend.pairDevice(manualIp.text.trim().length > 0
                            ? manualIp.text.trim() : root.backend.discoveredDevices[deviceList.currentIndex].host)
                    }
                    Label {
                        Layout.fillWidth: true
                        text: "The API token is stored on this computer with owner-only file permissions."
                        wrapMode: Text.Wrap
                        color: Kirigami.Theme.disabledTextColor
                    }
                }

                Label {
                    Layout.fillWidth: true
                    visible: root.mode !== "1D" && (root.backend.monitors.length === 0 || root.backend.monitorError.length > 0)
                    text: root.backend.monitorError.length > 0 ? "Could not refresh displays; open Diagnostics and help for recovery steps." : "No capture monitor is available. Connect a display or check the DRM device."
                    wrapMode: Text.Wrap
                    color: root.backend.monitorError.length > 0 ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.disabledTextColor
                }
                Label {
                    Layout.fillWidth: true
                    visible: root.mode !== "1D" && root.backend.selectedMonitor.length === 0 && root.backend.monitors.length > 0
                    text: "Choose a capture monitor to enable output."
                    wrapMode: Text.Wrap
                    color: Kirigami.Theme.disabledTextColor
                }
                Label {
                    Layout.fillWidth: true
                    visible: root.backend.settingsPending && root.backend.settingsError.length === 0
                    text: "Applying settings…"
                    color: Kirigami.Theme.disabledTextColor
                }
                Label {
                    Layout.fillWidth: true
                    visible: root.backend.settingsPending && root.backend.settingsError.length > 0
                    text: "Settings are not confirmed. Retry from More options or Fine tune."
                    wrapMode: Text.Wrap
                    color: Kirigami.Theme.negativeTextColor
                    Accessible.role: Accessible.AlertMessage
                }
                Label {
                    Layout.fillWidth: true
                    visible: root.backend.error.length > 0 && root.backend.status !== "error"
                    text: "The control service needs attention. Open Diagnostics and help for recovery steps."
                    wrapMode: Text.Wrap
                    color: Kirigami.Theme.negativeTextColor
                    Accessible.role: Accessible.AlertMessage
                }
                Label {
                    Layout.fillWidth: true
                    visible: root.backend.deviceMessage.startsWith("Warning:") && root.backend.deviceConnected
                    text: root.backend.deviceMessage
                    wrapMode: Text.Wrap
                    color: Kirigami.Theme.neutralTextColor
                    Accessible.role: Accessible.AlertMessage
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 1
            color: Kirigami.Theme.disabledTextColor
            opacity: 0.35
            Accessible.ignored: true
        }
        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing
            Label {
                visible: root.mode === "1D"
                text: "No screen capture needed"
                color: Kirigami.Theme.disabledTextColor
                font: Kirigami.Theme.smallFont
                Layout.fillWidth: true
            }
            ComboBox {
                id: footerMonitor
                objectName: "capture-monitor"
                visible: root.mode !== "1D"
                enabled: !root.backend.busy && !root.backend.deviceBusy && root.backend.monitors.length > 0
                Layout.fillWidth: true
                model: root.backend.monitors
                textRole: "name"
                currentIndex: {
                    for (let i = 0; i < model.length; ++i) {
                        if (model[i].name === root.backend.selectedMonitor)
                            return i
                    }
                    return -1
                }
                onActivated: root.backend.selectMonitor(model[currentIndex].name)
                Accessible.name: "Capture monitor"
                Accessible.description: "Changes the monitor immediately."
            }
            ToolButton {
                visible: root.mode !== "1D"
                icon.name: "view-refresh"
                display: AbstractButton.IconOnly
                enabled: !root.backend.busy && !root.backend.deviceBusy
                onClicked: root.backend.poll()
                Accessible.name: "Refresh monitor list"
                ToolTip.text: "Refresh monitor list"
                ToolTip.visible: hovered
            }
            Button {
                icon.name: "plug"
                text: root.backend.deviceConnected ? String(root.backend.deviceInfo.name || "Nanoleaf 4D") : "Not paired"
                enabled: root.backend.deviceConnected || root.backend.deviceInfoStale
                onClicked: deviceDialog.open()
                Accessible.name: root.backend.deviceConnected ? "Device details for " + text : "No paired device"
                ToolTip.text: root.backend.deviceHost
                ToolTip.visible: hovered && root.backend.deviceConnected
            }
        }
    }

    Timer {
        id: previewTimer
        interval: 100
        repeat: true
        running: root.expanded && root.backend.running && root.backend.previewReady && root.mode !== "1D"
        onTriggered: root.backend.getPreview()
    }

    ColorDialog {
        id: colorDialog
        title: "Choose a static color"
        onAccepted: root.backend.setSetting("static_color", root.colorHex(selectedColor))
        options: ColorDialog.DontUseNativeDialog
    }

    Dialog {
        id: deviceDialog
        title: "Nanoleaf 4D details"
        modal: true
        standardButtons: Dialog.Close
        contentItem: ColumnLayout {
            width: Kirigami.Units.gridUnit * 20
            Label {
                Layout.fillWidth: true
                visible: root.backend.deviceInfoStale
                text: "Details are stale; refresh to retry."
                wrapMode: Text.Wrap
                color: Kirigami.Theme.neutralTextColor
            }
            Repeater {
                model: [
                    { label: "Name", key: "name" }, { label: "Model", key: "model" },
                    { label: "Serial", key: "serialNo" }, { label: "Firmware", key: "firmwareVersion" },
                    { label: "Address", key: "host" }
                ]
                delegate: TextField {
                    required property var modelData
                    Layout.fillWidth: true
                    visible: modelData.key === "host" ? root.backend.deviceHost.length > 0
                        : String(root.backend.deviceInfo[modelData.key] || "").length > 0
                    text: modelData.label + ": " + (modelData.key === "host" ? root.backend.deviceHost
                        : String(root.backend.deviceInfo[modelData.key] || ""))
                    readOnly: true
                    selectByMouse: true
                    Accessible.name: modelData.label
                    Accessible.description: "Read-only device detail; select and copy the value."
                }
            }
            Button {
                Layout.alignment: Qt.AlignLeft
                text: root.backend.deviceBusy ? "Refreshing…" : "Refresh details"
                enabled: root.backend.deviceConnected && !root.backend.deviceBusy
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
            width: Kirigami.Units.gridUnit * 20
            text: root.mode === "1D" ? "Static color mode does not capture the screen. Other modes read the selected display directly through KMS, bypass the KDE portal and its privacy indicator, and do not save images."
                : "Wayleaf 4D reads the selected display directly through KMS. It bypasses the KDE portal and its privacy indicator, and does not save images."
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
            width: Kirigami.Units.gridUnit * 20
            Label {
                Layout.fillWidth: true
                text: "Status: " + (root.backend.running ? "output is on" : root.backend.status)
                wrapMode: Text.Wrap
            }
            Label {
                Layout.fillWidth: true
                visible: root.backend.error.length > 0
                text: root.backend.error
                wrapMode: Text.Wrap
                color: Kirigami.Theme.negativeTextColor
            }
            Label {
                Layout.fillWidth: true
                text: "Check that wayleaf-4d is installed and inspect the user journal. For pairing, hold the controller's power button for 5 seconds."
                wrapMode: Text.Wrap
            }
            RowLayout {
                Button { text: "Retry connection"; onClicked: root.backend.poll() }
                Button {
                    text: "Clear error"
                    enabled: root.backend.error.length > 0
                    onClicked: root.backend.clearError()
                }
                Button { text: "Open project help"; onClicked: Qt.openUrlExternally("https://github.com/avitretiak/wayleaf-4d") }
            }
        }
    }

    Dialog {
        id: disconnectDialog
        title: !root.backend.deviceConnected ? "No paired device" : root.backend.running ? "Stop sync and disconnect?" : "Disconnect this device?"
        modal: true
        standardButtons: root.backend.deviceConnected ? Dialog.Cancel | Dialog.Ok : Dialog.Close
        contentItem: Label {
            width: Kirigami.Units.gridUnit * 20
            text: !root.backend.deviceConnected ? "There is no paired Nanoleaf 4D to disconnect."
                : (root.backend.running ? "Output will stop and the device will be asked to power off. " : "Output is already stopped. ")
                    + "The saved address and token will be removed from this computer. Controller authorization remains enabled."
            wrapMode: Text.Wrap
            padding: Kirigami.Units.largeSpacing
        }
        onAccepted: if (root.backend.deviceConnected) root.backend.disconnectDevice()
    }
}
