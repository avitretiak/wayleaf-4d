import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
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
    signal valueEdited(string key, real value)

    readonly property int valueScale: Math.max(1, Math.round(valueMultiplier))
    readonly property int low: Math.round(minimum * valueScale)
    readonly property int high: Math.round(maximum * valueScale)
    readonly property real sliderValue: Number(backend.settings[settingKey]) * valueScale
    readonly property real shownValue: Number(backend.settings[settingKey]) * displayMultiplier
    enabled: !backend.busy && !backend.deviceBusy && backend.status !== "starting" && backend.status !== "stopping"
    // Values remain in backend units; valueScale drives slider steps and displayMultiplier changes readout units.

    spacing: 2
    implicitWidth: 320
    Accessible.name: label
    Accessible.description: description
    ToolTip.text: description
    ToolTip.visible: labelHover.hovered
    ToolTip.delay: 700

    function formatted(value) {
        return Number(value).toLocaleString(Qt.locale(), "f", decimals) + suffix
    }

    function commitStoredValue(value) {
        const bounded = Math.max(minimum, Math.min(maximum, Math.round(value * valueScale) / valueScale))
        if (Number(backend.settings[settingKey]) !== bounded) {
            valueEdited(settingKey, bounded)
            backend.setSetting(settingKey, bounded)
        }
    }

    function commitDisplayedValue(value) {
        commitStoredValue(value / displayMultiplier)
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

    RowLayout {
        Layout.fillWidth: true
        Label {
            id: settingLabel
            text: root.label
            Layout.fillWidth: true
            Accessible.name: root.label
        }
        HoverHandler { id: labelHover; target: settingLabel }
        ToolButton {
            id: valueButton
            visible: !root.editing
            text: root.formatted(root.shownValue)
            flat: true
            onClicked: root.beginEdit()
            Accessible.name: "Edit " + root.label + " value"
            Accessible.description: "Current value " + text + ". Click to enter an exact value; use the reset button to restore " + root.formatted(root.defaultValue * root.displayMultiplier) + "."
            ToolTip.text: root.description
            ToolTip.visible: hovered
        }
        ToolButton {
            id: resetButton
            objectName: "reset-" + root.settingKey
            icon.name: "view-refresh"
            display: AbstractButton.IconOnly
            Layout.preferredWidth: 28
            Layout.preferredHeight: 28
            padding: 2
            enabled: Math.round(Number(root.backend.settings[root.settingKey]) * root.valueScale) !== Math.round(Number(root.defaultValue) * root.valueScale)
            onClicked: root.resetToDefault()
            Accessible.name: "Reset " + root.label + " to default"
            Accessible.description: "Restore " + root.formatted(root.defaultValue * root.displayMultiplier) + "."
            ToolTip.text: "Reset to default (" + root.formatted(root.defaultValue * root.displayMultiplier) + ")"
            ToolTip.visible: hovered
        }
        TextField {
            id: editField
            visible: root.editing
            Layout.preferredWidth: 88
            text: ""
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

    Slider {
        id: slider
        objectName: "setting-" + root.settingKey + "-slider"
        Layout.fillWidth: true
        from: root.low
        to: root.high
        stepSize: 1
        value: Math.round(root.sliderValue)
        onMoved: root.commitStoredValue(value / root.valueScale)
        Accessible.name: root.label
        Accessible.description: root.description + " Range " + root.formatted(root.minimum * root.displayMultiplier)
            + " to " + root.formatted(root.maximum * root.displayMultiplier) + ". Use keyboard arrows or Shift+scroll to adjust; use the reset button to restore the default."
        WheelHandler {
            acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
            onWheel: function(event) {
                if (event.angleDelta.y !== 0 && (event.modifiers & Qt.ShiftModifier)) {
                    root.commitStoredValue((slider.value + (event.angleDelta.y > 0 ? 1 : -1)) / root.valueScale)
                    event.accepted = true
                }
            }
        }
    }
    Label {
        text: "Range " + root.formatted(root.minimum * root.displayMultiplier) + "–"
            + root.formatted(root.maximum * root.displayMultiplier) + " · Default "
            + root.formatted(root.defaultValue * root.displayMultiplier)
        opacity: 0.75
        wrapMode: Text.Wrap
        Accessible.name: root.label + " " + text
    }

    Label {
        visible: root.backend.isSettingPending(root.settingKey)
        text: root.backend.settingsError.length > 0
            ? "Pending — service has not confirmed this value. Retry pending settings above."
            : "Applying setting…"
        wrapMode: Text.Wrap
        Accessible.name: root.label + ": " + text
    }
}
