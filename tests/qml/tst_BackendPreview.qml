import QtQuick
import QtTest
import "../../share/plasma/io.github.avitretiak.Wayleaf4D/contents/ui/Nanoleaf" 1.0 as Nanoleaf

TestCase {
    id: testCase
    name: "BackendPreview"
    property var backend
    property var requests

    Component {
        id: backendComponent
        Nanoleaf.Backend {
            function callControl(method, args, success, failure) {
                testCase.requests.push({ method: method, success: success, failure: failure })
            }
        }
    }

    function init() {
        requests = []
        backend = createTemporaryObject(backendComponent, testCase)
        verify(backend !== null)
        backend.pollTimer.stop()
        backend.running = true
    }

    function request(method) {
        for (let i = 0; i < requests.length; ++i) {
            if (requests[i].method === method)
                return requests.splice(i, 1)[0]
        }
        fail("Missing request " + method)
    }

    function enablePreview() {
        backend.setPreviewEnabled(true)
        request("SetPreviewEnabled").success()
        verify(backend.previewReady)
    }

    function test_old_preview_completion_after_reopen_data() {
        return [{ tag: "success", fails: false }, { tag: "failure", fails: true }]
    }

    function test_old_preview_completion_after_reopen(data) {
        enablePreview()
        backend.getPreview()
        const oldRequest = request("GetPreview")
        backend.setPreviewEnabled(false)
        request("SetPreviewEnabled").success()
        enablePreview()
        const current = [{ id: 2, edge: "top", position: 0.5, span: 0.2, r: 1, g: 2, b: 3 }]
        backend.previewZones = current
        if (data.fails)
            oldRequest.failure("stale failure")
        else
            oldRequest.success(JSON.stringify([{ id: 1, edge: "top", position: 0.5, span: 0.2, r: 4, g: 5, b: 6 }]))
        compare(backend.previewZones[0].id, 2)
        backend.getPreview()
        request("GetPreview").success(JSON.stringify(current))
        compare(backend.previewZones[0].id, 2)
    }

    function test_refresh_error_clears_after_recovery() {
        backend.poll()
        request("GetState").success(JSON.stringify({ status: "error", running: false, error: "refresh failed", refreshError: true }))
        compare(backend.error, "refresh failed")
        backend.poll()
        request("GetState").success(JSON.stringify({ status: "stopped", running: false, error: "", refreshError: false }))
        compare(backend.error, "")
        compare(backend.status, "stopped")
    }

    function test_non_refresh_error_is_retained_when_poll_has_no_error() {
        backend.error = "operation failed"
        backend.poll()
        request("GetState").success(JSON.stringify({ status: "stopped", running: false, error: "", refreshError: false }))
        compare(backend.error, "operation failed")
    }
}
