pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import org.kde.kirigami as Kirigami

Rectangle {
    id: root
    required property var backend
    required property string statusText
    required property string mode
    required property string modeDescription
    required property var zonesByEdge
    signal moreRequested()

    readonly property real u: Kirigami.Units.gridUnit / 16
    readonly property real brightness: Math.max(0, Math.min(2, Number(backend.settings.brightness)))
    readonly property real previewBrightness: Math.max(0, Math.min(1.35, brightness / 1.5))
    readonly property var modes: ["1D", "2D", "3D", "4D"]
    // Kirigami's PlatformTheme has no separatorColor role.
    readonly property color separatorColor: Qt.rgba(
        Kirigami.Theme.textColor.r, Kirigami.Theme.textColor.g, Kirigami.Theme.textColor.b, 0.2)
    function descriptionForMode(value) {
        if (value === "1D") return "Static color ambience"
        if (value === "2D") return "Dominant color from the screen"
        if (value === "3D") return "Several colors from the screen"
        return "Full immersive edge mirroring"
    }

    implicitWidth: 280 * u
    implicitHeight: 160 * u
    radius: 12 * u
    antialiasing: true
    color: Qt.rgba(
        Kirigami.Theme.backgroundColor.r * 0.94 + Kirigami.Theme.textColor.r * 0.06,
        Kirigami.Theme.backgroundColor.g * 0.94 + Kirigami.Theme.textColor.g * 0.06,
        Kirigami.Theme.backgroundColor.b * 0.94 + Kirigami.Theme.textColor.b * 0.06,
        1)
    border.width: 0.5 * u
    border.color: root.separatorColor
    Accessible.name: "Wayleaf 4D controls"

    GlowRing {
        u: root.u
        anchors.fill: parent
        anchors.margins: -16 * root.u
        zonesByEdge: root.zonesByEdge
        running: root.backend.running
        brightness: root.previewBrightness
        disabledColor: Kirigami.Theme.disabledTextColor
        radius: 12 * root.u
        z: 0
        Accessible.ignored: true
    }

    Repeater {
        model: ["top", "right", "bottom", "left"]
        delegate: SegmentPills {
            required property int index
            required property string modelData
            edge: modelData
            zones: root.zonesByEdge[modelData] || []
            horizontal: index === 0 || index === 2
            mode: root.mode
            running: root.backend.running
            brightness: root.previewBrightness
            x: index === 0 || index === 2 ? 22 * root.u : index === 1 ? root.width - 10 * root.u : 4 * root.u
            y: index === 0 ? 4 * root.u : index === 2 ? root.height - 10 * root.u : 22 * root.u
            width: horizontal ? root.width - 44 * root.u : 6 * root.u
            height: horizontal ? 6 * root.u : root.height - 44 * root.u
            z: 2
        }
    }

    Rectangle {
        id: statusDot
        x: 22 * root.u
        y: 24 * root.u
        width: 9 * root.u
        height: width
        radius: width / 2
        color: root.backend.error.length > 0 || root.backend.status === "error" ? Kirigami.Theme.negativeTextColor
            : root.backend.status === "starting" || root.backend.status === "stopping" ? Kirigami.Theme.neutralTextColor
                : root.backend.running ? Kirigami.Theme.positiveTextColor : Kirigami.Theme.disabledTextColor
        Accessible.ignored: true
    }
    Label {
        x: 36 * root.u
        y: 17 * root.u
        width: Math.max(0, syncSwitch.x - x - 6 * root.u)
        height: 20 * root.u
        text: "Wayleaf 4D"
        font.pixelSize: 13 * root.u
        font.bold: true
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideRight
    }
    Label {
        x: 22 * root.u
        y: 42 * root.u
        width: root.width - 44 * root.u
        height: 18 * root.u
        text: root.statusText
        color: root.backend.error.length > 0 || root.backend.status === "error" ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.disabledTextColor
        font.pixelSize: 11 * root.u
        elide: Text.ElideMiddle
        Accessible.name: text
        Accessible.role: root.backend.error.length > 0 ? Accessible.AlertMessage : Accessible.StaticText
    }
    Switch {
        id: syncSwitch
        objectName: "sync-switch"
        x: root.width - moreButton.width - width - 27 * root.u
        y: 18 * root.u
        text: ""
        height: 26 * root.u
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
        id: moreButton
        x: root.width - width - 18 * root.u
        y: 18 * root.u
        icon.name: "overflow-menu"
        width: 26 * root.u
        height: 26 * root.u
        display: AbstractButton.IconOnly
        Accessible.name: "More options"
        ToolTip.text: "More options"
        ToolTip.visible: hovered
        onClicked: root.moreRequested()
    }

    Kirigami.Icon {
        x: 22 * root.u
        y: 70 * root.u
        width: 16 * root.u
        height: width
        source: "brightness-high"
        Accessible.ignored: true
    }
    Slider {
        id: brightnessSlider
        x: 38 * root.u
        y: 66 * root.u
        width: root.width - 112 * root.u
        height: 24 * root.u
        leftPadding: 0
        rightPadding: 0
        from: 0
        to: 100
        enabled: !root.backend.busy && !root.backend.deviceBusy
            && root.backend.status !== "starting" && root.backend.status !== "stopping"
        value: root.brightness * 50
        onMoved: root.backend.setSetting("brightness", Math.round(value) / 50)
        Accessible.name: "Brightness"
        Accessible.description: "Scales the output brightness."
        objectName: "control-brightness"
    }
    Label {
        x: root.width - 62 * root.u
        y: 67 * root.u
        width: 40 * root.u
        height: 22 * root.u
        text: Math.round(brightnessSlider.value) + "%"
        horizontalAlignment: Text.AlignRight
        verticalAlignment: Text.AlignVCenter
        font.pixelSize: 11 * root.u
    }

    SteppedSlider {
        id: modeSlider
        objectName: "mode-slider"
        x: 22 * root.u
        y: 94 * root.u
        width: root.width - 44 * root.u
        height: 26 * root.u
        labels: root.modes
        value: root.modes.indexOf(root.mode)
        enabled: !root.backend.busy && !root.backend.deviceBusy
        onMoved: root.backend.setSetting("mode", root.modes[Math.round(value)])
        Accessible.name: "Output mode"
        Accessible.description: root.mode + " · " + root.descriptionForMode(root.mode)
            + ". Drag or use Left and Right to choose 1D, 2D, 3D, or 4D."
    }
    Label {
        x: 22 * root.u
        y: 122 * root.u
        width: root.width - 44 * root.u
        height: 16 * root.u
        text: root.modeDescription
        color: Kirigami.Theme.disabledTextColor
        font.pixelSize: 11 * root.u
        elide: Text.ElideRight
    }
}
