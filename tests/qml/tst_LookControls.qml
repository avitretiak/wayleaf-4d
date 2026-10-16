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
        saturation_boost: 1.0, smooth: 0.35, max_step: 12, edge_depth: 8,
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
        property bool fineTuneExpanded: false
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
            popup.destroy()
            popup = null
            wait(0)
        }
    }

    function init() {
        savedState.customSettings = ""
        savedState.selectedPreset = ""
        savedState.fineTuneExpanded = false
        backendState.settings = normalized(Object.assign({}, defaults, customValues))
        openPopup()
    }

    function cleanup() { closePopup() }

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

    function test_custom_survives_balanced_and_popup_reopen() {
        compare(popup.selectedPreset, "Custom")
        selectLook(1)
        compare(backendState.settings.peak_mix, 15)
        const snapshot = JSON.parse(savedState.customSettings)
        for (const key of Object.keys(customValues))
            verify(Math.abs(snapshot[key] - customValues[key]) < 0.000001, key)
        selectLook(3)
        verifyCustomValues()
        verify(popup.fineTuneExpanded)
        selectLook(1)
        closePopup()
        openPopup()
        compare(popup.selectedPreset, "Balanced")
        selectLook(3)
        verifyCustomValues()
    }

    function test_each_preset_hides_fine_tuning_and_preserves_custom() {
        const names = ["Subtle", "Balanced", "Vivid"]
        const expectedSaturation = [0.8, 1.0, 1.9]
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
        compare(popup.selectedPreset, "Custom")
        verify(findChild(popup, "fine-tune-controls").visible)
        closePopup()
        openPopup()
        compare(popup.selectedPreset, "Custom")
        compare(backendState.settings.peak_mix, 15)
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
        backendState.setSetting("mode", "1D")
        verify(!findChild(popup, "fine-tune-controls").visible)
        for (const mode of ["2D", "3D", "4D"]) {
            backendState.setSetting("mode", mode)
            verify(findChild(popup, "fine-tune-controls").visible, mode)
        }
    }

    function test_alignment_survives_look_changes_and_group_reset() {
        const controls = findChild(popup, "alignment-controls")
        const corner = findChild(popup, "alignment-start-corner")
        const reverse = findChild(popup, "alignment-reverse-direction")
        const offset = findChild(popup, "alignment-perimeter-offset")
        corner.forceActiveFocus()
        keyClick(Qt.Key_Down)
        keyClick(Qt.Key_Down)
        compare(backendState.settings.start_corner, "top_left")
        reverse.forceActiveFocus()
        keyClick(Qt.Key_Space)
        offset.forceActiveFocus()
        keyClick(Qt.Key_Up)
        compare(backendState.settings.perimeter_offset, 1)
        for (const look of ["Subtle", "Balanced", "Vivid", "Custom"]) {
            popup.choosePreset(look)
            verify(controls.visible)
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
            verify(!controls.visible)
        }
    }
}
