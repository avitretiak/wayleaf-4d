pragma ComponentBehavior: Bound

import QtQuick

Item {
    id: root

    property var zonesByEdge: ({ top: [], right: [], bottom: [], left: [] })
    property bool running: false
    property real brightness: 1.0
    property color disabledColor: "#808080"
    property real u: 1
    property real radius: 12 * u
    property real padding: 16 * u
    property real band: 6 * u
    property real reach: 16 * u
    property var orderedZonesByEdge: ({
        top: zonesByEdge && zonesByEdge.top ? zonesByEdge.top.slice().sort((a, b) => a.position - b.position) : [],
        right: zonesByEdge && zonesByEdge.right ? zonesByEdge.right.slice().sort((a, b) => a.position - b.position) : [],
        bottom: zonesByEdge && zonesByEdge.bottom ? zonesByEdge.bottom.slice().sort((a, b) => a.position - b.position) : [],
        left: zonesByEdge && zonesByEdge.left ? zonesByEdge.left.slice().sort((a, b) => a.position - b.position) : []
    })

    function zoneColor(edge, position) {
        const ordered = orderedZonesByEdge[edge]
        if (!ordered || ordered.length === 0)
            return disabledColor

        const target = Math.max(0, Math.min(1, position))
        if (target <= ordered[0].position)
            return zoneColorValue(ordered[0])
        for (let i = 1; i < ordered.length; ++i) {
            if (target <= ordered[i].position) {
                const a = ordered[i - 1]
                const b = ordered[i]
                const span = b.position - a.position
                const t = span > 0 ? (target - a.position) / span : 0
                const ca = zoneColorValue(a)
                const cb = zoneColorValue(b)
                return Qt.rgba(ca.r + (cb.r - ca.r) * t,
                               ca.g + (cb.g - ca.g) * t,
                               ca.b + (cb.b - ca.b) * t, 1)
            }
        }
        return zoneColorValue(ordered[ordered.length - 1])
    }

    function zoneColorValue(zone) {
        return Qt.rgba(zone.r / 255, zone.g / 255, zone.b / 255, 1)
    }

    ShaderEffectSource {
        id: zoneTexture
        sourceItem: zoneRows
        sourceRect: Qt.rect(0, 0, zoneRows.width, zoneRows.height)
        textureSize: Qt.size(zoneRows.width, zoneRows.height)
        hideSource: true
        live: root.visible
        recursive: false
        smooth: true
    }

    Item {
        id: zoneRows
        width: 256
        height: 4
        visible: true

        Repeater {
            model: ["top", "right", "bottom", "left"]
            delegate: Item {
                id: edgeRow
                required property int index
                required property string modelData
                x: 0
                y: index
                width: zoneRows.width
                height: 1

                Repeater {
                    model: zoneRows.width
                    delegate: Rectangle {
                        required property int index
                        x: index
                        y: 0
                        width: 1
                        height: 1
                        color: root.zoneColor(edgeRow.modelData, (index + 0.5) / zoneRows.width)
                    }
                }
            }
        }
    }

    ShaderEffect {
        anchors.fill: parent
        visible: root.visible
        blending: true
        property var zoneColors: zoneTexture
        property vector4d card: Qt.vector4d(width, height, root.padding, root.radius)
        property vector4d effect: Qt.vector4d(root.band, root.reach, root.brightness,
                                              root.running ? 1 : 0)
        vertexShader: Qt.resolvedUrl("../../shaders/glow_ring.vert.qsb")
        fragmentShader: Qt.resolvedUrl("../../shaders/glow_ring.frag.qsb")
    }
}
