import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

Item {
    id: root
    required property var backend
    required property string settingKey
    required property string label
    required property string description
    required property real minimum
    required property real maximum
    property int decimals: 0
    property real defaultValue: minimum
    property real valueMultiplier: Math.pow(10, decimals)
    property real displayMultiplier: 1
    property string suffix: ""
    property bool editing: false
    property real pendingValue: 0
    property bool pendingValueValid: false
    signal valueEdited(string key, real value)

    readonly property int valueScale: Math.max(1, Math.round(valueMultiplier))
    readonly property int low: Math.round(minimum * valueScale)
    readonly property int high: Math.round(maximum * valueScale)
    readonly property real sliderValue: Number(backend.settings[settingKey]) * valueScale
    readonly property real shownValue: Number(backend.settings[settingKey]) * displayMultiplier
    readonly property bool modified: Math.round(Number(backend.settings[settingKey]) * valueScale)
        !== Math.round(Number(defaultValue) * valueScale)
    enabled: !backend.busy && !backend.deviceBusy && backend.status !== "starting" && backend.status !== "stopping"
    implicitWidth: 320
    implicitHeight: 34
    Accessible.name: label
    Accessible.description: description

    function formatted(value) {
        return Number(value).toLocaleString(Qt.locale(), "f", decimals) + suffix
    }

    readonly property string tooltipText: root.description + "\nRange " + root.formatted(root.minimum * root.displayMultiplier) + " to "
        + root.formatted(root.maximum * root.displayMultiplier) + " · Default "
        + root.formatted(root.defaultValue * root.displayMultiplier)

    function commitStoredValue(value) {
        debounce.stop()
        pendingValueValid = false
        const bounded = Math.max(minimum, Math.min(maximum, Math.round(value * valueScale) / valueScale))
        if (Number(backend.settings[settingKey]) !== bounded) {
            valueEdited(settingKey, bounded)
            backend.setSetting(settingKey, bounded)
        }
    }

    function commitDisplayedValue(value) {
        commitStoredValue(value / displayMultiplier)
    }

    function queueValue(value) {
        pendingValue = value
        pendingValueValid = true
        debounce.restart()
    }

    function beginEdit() {
        editField.text = Number(shownValue).toLocaleString(Qt.locale(), "f", decimals)
        editing = true
        Qt.callLater(function() { editField.forceActiveFocus(); editField.selectAll() })
    }

    function finishEdit() {
        if (editField.acceptableInput) {
            const value = Number.fromLocaleString(Qt.locale(), editField.text)
            if (Number.isFinite(value))
                commitDisplayedValue(value)
        }
        editing = false
    }

    function resetToDefault() {
        const value = Number(defaultValue)
        if (Number.isFinite(value))
            commitStoredValue(value)
        editing = false
    }

    Timer {
        id: debounce
        interval: 50
        repeat: false
        onTriggered: {
            if (root.pendingValueValid)
                root.commitStoredValue(root.pendingValue)
        }
    }

    RowLayout {
        anchors.fill: parent
        spacing: Kirigami.Units.smallSpacing / 2
        Label {
            id: settingLabel
            text: root.label
            Layout.preferredWidth: Kirigami.Units.gridUnit * 6.5
            Layout.minimumWidth: Kirigami.Units.gridUnit * 5
            elide: Text.ElideRight
            Accessible.name: root.label + (root.modified ? ", changed from default" : "")
            ToolTip.text: root.tooltipText
            ToolTip.visible: labelHover.hovered || activeFocus
            ToolTip.delay: 500
            HoverHandler { id: labelHover }
            TapHandler { onDoubleTapped: root.resetToDefault() }
        }
        Rectangle {
            Layout.preferredWidth: 5
            Layout.preferredHeight: 5
            radius: width / 2
            visible: root.modified
            color: Kirigami.Theme.highlightColor
            Accessible.ignored: true
        }
        Slider {
            id: slider
            objectName: "setting-" + root.settingKey + "-slider"
            readonly property string tooltipText: root.tooltipText
            Layout.fillWidth: true
            from: root.low
            to: root.high
            stepSize: 1
            value: Math.round(root.sliderValue)
            onMoved: root.queueValue(value / root.valueScale)
            onPressedChanged: {
                if (!pressed && root.pendingValueValid)
                    root.commitStoredValue(root.pendingValue)
            }
            Accessible.name: root.label
            Accessible.description: root.description + " Range " + root.formatted(root.minimum * root.displayMultiplier)
                + " to " + root.formatted(root.maximum * root.displayMultiplier) + ". Use arrow keys or the mouse wheel to adjust. Double-click to reset."
            ToolTip.text: slider.tooltipText
            ToolTip.visible: slider.hovered || slider.activeFocus
            ToolTip.delay: 500
            TapHandler { onDoubleTapped: root.resetToDefault() }
            WheelHandler {
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                onWheel: function(event) {
                    if (event.angleDelta.y !== 0) {
                        root.commitStoredValue((slider.value + (event.angleDelta.y > 0 ? 1 : -1)) / root.valueScale)
                        event.accepted = true
                    }
                }
            }
        }
        Label {
            visible: !root.editing
            Layout.preferredWidth: 48
            horizontalAlignment: Text.AlignRight
            text: root.formatted(root.shownValue)
            Accessible.name: root.label + " value " + text
            Accessible.role: Accessible.Button
            activeFocusOnTab: true
            Keys.onReturnPressed: root.beginEdit()
            Keys.onEnterPressed: root.beginEdit()
            Keys.onSpacePressed: root.beginEdit()
            TapHandler { onTapped: root.beginEdit() }
        }
        TextField {
            id: editField
            visible: root.editing
            Layout.preferredWidth: 58
            text: ""
            horizontalAlignment: Text.AlignRight
            validator: DoubleValidator {
                bottom: root.minimum * root.displayMultiplier
                top: root.maximum * root.displayMultiplier
                decimals: root.decimals
                notation: DoubleValidator.StandardNotation
                locale: Qt.locale().name
            }
            onAccepted: root.finishEdit()
            onEditingFinished: if (root.editing) root.finishEdit()
            Accessible.name: root.label + " exact value"
            Accessible.description: "Enter a value from " + root.formatted(root.minimum * root.displayMultiplier)
                + " to " + root.formatted(root.maximum * root.displayMultiplier) + ". Press Enter to apply."
        }
    }
}
