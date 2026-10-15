pragma ComponentBehavior: Bound

import QtQuick
import org.kde.plasma.plasmoid
import org.kde.plasma.core as PlasmaCore
import org.kde.kirigami as Kirigami
import "Nanoleaf"

PlasmoidItem {
    id: root
    property alias syncBackend: backend

    toolTipMainText: "Wayleaf 4D"
    toolTipSubText: backend.status === "error" || backend.error.length > 0 ? "Screen synchronization needs attention"
        : backend.status === "stopping" ? "Screen synchronization is stopping"
            : backend.status === "starting" ? "Screen synchronization is starting"
                : backend.running ? "Screen synchronization is on"
                    : "Screen synchronization is stopped"
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
                color: root.syncBackend.status === "error" || root.syncBackend.error.length > 0 ? Kirigami.Theme.negativeTextColor
                    : root.syncBackend.status === "starting" || root.syncBackend.status === "stopping"
                        ? Kirigami.Theme.neutralTextColor
                        : root.syncBackend.running ? Kirigami.Theme.positiveTextColor : Kirigami.Theme.disabledTextColor
                border.width: 1
                border.color: Kirigami.Theme.backgroundColor
            }
        }
        TapHandler {
            onTapped: root.expanded = !root.expanded
        }
    }
    fullRepresentation: FullRepresentation {
        backend: root.syncBackend
        expanded: root.expanded
    }

    Backend {
        id: backend
    }
}
