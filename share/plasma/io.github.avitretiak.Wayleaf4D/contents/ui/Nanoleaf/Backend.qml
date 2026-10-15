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
    property bool running: false
    property string error: ""
    property bool startAtLogin: false
    property bool busy: false
    property bool deviceConnected: false
    property bool deviceBusy: false
    property string deviceHost: ""
    property var deviceInfo: ({})
    property var discoveredDevices: []
    property string deviceMessage: ""
    property var monitors: []
    property string monitorError: ""
    property string selectedMonitor: ""
    property var previewZones: []
    property bool previewEnabled: false
    property bool previewReady: false
    property bool _previewCallInFlight: false
    property int _previewGeneration: 0

    property var _pendingSettings: ({})
    readonly property bool settingsPending: Object.keys(_pendingSettings).length > 0
    readonly property bool settingsRetryAvailable: settingsPending && !_settingsInFlight
    property string settingsError: ""
    property int _settingsGeneration: 0
    property bool _settingsInFlight: false
    property int _settingsRetryCount: 0
    property var _flushCallback: null
    property bool _localBusy: false
    property bool _localDevicePending: false
    property bool deviceInfoStale: false
    property int _stateGeneration: 0
    property bool _controlUnavailable: false
    property bool _idleCheckInFlight: false
    property var _idleSuccess: null
    property var _idleFailure: null
    property Timer settingsRetryTimer: Timer {
        interval: 600
        repeat: false
        onTriggered: root.flushSettings()
    }

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
    function isSettingPending(key) {
        return Object.prototype.hasOwnProperty.call(_pendingSettings, key)
    }

    function applyDeviceState(state, refreshConfirmed, clearDeviceInfo) {
        if (typeof state.deviceConnected === "boolean")
            deviceConnected = state.deviceConnected
        if (typeof state.deviceBusy === "boolean")
            deviceBusy = state.deviceBusy
        if (typeof state.deviceHost === "string")
            deviceHost = state.deviceHost
        const serviceReportsStale = typeof state.deviceInfoStale === "boolean"
        if (serviceReportsStale)
            deviceInfoStale = state.deviceInfoStale
        if (!deviceConnected && clearDeviceInfo) {
            deviceInfo = state.deviceInfo && typeof state.deviceInfo === "object" && !Array.isArray(state.deviceInfo)
                ? state.deviceInfo : ({})
            deviceInfoStale = false
        } else if (state.deviceInfo && typeof state.deviceInfo === "object" && !Array.isArray(state.deviceInfo)
                && (!deviceInfoStale || (refreshConfirmed && serviceReportsStale && !state.deviceInfoStale))) {
            deviceInfo = state.deviceInfo
            if (refreshConfirmed && !serviceReportsStale)
                deviceInfoStale = false
        }
        if (Array.isArray(state.discoveredDevices))
            discoveredDevices = state.discoveredDevices
        if (typeof state.deviceMessage === "string") {
            if (clearDeviceInfo || (!deviceConnected && deviceMessage.startsWith("Warning:"))) {
                if (state.deviceMessage.length > 0)
                    deviceMessage = state.deviceMessage.startsWith("Warning:")
                        ? state.deviceMessage : "Warning: " + state.deviceMessage
                else if (clearDeviceInfo)
                    deviceMessage = ""
            } else {
                deviceMessage = state.deviceMessage
            }
        }
        if (typeof state.monitorError === "string")
            monitorError = state.monitorError
        if (Array.isArray(state.monitors))
            monitors = state.monitors

    }
    function applyControlState(state) {
        if (typeof state.status === "string")
            status = state.status
        if (typeof state.running === "boolean")
            running = state.running
        if (typeof state.enabled === "boolean")
            startAtLogin = state.enabled
        if (typeof state.busy === "boolean")
            busy = state.busy
        if (typeof state.selectedMonitor === "string")
            selectedMonitor = state.selectedMonitor
        if (!settingsPending && !_settingsInFlight && state.settings && typeof state.settings === "object" && !Array.isArray(state.settings))
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
                if (typeof state.running === "boolean")
                    running = state.running
                if (typeof state.enabled === "boolean")
                    startAtLogin = state.enabled
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

    function setPreviewEnabled(enabled) {
        const target = Boolean(enabled)
        if (previewEnabled === target && (!target || previewReady))
            return
        previewEnabled = target
        previewReady = false
        previewZones = []
        const generation = ++_previewGeneration
        callControl("SetPreviewEnabled", [target], function() {
            if (generation === _previewGeneration && target)
                previewReady = true
        }, function() {
            if (generation === _previewGeneration)
                previewReady = false
        })
    }

    function getPreview() {
        if (!previewEnabled || !previewReady || !running || _previewCallInFlight)
            return
        _previewCallInFlight = true
        callControl("GetPreview", [], function(value) {
            _previewCallInFlight = false
            if (!previewEnabled || !previewReady || !running)
                return
            let zones
            try {
                zones = JSON.parse(String(value))
            } catch (e) {
                previewZones = []
                return
            }
            if (!Array.isArray(zones)) {
                previewZones = []
                return
            }
            previewZones = zones.filter(function(zone) {
                return zone && Number.isInteger(zone.id) && zone.id >= 0 && zone.id <= 65535
                    && ["top", "right", "bottom", "left"].indexOf(zone.edge) >= 0
                    && Number.isFinite(Number(zone.position)) && Number(zone.position) >= 0 && Number(zone.position) <= 1
                    && Number.isFinite(Number(zone.span)) && Number(zone.span) >= 0 && Number(zone.span) <= 1
                    && [zone.r, zone.g, zone.b].every(function(channel) {
                        return Number.isInteger(channel) && channel >= 0 && channel <= 255
                    })
            }).map(function(zone) {
                return { id: zone.id, edge: zone.edge, position: zone.position, span: zone.span,
                    r: zone.r, g: zone.g, b: zone.b }
            })
        }, function() {
            _previewCallInFlight = false
            previewZones = []
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
        _settingsRetryCount = 0
        settingsRetryTimer.stop()
        settingsError = ""
        settingsTimer.restart()
    }

    function retrySettings() {
        if (!settingsPending || busy || deviceBusy || _settingsInFlight)
            return
        settingsTimer.stop()
        settingsRetryTimer.stop()
        _settingsRetryCount = 0
        settingsError = ""
        flushSettings()
    }

    function flushPending(callback) {
        settingsTimer.stop()
        settingsRetryTimer.stop()
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
        settingsRetryTimer.stop()
        if (!settingsPending) {
            completeFlush(true)
            return
        }
        const generation = _settingsGeneration
        const submittedSettings = Object.assign({}, _pendingSettings)
        const payload = JSON.stringify(submittedSettings)
        _settingsInFlight = true
        const fail = function(context, detail) {
            _settingsInFlight = false
            settingsError = context + ": " + detail + ". Changes remain pending; "
                + (_settingsRetryCount === 0 ? "retrying once automatically." : "use Retry settings to try again.")
            if (_settingsRetryCount === 0) {
                _settingsRetryCount = 1
                settingsRetryTimer.restart()
            }
            completeFlush(false)
        }
        const settingsApplied = function(state) {
            _settingsInFlight = false
            if (state && typeof state.error === "string" && state.error.length > 0) {
                fail("Could not apply settings", state.error)
                return
            }
            if (!state || !state.settings || typeof state.settings !== "object" || Array.isArray(state.settings)) {
                fail("Could not confirm settings", "the service returned no settings values")
                return
            }
            for (const key in submittedSettings) {
                if (!Object.prototype.hasOwnProperty.call(state.settings, key)) {
                    fail("Could not confirm settings", "the service did not return " + key)
                    return
                }
                const expected = submittedSettings[key]
                const actual = state.settings[key]
                if (typeof expected === "number" ? typeof actual !== "number" || Math.abs(actual - expected) > 0.000001 : actual !== expected) {
                    fail("Could not confirm settings", key + " does not match the requested value")
                    return
                }
            }
            if (generation === _settingsGeneration) {
                settings = settingsFrom(state.settings)
                _pendingSettings = ({})
                _settingsRetryCount = 0
                settingsError = ""
            }
            if (settingsPending) {
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

    function runControlAction(method, args, transition, flushSettingsFirst, failureContext) {
        if (busy || deviceBusy)
            return
        const previousStatus = status
        busy = true
        _localBusy = true
        ++_stateGeneration
        if (transition)
            status = transition
        error = ""
        const fail = function(context, detail) {
            _localBusy = false
            busy = false
            ++_stateGeneration
            status = "error"
            reportControlError(context, detail)
            poll()
        }
        const invoke = function() {
            callControl(method, args, function() {
                waitForControlIdle(function(state) {
                    _localBusy = false
                    busy = false
                    ++_stateGeneration
                    applyControlState(state)
                    poll()
                }, function(detail) {
                    fail("Could not confirm " + failureContext, detail)
                })
            }, function(detail) {
                fail("Could not " + failureContext, detail)
            })
        }
        if (flushSettingsFirst) {
            flushPending(function(ok) {
                if (!ok) {
                    busy = false
                    _localBusy = false
                    status = previousStatus
                    ++_stateGeneration
                    return
                }
                invoke()
            })
        } else {
            invoke()
        }
    }

    function run() {
        if (running || status === "starting")
            return
        runControlAction("Run", [], "starting", true, "start screen synchronization")
    }

    function stop() {
        if (!running && status === "stopped")
            return
        runControlAction("Stop", [], "stopping", false, "stop screen synchronization")
    }

    function setStartAtLogin(on) {
        if (busy || deviceBusy)
            return
        const previous = startAtLogin
        const target = Boolean(on)
        if (target === previous)
            return
        busy = true
        _localBusy = true
        ++_stateGeneration
        startAtLogin = target
        error = ""
        callControl("SetStartAtLogin", [target], function() {
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
                startAtLogin = previous
                reportControlError("Could not confirm the start-at-login setting", detail)
                poll()
            })
        }, function(detail) {
            _localBusy = false
            busy = false
            ++_stateGeneration
            startAtLogin = previous
            reportControlError("Could not change the start-at-login setting", detail)
            poll()
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
        if (running)
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
        error = ""
        if (method === "DiscoverDevices")
            discoveredDevices = []
        const invoke = function() {
            if (method === "RefreshDeviceInfo")
                deviceInfoStale = true
            callControl(method, args, function() {
                waitForControlIdle(function(state) {
                    _localDevicePending = false
                    deviceBusy = false
                    ++_stateGeneration
                    error = ""
                    applyDeviceState(state, method === "RefreshDeviceInfo", method === "StopAndForgetDevice")
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
        if (running || status !== "stopped") {
            deviceMessage = "Stop synchronization before pairing or forgetting a device."
            return
        }
        deviceAction("PairDevice", [String(host).trim()], "Pairing. Hold the 4D power button for 5 seconds first…", true)
    }

    function refreshDeviceInfo() {
        if (!deviceConnected || deviceBusy)
            return
        deviceAction("RefreshDeviceInfo", [], "Loading device details…", true)
    }

    function disconnectDevice() {
        deviceAction("StopAndForgetDevice", [], "Stopping synchronization and disconnecting…", true)
    }
}
