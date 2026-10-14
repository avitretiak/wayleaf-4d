import QtQuick
import org.kde.plasma.plasmoid
import org.kde.plasma.core as PlasmaCore
import org.kde.kirigami as Kirigami
import "Nanoleaf"

PlasmoidItem {
    id: root
    property alias syncBackend: backend

    toolTipMainText: "Wayleaf 4D"
    toolTipSubText: backend.status === "running"
        ? "Screen synchronization is on"
        : backend.status === "error" ? "Screen synchronization needs attention"
            : backend.status === "stopping" ? "Screen synchronization is stopping"
                : backend.enabled ? "Screen synchronization is starting" : "Screen synchronization is off"
    Plasmoid.status: PlasmaCore.Types.ActiveStatus

    compactRepresentation: Item {
        id: compactButton
        implicitWidth: Kirigami.Units.iconSizes.small
        implicitHeight: Kirigami.Units.iconSizes.small
        Accessible.role: Accessible.Button
        Accessible.name: "Wayleaf 4D"
        Accessible.description: root.toolTipSubText
        activeFocusOnTab: true
        Keys.onReturnPressed: root.expanded = !root.expanded
        Keys.onEnterPressed: root.expanded = !root.expanded
        Keys.onSpacePressed: root.expanded = !root.expanded
        Item {
            width: Math.min(Math.min(compactButton.width, compactButton.height), Kirigami.Units.iconSizes.medium)
            height: width
            anchors.left: parent.left
            anchors.verticalCenter: parent.verticalCenter
            Kirigami.Icon {
                anchors.fill: parent
                source: Qt.resolvedUrl("../ambient-light.svg")
                isMask: false
            }
            Rectangle {
                width: 10
                height: 10
                radius: width / 2
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                color: root.syncBackend.status === "running" ? Kirigami.Theme.positiveTextColor
                    : root.syncBackend.status === "error" ? Kirigami.Theme.negativeTextColor
                        : root.syncBackend.status === "starting" ? Kirigami.Theme.neutralTextColor
                            : Kirigami.Theme.disabledTextColor
                border.width: 1
                border.color: Kirigami.Theme.backgroundColor
            }
        }
        TapHandler {
            onTapped: root.expanded = !root.expanded
        }
    }
    fullRepresentation: FullRepresentation { backend: root.syncBackend }

    Backend {
        id: backend
    }
}
