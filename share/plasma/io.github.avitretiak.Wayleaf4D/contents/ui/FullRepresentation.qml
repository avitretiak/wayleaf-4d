import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Item {
    id: root

    required property var backend
    implicitWidth: Kirigami.Units.gridUnit * 24
    implicitHeight: Kirigami.Units.gridUnit * 34
    Accessible.name: "Wayleaf 4D controls"

    ColumnLayout {
        id: content
        anchors.fill: parent
        anchors.margins: Kirigami.Units.smallSpacing * 2
        spacing: Kirigami.Units.smallSpacing

        RowLayout {
            Layout.fillWidth: true
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 1
                Label {
                    text: statusTitle()
                    font.bold: true
                    Accessible.name: text
                }
                Label {
                    text: backend.enabled ? "Enabled at login" : "Disabled at login"
                    color: Kirigami.Theme.disabledTextColor
                    font: Kirigami.Theme.smallFont
                }
            }
            Switch {
                id: syncSwitch
                objectName: "sync-switch"
                checked: root.backend.enabled
                enabled: !root.backend.busy && !root.backend.deviceBusy
                onClicked: root.backend.setEnabled(checked)
                Accessible.name: "Screen synchronization"
                Accessible.description: checked
                    ? "On. Wayleaf 4D will start automatically at login."
                    : "Off. Wayleaf 4D will remain off at the next login."
                ToolTip.text: Accessible.description
                ToolTip.visible: hovered
            }
        }

        Label {
            Layout.fillWidth: true
            visible: backend.error.length > 0
            text: backend.error
            wrapMode: Text.Wrap
            color: Kirigami.Theme.negativeTextColor
            Accessible.role: Accessible.AlertMessage
            Accessible.name: "Wayleaf 4D error: " + text
            objectName: "backend-error"
        }

        TabBar {
            id: tabs
            Layout.fillWidth: true
            objectName: "settings-tabs"
            TabButton { text: "Main"; objectName: "tab-main"; Accessible.name: "Main settings" }
            TabButton { text: "Device"; objectName: "tab-device"; Accessible.name: "Device connection" }
            TabButton { text: "Tuning"; objectName: "tab-tuning"; Accessible.name: "Tuning settings" }
            TabButton { text: "Alignment"; objectName: "tab-alignment"; Accessible.name: "4D alignment settings" }
        }

        StackLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            currentIndex: tabs.currentIndex
            enabled: !backend.busy && !backend.deviceBusy && backend.status !== "starting" && backend.status !== "stopping"

            Flickable {
                clip: true
                contentWidth: width
                contentHeight: mainColumn.implicitHeight
                boundsBehavior: Flickable.StopAtBounds
                ScrollBar.vertical: ScrollBar { }
                ColumnLayout {
                    id: mainColumn
                    width: parent.width
                    spacing: Kirigami.Units.smallSpacing
                    Label {
                        Layout.fillWidth: true
                        text: "Capture source"
                        font.bold: true
                    }
                    Label {
                        Layout.fillWidth: true
                        text: "Choose a different display. Sync pauses briefly while the desktop portal opens."
                        wrapMode: Text.Wrap
                    }
                    Button {
                        text: "Change Capture…"
                        objectName: "change-capture"
                        enabled: root.backend.enabled && (root.backend.status === "running" || root.backend.status === "error")
                        onClicked: root.backend.changeCapture()
                        Accessible.name: "Change screen capture"
                        Accessible.description: "Reopen the desktop portal and select a display again."
                    }
                    SettingSlider {
                        backend: root.backend
                        settingKey: "brightness"
                        label: "Brightness"
                        description: "Scales final LED brightness; 1.0 is neutral."
                        minimum: 0; maximum: 2; decimals: 2
                        objectName: "control-brightness"
                    }
                    SettingSlider {
                        backend: root.backend
                        settingKey: "saturation_boost"
                        label: "Saturation boost"
                        description: "Higher values emphasize saturated pixels and boost color intensity."
                        minimum: 0; maximum: 3; decimals: 2
                        objectName: "control-saturation_boost"
                    }
                    SettingSlider {
                        backend: root.backend
                        settingKey: "smooth"
                        label: "Response (higher = faster)"
                        description: "Lower values smooth flicker but respond more slowly; higher values follow changes faster."
                        minimum: 0.05; maximum: 1; decimals: 2
                        objectName: "control-smooth"
                    }
                }
            }
            Flickable {
                clip: true
                contentWidth: width
                contentHeight: deviceColumn.implicitHeight
                boundsBehavior: Flickable.StopAtBounds
                ScrollBar.vertical: ScrollBar { }
                ColumnLayout {
                    id: deviceColumn
                    width: parent.width
                    spacing: Kirigami.Units.smallSpacing
                    Label {
                        text: root.backend.deviceConnected ? "Paired device" : "Connect a Nanoleaf 4D"
                        font.bold: true
                    }
                    Label {
                        Layout.fillWidth: true
                        visible: root.backend.deviceMessage.length > 0
                        text: root.backend.deviceMessage
                        wrapMode: Text.Wrap

                    }
                    ColumnLayout {
                        Layout.fillWidth: true
                        visible: root.backend.deviceConnected
                        Label { text: "Name: " + (root.backend.deviceInfo.name || "Not loaded") }
                        Label { text: "Model: " + (root.backend.deviceInfo.model || "Not loaded") }
                        Label { text: "Serial: " + (root.backend.deviceInfo.serialNo || "Not loaded") }
                        Label { text: "Firmware: " + (root.backend.deviceInfo.firmwareVersion || "Not loaded") }
                        Label { text: "Address: " + root.backend.deviceHost }
                        RowLayout {
                            Button {
                                text: root.backend.deviceBusy ? "Working…" : "Refresh details"
                                enabled: !root.backend.deviceBusy
                                onClicked: root.backend.refreshDeviceInfo()
                            }
                            Button {
                                text: "Disconnect"
                                enabled: !root.backend.busy && !root.backend.deviceBusy && !root.backend.enabled && root.backend.status === "stopped"
                                onClicked: forgetDialog.open()
                            }
                        }
                        Label {
                            Layout.fillWidth: true
                            text: "Disconnecting removes the saved address and token from this computer only; it does not revoke API access on the controller. Turn sync off first."
                            wrapMode: Text.Wrap
                            color: Kirigami.Theme.disabledTextColor
                        }
                    }
                    ColumnLayout {
                        Layout.fillWidth: true
                        visible: !root.backend.deviceConnected
                        Label {
                            Layout.fillWidth: true
                            text: "Search with the system's Avahi mDNS service, or enter the controller's IPv4 address. Hold the 4D power button for 5 seconds before pairing; the pairing window lasts up to 30 seconds."
                            wrapMode: Text.Wrap
                        }
                        Button {
                            text: "Find devices on this network"
                            enabled: !root.backend.busy && !root.backend.deviceBusy
                            onClicked: { deviceList.currentIndex = -1; root.backend.discoverDevices() }
                        }
                        ListView {
                            id: deviceList
                            Layout.fillWidth: true
                            Layout.preferredHeight: Math.min(contentHeight, Kirigami.Units.gridUnit * 8)
                            visible: count > 0
                            clip: true
                            interactive: false
                            model: root.backend.discoveredDevices
                            currentIndex: -1
                            delegate: ItemDelegate {
                                width: deviceList.width
                                text: modelData.name + " — " + modelData.host
                                highlighted: deviceList.currentIndex === index
                                onClicked: { deviceList.currentIndex = index; manualIp.text = modelData.host }
                            }
                        }
                        Label { text: "Controller IPv4 address" }
                        TextField {
                            id: manualIp
                            Layout.fillWidth: true
                            placeholderText: "192.168.1.50"
                            validator: RegularExpressionValidator { regularExpression: /^(?:25[0-5]|2[0-4][0-9]|1?[0-9]?[0-9])(?:\.(?:25[0-5]|2[0-4][0-9]|1?[0-9]?[0-9])){3}$/ }
                            Accessible.name: "Controller IPv4 address"
                        }
                        Button {
                            text: "Pair device"
                            enabled: !root.backend.busy && !root.backend.deviceBusy && !root.backend.enabled && root.backend.status === "stopped" && (manualIp.text.trim().length > 0 ? manualIp.acceptableInput : deviceList.currentIndex >= 0)
                            onClicked: root.backend.pairDevice(manualIp.text.trim().length > 0 ? manualIp.text.trim() : root.backend.discoveredDevices[deviceList.currentIndex].host)
                        }
                        Label {
                            Layout.fillWidth: true
                            text: "Turn sync off before pairing. Pairing stores the API token on this computer with owner-only file permissions."
                            wrapMode: Text.Wrap
                            color: Kirigami.Theme.disabledTextColor
                        }
                    }
                }
            }

            Flickable {
                clip: true
                contentWidth: width
                contentHeight: tuningColumn.implicitHeight
                boundsBehavior: Flickable.StopAtBounds
                ScrollBar.vertical: ScrollBar { }
                ColumnLayout {
                    id: tuningColumn
                    width: parent.width
                    spacing: Kirigami.Units.smallSpacing
                    Label {
                        text: "Capture and color response"
                        font.bold: true
                    }
                    SettingSlider {
                        backend: root.backend
                        settingKey: "fps"
                        label: "Capture/send rate (10 Hz ceiling)"
                        description: "Samples and sends at the selected rate, within the controller's documented 10 Hz limit."
                        minimum: 1; maximum: 10
                        objectName: "control-fps"
                    }
                    SettingSlider {
                        backend: root.backend
                        settingKey: "max_step"
                        label: "Maximum step"
                        description: "Limits each frame's color change; lower values are smoother but add lag."
                        minimum: 1; maximum: 255
                        objectName: "control-max_step"
                    }
                    SettingSlider {
                        backend: root.backend
                        settingKey: "edge_depth"
                        label: "Edge sample depth (%)"
                        description: "How far inward from each screen edge to sample; higher values pick up nearby content sooner."
                        minimum: 5; maximum: 20
                        objectName: "control-edge_depth"
                    }
                    SettingSlider {
                        backend: root.backend
                        settingKey: "center_bleed"
                        label: "Center color blend (%)"
                        description: "Mix this share of the central picture's average color into every edge segment."
                        minimum: 0; maximum: 50
                        objectName: "control-center_bleed"
                    }
                    SettingSlider {
                        backend: root.backend
                        settingKey: "peak_mix"
                        label: "Brightest-pixel blend (%)"
                        description: "Mix this share of the brightest-luma pixel in each zone; lower values suppress tiny highlights."
                        minimum: 0; maximum: 35
                        objectName: "control-peak_mix"
                    }
                    SettingSlider {
                        backend: root.backend
                        settingKey: "feather_depth"
                        label: "Inward feather depth (%)"
                        description: "Extend each edge zone inward with a smooth taper to zero; 0 disables the feather."
                        minimum: 0; maximum: 20
                        objectName: "control-feather_depth"
                    }
                    SettingSlider {
                        backend: root.backend
                        settingKey: "black_threshold"
                        label: "Black threshold"
                        description: "Suppresses near-black capture noise; increase it if dark areas glow."
                        minimum: 0; maximum: 0.1; decimals: 3
                        objectName: "control-black_threshold"
                    }
                    Label {
                        text: "Image handling"
                        font.bold: true
                    }
                    CheckBox {
                        objectName: "setting-letterbox"
                        text: "Letterbox detection"
                        checked: Boolean(root.backend.settings.letterbox)
                        onClicked: root.backend.setSetting("letterbox", checked)
                        Accessible.name: text
                        Accessible.description: "Detects black bars and samples within the active picture."
                        ToolTip.text: Accessible.description
                        ToolTip.visible: hovered
                    }
                    CheckBox {
                        objectName: "setting-midtone_lift"
                        text: "Midtone lift"
                        checked: Boolean(root.backend.settings.midtone_lift)
                        onClicked: root.backend.setSetting("midtone_lift", checked)
                        Accessible.name: text
                        Accessible.description: "Optional legacy lift for dim and midtone content."
                        ToolTip.text: Accessible.description
                        ToolTip.visible: hovered
                    }
                    CheckBox {
                        objectName: "setting-strict_black"
                        text: "Strict black"
                        checked: Boolean(root.backend.settings.strict_black)
                        onClicked: root.backend.setSetting("strict_black", checked)
                        Accessible.name: text
                        Accessible.description: "Sends exact black immediately instead of smoothing dark transitions."
                        ToolTip.text: Accessible.description
                        ToolTip.visible: hovered
                    }
                }
            }

            Flickable {
                clip: true
                contentWidth: width
                contentHeight: alignmentColumn.implicitHeight
                boundsBehavior: Flickable.StopAtBounds
                ScrollBar.vertical: ScrollBar { }
                ColumnLayout {
                    id: alignmentColumn
                    width: parent.width
                    spacing: Kirigami.Units.smallSpacing
                    Label {
                        Layout.fillWidth: true
                        wrapMode: Text.Wrap
                        text: "Choose the screen corner that matches the strip's starting segment. Use Reverse if the lighting order runs backwards; use Offset to shift the segment mapping."
                        Accessible.name: text
                    }
                    ColumnLayout {
                        Layout.fillWidth: true
                        Label { text: "Start corner"; Accessible.name: text }
                        ComboBox {
                            id: cornerBox
                            objectName: "setting-corner"
                            Layout.fillWidth: true
                            model: ["bottom-center", "bottom-left", "top-left", "top-right", "bottom-right"]
                            currentIndex: Math.max(0, model.indexOf(String(root.backend.settings.corner)))
                            onActivated: root.backend.setSetting("corner", currentText)
                            Accessible.name: "Start corner"
                            Accessible.description: "Choose the screen corner used to align the strip's starting segment."
                            ToolTip.text: Accessible.description
                            ToolTip.visible: hovered
                        }
                    }
                    SettingSlider {
                        backend: root.backend
                        settingKey: "offset"
                        label: "Offset"
                        description: "Rotates the panel mapping by this many segments."
                        minimum: 0; maximum: 39
                    }
                    CheckBox {
                        objectName: "setting-reverse"
                        text: "Reverse direction"
                        checked: Boolean(root.backend.settings.reverse)
                        onClicked: root.backend.setSetting("reverse", checked)
                        Accessible.name: text
                        ToolTip.text: Accessible.description
                        ToolTip.visible: hovered
                        Accessible.description: "Reverses the order used to map screen zones to strip segments."
                    }
                }
            }
        }
    }
    Dialog {
        id: forgetDialog
        title: "Disconnect this device?"
        modal: true
        standardButtons: Dialog.Ok | Dialog.Cancel
        contentItem: Label {
            text: "The saved address and token will be removed from this computer. The controller's API access will remain enabled."
            wrapMode: Text.Wrap
            padding: Kirigami.Units.largeSpacing
        }
        onAccepted: root.backend.forgetDevice()
    }

    function statusTitle() {
        switch (backend.status) {
        case "running": return "Screen synchronization is on"
        case "starting": return "Starting screen synchronization…"
        case "stopping": return "Stopping screen synchronization…"
        case "error": return "Screen synchronization needs attention"
        default: return backend.enabled ? "Screen synchronization is starting" : "Screen synchronization is off"
        }
    }
}
