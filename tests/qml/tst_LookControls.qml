pragma ComponentBehavior: Bound

import QtQuick
import QtTest
import "../../share/plasma/io.github.avitretiak.Wayleaf4D/contents/ui" as UI

TestCase {
    id: testCase
    name: "LookControls"
    width: 400
    height: 700
    visible: true
    when: windowShown

    property var popup: null
    readonly property var customValues: ({
        saturation_boost: 2.27, smooth: 0.64, max_step: 31, edge_depth: 13,
        feather_depth: 9, center_bleed: 17, peak_mix: 29, black_threshold: 0.023, fps: 7
    })
    readonly property var defaults: ({
        mode: "4D", static_color: "#ffffff", brightness: 1.5,
        saturation_boost: 1.25, smooth: 0.35, max_step: 12, edge_depth: 8,
        feather_depth: 5, center_bleed: 0, peak_mix: 15, black_threshold: 0.01,
        fps: 10, letterbox: true, midtone_lift: false, strict_black: false,
        start_corner: "bottom_center", reverse_direction: false, perimeter_offset: 0
    })
    function normalized(values) {
        const result = Object.assign({}, values)
        for (const key of Object.keys(result)) {
            if (typeof result[key] === "number")
                result[key] = Math.fround(result[key])
        }
        return result
    }

    QtObject {
        id: backendState
        property var settings: ({})
        property string status: "running"
        property bool running: true
        property string error: ""
        property bool busy: false
        property bool deviceBusy: false
        property bool deviceConnected: true
        property bool deviceInfoStale: false
        property int detectedPanelCount: 32
        property var deviceInfo: ({ name: "Fixture 4D" })
        property string deviceHost: "192.0.2.4"
        property string deviceMessage: ""
        property var discoveredDevices: []
        property var monitors: [{ name: "DP-3" }]
        property string selectedMonitor: "DP-3"
        property string monitorError: ""
        property bool settingsPending: false
        property bool settingsRetryAvailable: false
        property string settingsError: ""
        property bool startAtLogin: false
        property bool previewEnabled: false
        property bool previewReady: false
        property var previewZones: []
        function setPreviewEnabled(value) { previewEnabled = value }
        function setSetting(key, value) {
            const next = Object.assign({}, settings)
            next[key] = typeof value === "number" ? Math.fround(value) : value
            settings = next
        }
    }

    QtObject {
        id: savedState
        property string customSettings: ""
        property string selectedPreset: ""
    }

    Component {
        id: popupComponent
        UI.FullRepresentation {
            width: testCase.width
            height: testCase.height
            backend: backendState
            configuration: savedState
            expanded: true
        }
    }

    function openPopup() {
        popup = createTemporaryObject(popupComponent, testCase)
        verify(popup !== null)
        wait(0)
    }

    function closePopup() {
        if (popup) {
            const deviceDialog = findChild(popup, "device-details-dialog")
            if (deviceDialog && deviceDialog.visible) {
                deviceDialog.close()
                tryCompare(deviceDialog, "visible", false)
                wait(0)
            }
            popup.destroy()
            popup = null
            wait(0)
        }
    }

    function init() {
        savedState.customSettings = ""
        savedState.selectedPreset = ""
        backendState.settings = normalized(Object.assign({}, defaults, customValues))
        openPopup()
    }

    function cleanup() {
        backendState.busy = false
        backendState.deviceBusy = false
        closePopup()
    }

    function verifyCustomValues() {
        for (const key of Object.keys(customValues)) {
            verify(Math.abs(Number(backendState.settings[key]) - customValues[key]) < 0.000001,
                "Custom " + key + " was replaced: " + backendState.settings[key])
        }
    }

    function selectLook(index) {
        const slider = findChild(popup, "look-slider")
        verify(slider.visible)
        mouseClick(slider, slider.width * (index + 0.5) / 4, slider.height / 2)
        tryCompare(popup, "selectedPreset", ["Subtle", "Balanced", "Vivid", "Custom"][index])
    }


    function test_overflow_menu_anchors_to_actual_button() {
        const button = findChild(popup, "more-options-button")
        const menu = findChild(popup, "overflow-menu")
        verify(button !== null)
        verify(menu !== null)
        mouseClick(button)
        tryCompare(menu, "visible", true)
        const buttonTopLeft = button.mapToItem(testCase, 0, 0)
        const buttonBottomRight = button.mapToItem(testCase, button.width, button.height)
        const menuTopLeft = menu.contentItem.mapToItem(testCase, 0, 0)
        const menuBottomRight = menu.contentItem.mapToItem(testCase,
            menu.contentItem.width, menu.contentItem.height)
        verify(menu.visible)
        verify(menuTopLeft.x >= 0 && menuTopLeft.y >= 0
            && menuBottomRight.x <= testCase.width && menuBottomRight.y <= testCase.height,
            "Visible menu must remain within the available viewport")
        const opensBelow = menuTopLeft.y >= buttonBottomRight.y
        const opensAbove = menuBottomRight.y <= buttonTopLeft.y
        verify(opensBelow || opensAbove, "Menu must open adjacent to the activated button")
        verify(menuTopLeft.x < buttonBottomRight.x && menuBottomRight.x > buttonTopLeft.x,
            "Menu popup must stay horizontally anchored to the activated button")
        menu.close()

    }
    function verifyCustomControls(mode) {
        const keys = mode === "2D"
            ? ["saturation_boost", "smooth", "fps"]
            : mode === "3D"
                ? ["saturation_boost", "smooth", "edge_depth", "max_step", "peak_mix", "fps"]
                : ["saturation_boost", "smooth", "midtone_lift", "edge_depth", "feather_depth", "max_step",
                    "center_bleed", "peak_mix", "black_threshold", "letterbox", "strict_black", "fps"]
        compare(popup.fineTuneCount, keys.length)
        for (const key of keys) {
            const isSwitch = key === "midtone_lift" || key === "letterbox" || key === "strict_black"
            const objectName = "setting-" + key + (isSwitch ? "" : "-slider")
            const control = findChild(popup, objectName)
            verify(control !== null && control.visible, mode + " Custom control not visible: " + objectName)
        }
    }

    function test_custom_survives_balanced_and_popup_reopen() {
        verifyCustomControls("4D")
        selectLook(1)
        compare(backendState.settings.peak_mix, 15)
        const snapshot = JSON.parse(savedState.customSettings)
        for (const key of Object.keys(customValues))
            verify(Math.abs(snapshot[key] - customValues[key]) < 0.000001, key)
        selectLook(3)
        verifyCustomControls("4D")
        verifyCustomValues()
        selectLook(1)
        closePopup()
        openPopup()
        compare(popup.selectedPreset, "Balanced")
        selectLook(3)
        verifyCustomControls("4D")
        verifyCustomValues()
    }

    function test_each_preset_hides_fine_tuning_and_preserves_custom() {
        const names = ["Subtle", "Balanced", "Vivid"]
        const expectedSaturation = [1.0, 1.25, 1.9]
        for (let i = 0; i < names.length; ++i) {
            selectLook(i)
            verify(!findChild(popup, "fine-tune-controls").visible, names[i])
            verify(Math.abs(backendState.settings.saturation_boost - expectedSaturation[i]) < 0.000001)
            closePopup()
            openPopup()
            compare(popup.selectedPreset, names[i], "f32 values must still recognize " + names[i])
            selectLook(3)
            verify(findChild(popup, "fine-tune-controls").visible)
            verifyCustomValues()
        }
    }
    function test_legacy_named_preset_keeps_saved_values() {
        closePopup()
        savedState.selectedPreset = "Subtle"
        backendState.settings = normalized(Object.assign({}, defaults, {
            saturation_boost: 0.8, smooth: 0.25, max_step: 8, peak_mix: 5
        }))
        openPopup()
        compare(popup.selectedPreset, "Subtle")
        compare(backendState.settings.saturation_boost, 0.8)
    }
    function test_legacy_balanced_preset_keeps_saved_values() {
        closePopup()
        savedState.selectedPreset = "Balanced"
        backendState.settings = normalized(Object.assign({}, defaults, {
            saturation_boost: 1.0
        }))
        openPopup()
        compare(popup.selectedPreset, "Balanced")
        compare(backendState.settings.saturation_boost, 1.0)
    }


    function test_legacy_preset_values_without_saved_label_are_recognized() {
        closePopup()
        savedState.selectedPreset = ""
        backendState.settings = normalized(Object.assign({}, defaults, {
            saturation_boost: 0.8, smooth: 0.25, max_step: 8, peak_mix: 5
        }))
        openPopup()
        compare(popup.selectedPreset, "Subtle")
        compare(backendState.settings.saturation_boost, 0.8)
    }

    function test_stale_named_preset_label_does_not_override_custom_values() {
        closePopup()
        savedState.selectedPreset = "Subtle"
        backendState.settings = normalized(Object.assign({}, defaults, { saturation_boost: 2.1 }))
        openPopup()
        compare(popup.selectedPreset, "Custom")
        compare(backendState.settings.saturation_boost, 2.1)
    }

    function test_keyboard_and_drag_round_trip() {
        selectLook(0)
        const slider = findChild(popup, "look-slider")
        slider.forceActiveFocus()
        keyClick(Qt.Key_Right)
        compare(popup.selectedPreset, "Balanced")
        keyClick(Qt.Key_Right)
        compare(popup.selectedPreset, "Vivid")
        keyClick(Qt.Key_Right)
        compare(popup.selectedPreset, "Custom")
        verifyCustomValues()
        mouseDrag(slider, slider.width * 0.875, slider.height / 2, -slider.width * 0.75, 0)
        compare(popup.selectedPreset, "Subtle")
        mouseDrag(slider, slider.width * 0.125, slider.height / 2, slider.width * 0.75, 0)
        compare(popup.selectedPreset, "Custom")
        verifyCustomValues()
    }

    function test_custom_equal_to_balanced_keeps_custom_identity() {
        backendState.settings = normalized(defaults)
        compare(popup.selectedPreset, "Balanced")
        selectLook(3)
        closePopup()
        openPopup()
        compare(popup.selectedPreset, "Custom")
        verify(findChild(popup, "fine-tune-controls").visible)
    }

    function test_reset_all_keeps_custom_controls_available() {
        selectLook(1)
        selectLook(3)
        popup.resetAllFineTune()
        compare(popup.selectedPreset, "Balanced")
        verify(!findChild(popup, "fine-tune-controls").visible)
        selectLook(3)
        compare(popup.selectedPreset, "Custom")
        verify(findChild(popup, "fine-tune-controls").visible)
        closePopup()
        openPopup()
        compare(popup.selectedPreset, "Custom")
        compare(backendState.settings.peak_mix, 15)
    }

    function test_reset_uses_balanced_defaults() {
        selectLook(1)
        selectLook(3)
        popup.resetAllFineTune()
        compare(backendState.settings.saturation_boost, 1.25)
        compare(popup.selectedPreset, "Balanced")
        verify(!findChild(popup, "fine-tune-controls").visible)
        closePopup()
        openPopup()
        compare(popup.selectedPreset, "Balanced")
        compare(backendState.settings.saturation_boost, 1.25)
        verify(!findChild(popup, "fine-tune-controls").visible)
    }

    function test_custom_edit_survives_immediate_preset_switch() {
        selectLook(1)
        selectLook(3)
        const saturation = findChild(popup, "setting-saturation_boost-slider")
        saturation.forceActiveFocus()
        keyClick(Qt.Key_Right)
        verify(Math.abs(backendState.settings.saturation_boost - 2.28) < 0.000001)
        popup.choosePreset("Balanced")
        compare(popup.selectedPreset, "Balanced")
        popup.choosePreset("Custom")
        verify(Math.abs(backendState.settings.saturation_boost - 2.28) < 0.000001)
    }

    function test_custom_fine_tuning_follows_capture_mode() {
        selectLook(1)
        selectLook(3)
        verifyCustomControls("4D")
        backendState.setSetting("mode", "1D")
        verify(!findChild(popup, "fine-tune-controls").visible)
        for (const mode of ["2D", "3D", "4D"]) {
            backendState.setSetting("mode", mode)
            verify(findChild(popup, "fine-tune-controls").visible, mode)
            verifyCustomControls(mode)
        }
    }

    function test_alignment_survives_look_changes_and_group_reset() {
        popup.width = 260
        mouseClick(findChild(popup, "device-details-button"))
        wait(0)
        const alignment = findChild(popup, "alignment-controls")
        verify(alignment.visible)
        const dialog = findChild(popup, "device-details-dialog")
        const detailsScroll = findChild(popup, "device-details-scroll")
        verify(dialog.width <= popup.width && dialog.height <= popup.height)
        verify(detailsScroll.width > 0 && detailsScroll.height > 0)
        verify(detailsScroll.padding > 0)
        verify(detailsScroll.width <= dialog.width && detailsScroll.height <= dialog.height)
        verify(detailsScroll.availableWidth > 2 * detailsScroll.padding)
        const alignmentPosition = alignment.mapToItem(detailsScroll, 0, 0)
        verify(alignmentPosition.x >= detailsScroll.padding && alignmentPosition.y >= detailsScroll.padding)
        verify(alignment.width <= detailsScroll.availableWidth - 2 * detailsScroll.padding)
        const bodyScroll = findChild(popup, "body-controls-scroll")
        const saturation = findChild(popup, "setting-saturation_boost-slider")
        const saturationTopLeft = saturation.mapToItem(bodyScroll, 0, 0)
        verify(saturation.width > 0 && saturationTopLeft.x >= 0
            && saturationTopLeft.x + saturation.width <= bodyScroll.width)
        const corner = findChild(popup, "alignment-start-corner")
        const reverse = findChild(popup, "alignment-reverse-direction")
        const offset = findChild(popup, "alignment-perimeter-offset")
        const cornerRow = corner.parent
        const cornerPosition = corner.mapToItem(cornerRow, 0, 0)
        verify(corner.width > 0 && cornerPosition.x >= 0
            && cornerPosition.x + corner.width <= cornerRow.width)
        const offsetRow = offset.parent
        const offsetPosition = offset.mapToItem(offsetRow, 0, 0)
        verify(offset.width > 0 && offsetPosition.x >= 0
            && offsetPosition.x + offset.width <= offsetRow.width)
        corner.forceActiveFocus()
        keyClick(Qt.Key_Down)
        keyClick(Qt.Key_Down)
        compare(backendState.settings.start_corner, "top_left")
        reverse.forceActiveFocus()
        keyClick(Qt.Key_Space)
        backendState.setSetting("perimeter_offset", 39)
        compare(offset.value, 39)
        offset.forceActiveFocus()
        keyClick(Qt.Key_Up)
        compare(backendState.settings.perimeter_offset, 39)
        backendState.setSetting("perimeter_offset", 0)
        offset.forceActiveFocus()
        keyClick(Qt.Key_Up)
        compare(backendState.settings.perimeter_offset, 1)
        for (const look of ["Subtle", "Balanced", "Vivid", "Custom"]) {
            popup.choosePreset(look)
            verify(alignment.visible)
            compare(backendState.settings.start_corner, "top_left")
            compare(backendState.settings.reverse_direction, true)
            compare(backendState.settings.perimeter_offset, 1)
        }
        const saved = JSON.parse(savedState.customSettings)
        for (const key of ["start_corner", "reverse_direction", "perimeter_offset"])
            verify(!Object.prototype.hasOwnProperty.call(saved, key))
        const brightness = backendState.settings.brightness
        mouseClick(findChild(popup, "alignment-reset"))
        compare(backendState.settings.start_corner, "bottom_center")
        compare(backendState.settings.reverse_direction, false)
        compare(backendState.settings.perimeter_offset, 0)
        compare(backendState.settings.brightness, brightness)
        for (const mode of ["1D", "2D", "3D"]) {
            backendState.setSetting("mode", mode)
            verify(!alignment.visible)
        }
        backendState.setSetting("mode", "4D")
        verify(alignment.visible)
        backendState.detectedPanelCount = 0
        const detectedCount = findChild(popup, "detected-panel-count")
        verify(detectedCount.visible)
        backendState.busy = true
        verify(!corner.enabled)
        verify(!offset.enabled)
        verify(!findChild(popup, "alignment-reset").enabled)
    }
}
