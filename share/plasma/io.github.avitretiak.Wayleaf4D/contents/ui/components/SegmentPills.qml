pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import org.kde.kirigami as Kirigami

Item {
    id: root
    required property string edge
    required property var zones
    required property bool horizontal
    required property string mode
    required property bool running
    required property real brightness

    readonly property real u: Kirigami.Units.gridUnit / 16
    readonly property real thickness: 6 * u
    readonly property real gap: 4 * u
    readonly property int zoneCount: zones ? zones.length : 0
    Accessible.ignored: true

    Repeater {
        model: root.zones || []
        delegate: Rectangle {
            required property var modelData
            required property int index
            readonly property bool realId: modelData.id !== undefined && modelData.id !== null
            readonly property real brightnessScale: Math.max(0, Math.min(1.35, Number(root.brightness)))
            readonly property color zoneColor: Qt.rgba(
                Math.min(1, Number(modelData.r) / 255 * brightnessScale),
                Math.min(1, Number(modelData.g) / 255 * brightnessScale),
                Math.min(1, Number(modelData.b) / 255 * brightnessScale), 1)
            x: root.horizontal ? index * ((root.width - root.gap * Math.max(0, root.zoneCount - 1)) / Math.max(1, root.zoneCount) + root.gap) : (root.width - root.thickness) / 2
            y: root.horizontal ? (root.height - root.thickness) / 2 : index * ((root.height - root.gap * Math.max(0, root.zoneCount - 1)) / Math.max(1, root.zoneCount) + root.gap)
            width: root.horizontal ? Math.max(0, (root.width - root.gap * Math.max(0, root.zoneCount - 1)) / Math.max(1, root.zoneCount)) : root.thickness
            height: root.horizontal ? root.thickness : Math.max(0, (root.height - root.gap * Math.max(0, root.zoneCount - 1)) / Math.max(1, root.zoneCount))
            radius: root.thickness / 2
            color: zoneColor
            opacity: root.running ? 1 : 0.3
            Accessible.ignored: true
            ToolTip.visible: root.mode === "4D" && realId && hover.hovered
            ToolTip.text: "Controller zone " + modelData.id + " · " + root.edge + " · "
                + Math.round(Math.max(0, Math.min(1, Number(modelData.position))) * 100) + "%"
            HoverHandler { id: hover }
        }
    }
}
