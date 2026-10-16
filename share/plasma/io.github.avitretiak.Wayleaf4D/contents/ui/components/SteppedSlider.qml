pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import org.kde.kirigami as Kirigami

Slider {
    id: control
    required property var labels
    readonly property real u: Kirigami.Units.gridUnit / 16
    readonly property color separatorColor: Qt.rgba(
        Kirigami.Theme.textColor.r, Kirigami.Theme.textColor.g, Kirigami.Theme.textColor.b, 0.2)
    readonly property color raisedColor: Qt.rgba(
        Kirigami.Theme.backgroundColor.r * 0.94 + Kirigami.Theme.textColor.r * 0.06,
        Kirigami.Theme.backgroundColor.g * 0.94 + Kirigami.Theme.textColor.g * 0.06,
        Kirigami.Theme.backgroundColor.b * 0.94 + Kirigami.Theme.textColor.b * 0.06, 1)

    implicitWidth: 236 * u
    implicitHeight: 26 * u
    padding: 0
    from: 0
    to: labels.length - 1
    stepSize: 1
    snapMode: Slider.SnapAlways
    background: Rectangle {
        radius: 6 * control.u
        color: Kirigami.Theme.backgroundColor
        border.width: 0.5 * control.u
        border.color: control.separatorColor
    }
    handle: Item {
        x: control.leftPadding + control.visualPosition * (control.availableWidth - width)
        y: control.topPadding
        width: control.availableWidth / control.labels.length
        height: control.availableHeight
        z: 1
        Rectangle {
            anchors.fill: parent
            anchors.margins: 2 * control.u
            radius: 4 * control.u
            antialiasing: true
            color: control.raisedColor
            border.width: 0.5 * control.u
            border.color: control.activeFocus ? Kirigami.Theme.highlightColor : control.separatorColor
        }
    }
    contentItem: Item {
        z: 2
        Repeater {
            model: control.labels
            delegate: Label {
                required property int index
                required property string modelData
                x: (control.mirrored ? control.labels.length - 1 - index : index)
                    * control.availableWidth / control.labels.length
                width: control.availableWidth / control.labels.length
                height: control.availableHeight
                text: modelData
                font.pixelSize: 12 * control.u
                font.bold: index === Math.round(control.value)
                color: control.enabled && index === Math.round(control.value)
                    ? Kirigami.Theme.textColor : Kirigami.Theme.disabledTextColor
                horizontalAlignment: Text.AlignHCenter
                verticalAlignment: Text.AlignVCenter
                Accessible.ignored: true
            }
        }
    }
}
