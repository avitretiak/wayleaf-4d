import QtQuick
import org.kde.plasma.workspace.dbus as DBus

Item {
    id: root
    visible: false

    readonly property string service: "io.github.avitretiak.Wayleaf4D.Control"
    readonly property string objectPath: "/io/github/avitretiak/Wayleaf4D/Control"
    readonly property string interfaceName: "io.github.avitretiak.Wayleaf4D.Control"

    property var settings: ({
        brightness: 1.5,
        saturation_boost: 1.5,
        fps: 10,
        smooth: 0.35,
        black_threshold: 0.01,
        max_step: 12,
        edge_depth: 8,
        center_bleed: 0,
        peak_mix: 15,
        feather_depth: 5,
        letterbox: true,
        midtone_lift: false,
        strict_black: false
    })
    property string status: "stopped"
    property string error: ""
    property bool enabled: false
    property bool busy: false
    property bool deviceConnected: false
    property bool deviceBusy: false
    property string deviceHost: ""
    property var deviceInfo: ({})
    property var discoveredDevices: []
    property string deviceMessage: ""
    property var monitors: []
    property string selectedMonitor: ""

    property var _pendingSettings: ({})
    property int _settingsGeneration: 0
    property bool _settingsInFlight: false
    property var _flushCallback: null
    property bool _localBusy: false
    property bool _localDevicePending: false
    property int _stateGeneration: 0
    property bool _controlUnavailable: false
    property bool _idleCheckInFlight: false
    property var _idleSuccess: null
    property var _idleFailure: null

    property Timer pollTimer: Timer {
        interval: 2000
        repeat: true
        running: true
        onTriggered: root.poll()
    }

    property Timer settingsTimer: Timer {
        interval: 250
        repeat: false
        onTriggered: root.flushSettings()
    }
    property Timer controlIdleTimer: Timer {
        interval: 250
        repeat: false
        onTriggered: root.checkControlIdle()
    }

    Component.onCompleted: root.poll()

    function errorText(dbusError) {
        let parts = []
        if (dbusError && dbusError.name)
            parts.push(String(dbusError.name))
        if (dbusError && dbusError.message)
            parts.push(String(dbusError.message))
        return parts.length ? parts.join(": ") : "D-Bus request failed"
    }

    function callControl(method, args, success, failure) {
        let pending
        try {
            pending = DBus.SessionBus.asyncCall(new DBus.dbusMessage({
                service: service,
                path: objectPath,
                interface: interfaceName,
                member: method,
                arguments: args || []
            }))
        } catch (e) {
            if (failure)
                failure(String(e))
            return
        }
        if (!pending) {
            if (failure)
                failure("The D-Bus connection did not create a request")
            return
        }
        pending.finished.connect(function() {
            if (pending.isError || !pending.isValid) {
                if (failure)
                    failure(errorText(pending.error))
            } else if (success) {
                success(pending.value)
            }
        })
    }

    function reportControlError(context, detail) {
        error = context + ": " + detail + ". Check that wayleaf-4d is installed and inspect the user journal."
    }
    function settingsFrom(values) {
        const result = ({})
        for (const key in settings) {
            if (!Object.prototype.hasOwnProperty.call(settings, key))
                continue
            result[key] = values && Object.prototype.hasOwnProperty.call(values, key) ? values[key] : settings[key]
        }
        return result
    }

    function applyDeviceState(state) {
        if (typeof state.deviceConnected === "boolean")
            deviceConnected = state.deviceConnected
        if (typeof state.deviceBusy === "boolean")
            deviceBusy = state.deviceBusy
        if (typeof state.deviceHost === "string")
            deviceHost = state.deviceHost
        if (state.deviceInfo && typeof state.deviceInfo === "object" && !Array.isArray(state.deviceInfo))
            deviceInfo = state.deviceInfo
        if (Array.isArray(state.discoveredDevices))
            discoveredDevices = state.discoveredDevices
        if (typeof state.deviceMessage === "string")
            deviceMessage = state.deviceMessage
        if (Array.isArray(state.monitors))
            monitors = state.monitors
    }

    function applyControlState(state) {
        if (typeof state.status === "string")
            status = state.status
        if (typeof state.enabled === "boolean")
            enabled = state.enabled
        if (typeof state.busy === "boolean")
            busy = state.busy
        if (typeof state.selectedMonitor === "string")
            selectedMonitor = state.selectedMonitor
        if (state.settings && typeof state.settings === "object" && !Array.isArray(state.settings))
            settings = settingsFrom(state.settings)
        error = typeof state.error === "string" ? state.error : ""
        applyDeviceState(state)
    }

    function poll() {
        if (_pollInFlight)
            return
        _pollInFlight = true
        const settingsGeneration = _settingsGeneration
        const stateGeneration = _stateGeneration
        callControl("GetState", [], function(value) {
            _pollInFlight = false
            if (stateGeneration !== _stateGeneration) {
                poll()
                return
            }
            let state
            try {
                state = JSON.parse(String(value))
            } catch (e) {
                if (stateGeneration === _stateGeneration && !_localBusy) {
                    _controlUnavailable = true
                    status = "error"
                    reportControlError("The Wayleaf control service returned invalid state", String(e))
                }
                return
            }
            if (!state || typeof state !== "object" || Array.isArray(state)) {
                if (stateGeneration === _stateGeneration && !_localBusy) {
                    _controlUnavailable = true
                    status = "error"
                    reportControlError("The Wayleaf control service returned invalid state", "expected a JSON object")
                }
                return
            }
            const wasUnavailable = _controlUnavailable
            _controlUnavailable = false
            if (stateGeneration === _stateGeneration && !_localBusy) {
                if (typeof state.status === "string")
                    status = state.status
                if (typeof state.enabled === "boolean")
                    enabled = state.enabled
                if (typeof state.busy === "boolean")
                    busy = state.busy
            }
            if (settingsGeneration === _settingsGeneration && Object.keys(_pendingSettings).length === 0 && !_settingsInFlight && state.settings && typeof state.settings === "object" && !Array.isArray(state.settings))
                settings = settingsFrom(state.settings)
            if (stateGeneration === _stateGeneration && !_localDevicePending) {
                applyDeviceState(state)
                if (!_localBusy && typeof state.selectedMonitor === "string")
                    selectedMonitor = state.selectedMonitor
            }
            if (stateGeneration === _stateGeneration && !_localBusy) {
                if (state.error)
                    error = String(state.error)
                else if (wasUnavailable)
                    error = ""
            }
        }, function(detail) {
            _pollInFlight = false
            if (stateGeneration !== _stateGeneration) {
                poll()
                return
            }
            if (stateGeneration === _stateGeneration && !_localBusy) {
                _controlUnavailable = true
                status = "error"
                reportControlError("Could not contact the Wayleaf 4D control service", detail)
            }
        })
    }

    property bool _pollInFlight: false

    function setSetting(key, value) {
        if (busy || deviceBusy || !Object.prototype.hasOwnProperty.call(settings, key) || settings[key] === value)
            return
        const nextSettings = Object.assign({}, settings)
        nextSettings[key] = value
        settings = nextSettings
        const nextPending = Object.assign({}, _pendingSettings)
        nextPending[key] = value
        _pendingSettings = nextPending
        ++_settingsGeneration
        settingsTimer.restart()
    }

    function flushPending(callback) {
        settingsTimer.stop()
        _flushCallback = function(success) {
            if (!callback)
                return
            if (!success) {
                callback(false)
                return
            }
            waitForControlIdle(function() {
                callback(true)
            }, function(detail) {
                reportControlError("Could not confirm the control service is idle", detail)
                callback(false)
            })
        }
        flushSettings()
    }

    function completeFlush(success) {
        const callback = _flushCallback
        _flushCallback = null
        if (callback)
            callback(success)
    }

    function waitForControlIdle(success, failure) {
        if (_idleSuccess) {
            failure("Another control-idle request is already in progress")
            return
        }
        _idleSuccess = success
        _idleFailure = failure
        checkControlIdle()
    }

    function completeIdleWait(success, value) {
        controlIdleTimer.stop()
        const callback = success ? _idleSuccess : _idleFailure
        _idleSuccess = null
        _idleFailure = null
        if (callback)
            callback(value)
    }

    function checkControlIdle() {
        if (_idleCheckInFlight || !_idleSuccess)
            return
        _idleCheckInFlight = true
        callControl("GetState", [], function(value) {
            _idleCheckInFlight = false
            let state
            try {
                state = JSON.parse(String(value))
            } catch (e) {
                completeIdleWait(false, String(e))
                return
            }
            if (!state || typeof state !== "object" || typeof state.busy !== "boolean"
                    || typeof state.deviceBusy !== "boolean") {
                completeIdleWait(false, "The control service returned no operation state")
            } else if (state.busy || state.deviceBusy) {
                controlIdleTimer.restart()
            } else {
                completeIdleWait(true, state)
            }
        }, function(detail) {
            _idleCheckInFlight = false
            completeIdleWait(false, detail)
        })
    }

    function flushSettings() {
        if (_settingsInFlight)
            return
        if (Object.keys(_pendingSettings).length === 0) {
            completeFlush(true)
            return
        }
        const generation = _settingsGeneration
        const payload = JSON.stringify(_pendingSettings)
        _settingsInFlight = true
        const fail = function(context, detail) {
            _settingsInFlight = false
            reportControlError(context, detail)
            completeFlush(false)
        }
        const settingsApplied = function(state) {
            _settingsInFlight = false
            error = state && typeof state.error === "string" ? state.error : ""
            if (error) {
                completeFlush(false)
                return
            }
            if (generation === _settingsGeneration)
                _pendingSettings = ({})
            if (Object.keys(_pendingSettings).length > 0) {
                if (!settingsTimer.running)
                    flushSettings()
            } else {
                completeFlush(true)
            }
        }
        waitForControlIdle(function() {
            callControl("SetSettings", [payload], function() {
                waitForControlIdle(settingsApplied, function(detail) {
                    fail("Could not confirm settings were applied", detail)
                })
            }, function(detail) {
                fail("Could not apply settings", detail)
            })
        }, function(detail) {
            fail("Could not wait for the control service to become idle", detail)
        })
    }

    function setEnabled(on) {
        if (busy || deviceBusy)
            return
        const previousStatus = status
        const previousEnabled = enabled
        const target = Boolean(on)
        busy = true
        _localBusy = true
        ++_stateGeneration
        status = target ? "starting" : "stopping"
        enabled = target
        error = ""
        flushPending(function(ok) {
            if (!ok) {
                busy = false
                _localBusy = false
                status = previousStatus
                enabled = previousEnabled
                ++_stateGeneration
                return
            }
            callControl("SetEnabled", [target], function() {
                waitForControlIdle(function(state) {
                    _localBusy = false
                    busy = false
                    ++_stateGeneration
                    applyControlState(state)
                    poll()
                }, function(detail) {
                    _localBusy = false
                    busy = false
                    ++_stateGeneration
                    status = "error"
                    reportControlError("Could not confirm synchronization state", detail)
                    poll()
                })
            }, function(detail) {
                _localBusy = false
                busy = false
                ++_stateGeneration
                enabled = previousEnabled
                status = "error"
                reportControlError("Could not change synchronization state", detail)
                poll()
            })
        })
    }

    function selectMonitor(name) {
        if (busy || deviceBusy || !name || name === selectedMonitor)
            return
        const previousStatus = status
        const previousMonitor = selectedMonitor
        busy = true
        _localBusy = true
        ++_stateGeneration
        error = ""
        if (enabled)
            status = "stopping"
        flushPending(function(ok) {
            if (!ok) {
                busy = false
                _localBusy = false
                status = previousStatus
                ++_stateGeneration
                return
            }
            callControl("SelectMonitor", [name], function() {
                waitForControlIdle(function(state) {
                    _localBusy = false
                    busy = false
                    ++_stateGeneration
                    applyControlState(state)
                    poll()
                }, function(detail) {
                    _localBusy = false
                    busy = false
                    ++_stateGeneration
                    status = "error"
                    reportControlError("Could not confirm capture monitor selection", detail)
                    poll()
                })
            }, function(detail) {
                _localBusy = false
                busy = false
                ++_stateGeneration
                selectedMonitor = previousMonitor
                status = "error"
                reportControlError("Could not select capture monitor", detail)
                poll()
            })
        })
    }

    function clearError() {
        error = ""
        ++_stateGeneration
        _controlUnavailable = false
        callControl("ClearError", [], function() {
            ++_stateGeneration
            poll()
        }, function(detail) {
            reportControlError("Could not clear the error", detail)
        })
    }

    function deviceAction(method, args, message, flushSettingsFirst) {
        if (busy || deviceBusy)
            return
        deviceBusy = true
        _localDevicePending = true
        ++_stateGeneration
        deviceMessage = message
        if (method === "DiscoverDevices")
            discoveredDevices = []
        if (method === "RefreshDeviceInfo")
            deviceInfo = ({})
        const invoke = function() {
            callControl(method, args, function() {
                waitForControlIdle(function(state) {
                    _localDevicePending = false
                    deviceBusy = false
                    ++_stateGeneration
                    applyDeviceState(state)
                    poll()
                }, function(detail) {
                    _localDevicePending = false
                    deviceBusy = false
                    deviceMessage = "Could not confirm " + method + " completion: " + detail
                    ++_stateGeneration
                    reportControlError("Could not confirm " + method + " completion", detail)
                    poll()
                })
            }, function(detail) {
                _localDevicePending = false
                deviceBusy = false
                deviceMessage = method + " failed: " + detail
                ++_stateGeneration
                reportControlError(method + " failed", detail)
                poll()
            })
        }
        if (flushSettingsFirst) {
            flushPending(function(ok) {
                if (ok) {
                    invoke()
                } else {
                    _localDevicePending = false
                    deviceBusy = false
                    ++_stateGeneration
                    deviceMessage = "Settings could not be saved before the device operation."
                }
            })
        } else {
            invoke()
        }
    }

    function discoverDevices() {
        deviceAction("DiscoverDevices", [], "Searching the local network for Nanoleaf devices…", true)
    }

    function pairDevice(host) {
        if (enabled || status !== "stopped") {
            deviceMessage = "Turn sync off before pairing or forgetting a device."
            return
        }
        deviceAction("PairDevice", [String(host).trim()], "Pairing. Hold the 4D power button for 5 seconds first…", true)
    }

    function refreshDeviceInfo() {
        if (!deviceConnected || deviceBusy)
            return
        deviceAction("RefreshDeviceInfo", [], "Loading device details…", true)
    }

    function forgetDevice() {
        if (enabled || status !== "stopped") {
            deviceMessage = "Turn sync off before pairing or forgetting a device."
            return
        }
        deviceAction("ForgetDevice", [], "Removing saved device credentials…", true)
    }
}
