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
    readonly property int valueScale: Math.pow(10, decimals)
    readonly property int low: Math.round(minimum * valueScale)
    readonly property int high: Math.round(maximum * valueScale)

    spacing: 2
    implicitWidth: 320
    Accessible.name: label
    Accessible.description: description
    ToolTip.text: description
    ToolTip.visible: labelHover.hovered
    ToolTip.delay: 700

    RowLayout {
        Layout.fillWidth: true
        Label {
            id: settingLabel
            text: root.label
            Layout.fillWidth: true
            Accessible.name: root.label
        }
        HoverHandler { id: labelHover; target: settingLabel }
        SpinBox {
            id: numberEdit
            objectName: "setting-" + root.settingKey + "-value"
            from: root.low
            to: root.high
            stepSize: 1
            editable: true
            value: Math.round(Number(root.backend.settings[root.settingKey]) * root.valueScale)
            textFromValue: function(value, locale) {
                return (value / root.valueScale).toLocaleString(locale, "f", root.decimals)
            }
            valueFromText: function(text, locale) {
                return Math.round(Number.fromLocaleString(locale, text) * root.valueScale)
            }
            validator: root.decimals === 0 ? intValidator : decimalValidator
            onValueModified: root.backend.setSetting(root.settingKey,
                root.decimals === 0 ? value : value / root.valueScale)
            Accessible.name: root.label + " value"
            Accessible.description: root.description
            ToolTip.text: root.description
            ToolTip.visible: hovered
        }
    }

    Slider {
        id: slider
        objectName: "setting-" + root.settingKey + "-slider"
        Layout.fillWidth: true
        from: root.low
        to: root.high
        stepSize: 1
        value: Math.round(Number(root.backend.settings[root.settingKey]) * root.valueScale)
        onMoved: root.backend.setSetting(root.settingKey,
            root.decimals === 0 ? Math.round(value) : Math.round(value) / root.valueScale)
        Accessible.name: root.label
        Accessible.description: root.description
        ToolTip.text: root.description
        ToolTip.visible: hovered
    }

    IntValidator { id: intValidator; bottom: root.low; top: root.high }
    DoubleValidator {
        id: decimalValidator
        bottom: root.minimum
        top: root.maximum
        decimals: root.decimals
        notation: DoubleValidator.StandardNotation
    }
}
