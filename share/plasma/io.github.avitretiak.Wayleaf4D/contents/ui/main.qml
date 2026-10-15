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
    toolTipSubText: backend.error.length > 0 || backend.status === "error" ? "Error · " + (backend.error || "Synchronization needs attention")
        : backend.status === "stopping" ? "Stopping synchronization"
            : backend.status === "starting" ? "Starting synchronization"
                : backend.running && backend.settings.mode === "1D" ? "1D · On · " + backend.settings.static_color
                    : backend.running ? backend.settings.mode + " · Syncing · " + Number(backend.settings.fps) + " Hz · " + backend.selectedMonitor
                        : (backend.settings.mode || "4D") + " · Stopped"
    Plasmoid.status: PlasmaCore.Types.ActiveStatus

    compactRepresentation: Item {
        id: compactButton
        implicitWidth: Kirigami.Units.iconSizes.small
        implicitHeight: Kirigami.Units.iconSizes.small
        Accessible.role: Accessible.Button
        Accessible.name: "Wayleaf 4D, " + (root.syncBackend.settings.mode || "4D") + " mode"
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
                visible: root.syncBackend.running && root.syncBackend.settings.mode === "1D"
                width: 7
                height: 7
                radius: width / 2
                anchors.right: parent.right
                anchors.top: parent.top
                color: root.syncBackend.settings.static_color || "#ffffff"
                border.width: 1
                border.color: Kirigami.Theme.backgroundColor
                Accessible.ignored: true
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
