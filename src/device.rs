use crate::config::Config;
use crate::sampling::{Color, PanelPosition, Zone};
use crate::{AppError, Result};
use curl::easy::Easy;
use serde_json::Value;
use std::collections::HashSet;
use std::net::{ToSocketAddrs, UdpSocket};
use std::thread;
use std::time::{Duration, Instant};

const MAX_PANELS: usize = 40;
const MIN_PANELS: usize = 1;
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const NORMAL_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const NORMAL_TIMEOUT: Duration = Duration::from_secs(5);
const PAIR_WINDOW: Duration = Duration::from_secs(30);
const PAIR_CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
const PAIR_TIMEOUT: Duration = Duration::from_secs(2);
const EXT_CONTROL_PAYLOAD: &[u8] =
    br#"{"write":{"command":"display","animType":"extControl","extControlVersion":"v2"}}"#;
const POWER_OFF_PAYLOAD: &[u8] = br#"{"on":{"value":false}}"#;

struct HttpResponse {
    status: u32,
    body: Vec<u8>,
}

fn invalid(message: &'static str) -> AppError {
    AppError::new(message)
}

fn validate_host(host: &str) -> Result<String> {
    if host.is_empty() || host.len() > 253 || !host.is_ascii() {
        return Err(invalid("invalid Nanoleaf host"));
    }
    if host.parse::<std::net::IpAddr>().is_ok() {
        return Ok(host.to_owned());
    }

    let dns_name = host.strip_suffix('.').unwrap_or(host);
    if dns_name.is_empty()
        || dns_name.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || !label.as_bytes()[0].is_ascii_alphanumeric()
                || !label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        return Err(invalid("invalid Nanoleaf host"));
    }
    Ok(dns_name.to_owned())
}

fn validate_token(token: &str) -> Result<()> {
    if token.is_empty()
        || token.len() > 256
        || token == "."
        || token == ".."
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~'))
    {
        return Err(invalid("invalid Nanoleaf token"));
    }
    Ok(())
}

fn api_url(cfg: &Config, path: &str, token: Option<&str>) -> Result<String> {
    let host = validate_host(&cfg.host)?;
    let authority = if host.parse::<std::net::Ipv6Addr>().is_ok() {
        format!("[{host}]")
    } else {
        host
    };
    let route = if let Some(token) = token {
        validate_token(token)?;
        format!("{token}{path}")
    } else {
        path.strip_prefix('/').unwrap_or(path).to_owned()
    };
    Ok(format!("http://{authority}:16021/api/v1/{route}"))
}

fn retryable_curl_error(error: &curl::Error) -> bool {
    error.is_couldnt_resolve_host()
        || error.is_couldnt_resolve_proxy()
        || error.is_couldnt_connect()
        || error.is_operation_timedout()
        || error.is_send_error()
        || error.is_recv_error()
        || error.is_got_nothing()
        || error.is_again()
}

fn curl_failure(error: &curl::Error) -> AppError {
    if retryable_curl_error(error) {
        AppError::temporary("Nanoleaf HTTP request failed")
    } else {
        AppError::new("Nanoleaf HTTP request failed")
    }
}

fn http_status_failure(status: u32) -> AppError {
    let message = format!("Nanoleaf API returned HTTP {status}");
    if matches!(status, 408 | 425 | 429 | 500 | 502 | 503 | 504) {
        AppError::temporary(message)
    } else {
        AppError::new(message)
    }
}

fn request_raw(
    cfg: &Config,
    path: &str,
    method: &str,
    payload: Option<&[u8]>,
    connect_timeout: Duration,
    timeout: Duration,
    token: Option<&str>,
) -> Result<HttpResponse> {
    let url = api_url(cfg, path, token)?;
    let mut easy = Easy::new();
    easy.url(&url)
        .map_err(|_| AppError::new("cannot configure Nanoleaf HTTP request"))?;
    easy.proxy("")
        .map_err(|_| AppError::new("cannot configure Nanoleaf HTTP request"))?;
    easy.signal(false)
        .map_err(|_| AppError::new("cannot configure Nanoleaf HTTP request"))?;
    easy.connect_timeout(connect_timeout)
        .map_err(|_| AppError::new("cannot configure Nanoleaf HTTP request"))?;
    easy.timeout(timeout)
        .map_err(|_| AppError::new("cannot configure Nanoleaf HTTP request"))?;
    if let Some(body) = payload {
        easy.post_fields_copy(body)
            .map_err(|_| AppError::new("cannot configure Nanoleaf HTTP request"))?;
    }
    easy.custom_request(method)
        .map_err(|_| AppError::new("cannot configure Nanoleaf HTTP request"))?;

    let mut body = Vec::new();
    let mut too_large = false;
    let perform_result = {
        let mut transfer = easy.transfer();
        transfer
            .write_function(|chunk| {
                if chunk.len() > MAX_RESPONSE_BYTES.saturating_sub(body.len()) {
                    too_large = true;
                    return Ok(0);
                }
                body.extend_from_slice(chunk);
                Ok(chunk.len())
            })
            .map_err(|_| AppError::new("cannot configure Nanoleaf HTTP response handler"))?;
        transfer.perform()
    };
    if too_large {
        return Err(AppError::new("Nanoleaf response exceeds the size limit"));
    }
    if let Err(error) = perform_result {
        return Err(curl_failure(&error));
    }
    let status = easy
        .response_code()
        .map_err(|_| AppError::new("cannot read Nanoleaf HTTP response status"))?;
    Ok(HttpResponse { status, body })
}

fn request(cfg: &Config, path: &str, method: &str, payload: Option<&[u8]>) -> Result<Vec<u8>> {
    let response = request_raw(
        cfg,
        path,
        method,
        payload,
        NORMAL_CONNECT_TIMEOUT,
        NORMAL_TIMEOUT,
        Some(&cfg.token),
    )?;
    if !(200..300).contains(&response.status) {
        return Err(http_status_failure(response.status));
    }
    Ok(response.body)
}

fn parse_json(body: &[u8]) -> Result<Value> {
    serde_json::from_slice(body).map_err(|_| AppError::new("invalid Nanoleaf response JSON"))
}

fn parse_device_info(body: &[u8]) -> Result<Value> {
    let root = parse_json(body)?;
    let object = root
        .as_object()
        .ok_or_else(|| AppError::new("device info response is not an object"))?;
    let mut safe = serde_json::Map::new();
    for key in [
        "name",
        "model",
        "serialNo",
        "firmwareVersion",
        "hardwareVersion",
        "manufacturer",
        "productName",
    ] {
        if let Some(value) = object.get(key) {
            if value.is_string() || value.is_boolean() || value.is_number() {
                safe.insert(key.to_owned(), value.clone());
            }
        }
    }
    Ok(Value::Object(safe))
}

pub fn device_info(cfg: &Config) -> Result<Value> {
    let body = request(cfg, "/", "GET", None)?;
    parse_device_info(&body)
}

fn parse_panel_layout(body: &[u8]) -> Result<Vec<PanelPosition>> {
    let root = parse_json(body)?;
    let object = root
        .as_object()
        .ok_or_else(|| AppError::new("Nanoleaf layout response is not an object"))?;
    let count = object
        .get("numPanels")
        .and_then(Value::as_u64)
        .and_then(|count| usize::try_from(count).ok())
        .filter(|count| (MIN_PANELS..=MAX_PANELS).contains(count))
        .ok_or_else(|| AppError::new("Nanoleaf 4D layout must contain 1..40 segments"))?;
    let positions = object
        .get("positionData")
        .and_then(Value::as_array)
        .filter(|positions| positions.len() == count)
        .ok_or_else(|| AppError::new("Nanoleaf 4D layout must contain 1..40 segments"))?;
    let mut seen = HashSet::with_capacity(count);
    let mut panels = Vec::with_capacity(count);
    for entry in positions {
        let entry = entry
            .as_object()
            .ok_or_else(|| AppError::new("malformed panel layout entry"))?;
        let id = entry
            .get("panelId")
            .and_then(Value::as_u64)
            .filter(|id| *id <= u16::MAX as u64)
            .ok_or_else(|| AppError::new("invalid panel id in layout"))? as u16;
        if !seen.insert(id) {
            return Err(AppError::new("duplicate panel id in layout"));
        }
        let x = entry
            .get("x")
            .and_then(Value::as_i64)
            .ok_or_else(|| AppError::new("panel layout entry has invalid x coordinate"))?;
        let y = entry
            .get("y")
            .and_then(Value::as_i64)
            .ok_or_else(|| AppError::new("panel layout entry has invalid y coordinate"))?;
        panels.push(PanelPosition { id, x, y });
    }
    Ok(panels)
}

pub fn panel_layout(cfg: &Config) -> Result<Vec<PanelPosition>> {
    let body = request(cfg, "/panelLayout/layout", "GET", None)?;
    parse_panel_layout(&body)
}

fn parse_stream_port(body: &[u8], configured_port: u16) -> Result<u16> {
    if body.is_empty() {
        return Ok(configured_port);
    }
    let root = parse_json(body)?;
    let object = root
        .as_object()
        .ok_or_else(|| AppError::new("invalid Nanoleaf stream-control response"))?;
    let Some(port) = object.get("streamControlPort") else {
        return Ok(configured_port);
    };
    let port = port
        .as_u64()
        .filter(|port| (1..=u16::MAX as u64).contains(port))
        .ok_or_else(|| AppError::new("invalid Nanoleaf stream port"))?;
    Ok(port as u16)
}

pub fn enable_stream(cfg: &Config) -> Result<u16> {
    let body = request(cfg, "/effects", "PUT", Some(EXT_CONTROL_PAYLOAD))?;
    let port = u16::try_from(cfg.port)
        .map_err(|_| AppError::new("invalid configured Nanoleaf stream port"))?;
    parse_stream_port(&body, port)
}

fn parse_selected_effect(body: &[u8]) -> Option<String> {
    let raw = std::str::from_utf8(body).ok()?.trim();
    let selected = match serde_json::from_str::<Value>(raw) {
        Ok(Value::String(effect)) => effect,
        Ok(Value::Object(object)) => object.get("select")?.as_str()?.to_owned(),
        Ok(_) if raw.starts_with('{') || raw.starts_with('[') => return None,
        _ => raw.to_owned(),
    };
    if selected.is_empty()
        || selected.chars().any(char::is_control)
        || matches!(
            selected.as_str(),
            "*Static*" | "*Dynamic*" | "*Solid*" | "*ExtControl*"
        )
        || selected.eq_ignore_ascii_case("extControl")
    {
        return None;
    }
    Some(selected)
}

pub fn selected_effect(cfg: &Config) -> Option<String> {
    request(cfg, "/effects/select", "GET", None)
        .ok()
        .and_then(|body| parse_selected_effect(&body))
}

fn effect_selection_payload(effect: &str) -> Result<Vec<u8>> {
    serde_json::to_vec(&serde_json::json!({"select": effect}))
        .map_err(|_| AppError::new("cannot encode Nanoleaf effect selection"))
}

pub fn restore_effect(cfg: &Config, effect: &str) -> Result<()> {
    let payload = effect_selection_payload(effect)?;
    request(cfg, "/effects", "PUT", Some(&payload))?;
    Ok(())
}

pub fn power_off(cfg: &Config) -> Result<()> {
    request(cfg, "/state", "PUT", Some(POWER_OFF_PAYLOAD))?;
    Ok(())
}

pub fn pair_device(cfg: &mut Config) -> Result<()> {
    let host = validate_host(&cfg.host)?;
    eprintln!("Hold the Nanoleaf 4D power button for 5 seconds, then pair within 30 seconds.");
    let deadline = Instant::now() + PAIR_WINDOW;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining < Duration::from_millis(1) {
            break;
        }
        let timeout = PAIR_TIMEOUT.min(remaining);
        let connect_timeout = PAIR_CONNECT_TIMEOUT.min(timeout);
        match request_raw(cfg, "/new", "POST", None, connect_timeout, timeout, None) {
            Ok(response) if (200..300).contains(&response.status) => {
                let root = parse_json(&response.body)?;
                let token = root
                    .as_object()
                    .and_then(|object| object.get("auth_token"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| AppError::new("pairing response has no valid auth_token"))?;
                validate_token(token)?;
                cfg.token = token.to_owned();
                cfg.save_credentials()?;
                eprintln!("Paired with {host}; credentials saved.");
                return Ok(());
            }
            Ok(_) => {}
            Err(error) if error.temporary => {}
            Err(error) => return Err(error),
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        thread::sleep(Duration::from_millis(500).min(remaining));
    }
    Err(AppError::new(
        "pairing timed out; re-enable Connect to API in the Nanoleaf app and retry",
    ))
}

fn temporary_io_error(error: &std::io::Error) -> bool {
    use std::io::ErrorKind;
    matches!(
        error.kind(),
        ErrorKind::Interrupted
            | ErrorKind::WouldBlock
            | ErrorKind::TimedOut
            | ErrorKind::AddrNotAvailable
            | ErrorKind::ConnectionRefused
            | ErrorKind::ConnectionReset
            | ErrorKind::ConnectionAborted
            | ErrorKind::NotConnected
    ) || error.raw_os_error().is_some_and(|code| {
        code == libc::EAGAIN
            || code == libc::EINTR
            || code == libc::ENETDOWN
            || code == libc::ENETUNREACH
            || code == libc::EHOSTUNREACH
            || code == libc::ECONNREFUSED
            || code == libc::ECONNRESET
            || code == libc::ETIMEDOUT
            || code == libc::EADDRNOTAVAIL
            || code == libc::ENOBUFS
    })
}

fn io_failure(message: &'static str, error: &std::io::Error) -> AppError {
    if temporary_io_error(error) {
        AppError::temporary(message)
    } else {
        AppError::new(message)
    }
}

pub fn connect_udp(host: &str, port: u16) -> Result<UdpSocket> {
    if port == 0 {
        return Err(AppError::new("invalid Nanoleaf UDP port"));
    }
    let host = validate_host(host)?;
    let addresses = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|error| io_failure("cannot resolve Nanoleaf UDP target", &error))?;
    let mut last_error = None;
    for address in addresses {
        match UdpSocket::bind(if address.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        })
        .and_then(|socket| socket.connect(address).map(|()| socket))
        {
            Ok(socket) => return Ok(socket),
            Err(error) => last_error = Some(error),
        }
    }
    match last_error {
        Some(error) => Err(io_failure("cannot open Nanoleaf UDP socket", &error)),
        None => Err(AppError::new("cannot resolve Nanoleaf UDP target")),
    }
}

pub fn encode_packet(zones: &[Zone], colors: &[Color], packet: &mut [u8; 322]) -> Result<usize> {
    if zones.len() != colors.len() || zones.len() > MAX_PANELS {
        return Err(AppError::new("invalid Nanoleaf frame dimensions"));
    }
    packet[0] = 0;
    packet[1] = zones.len() as u8;
    for (index, (zone, color)) in zones.iter().zip(colors).enumerate() {
        if !color.r.is_finite() || !color.g.is_finite() || !color.b.is_finite() {
            return Err(AppError::new("invalid Nanoleaf frame color"));
        }
        let offset = 2 + index * 8;
        packet[offset] = (zone.id >> 8) as u8;
        packet[offset + 1] = zone.id as u8;
        packet[offset + 2] = color.r.round().clamp(0.0, 255.0) as u8;
        packet[offset + 3] = color.g.round().clamp(0.0, 255.0) as u8;
        packet[offset + 4] = color.b.round().clamp(0.0, 255.0) as u8;
        packet[offset + 5..offset + 8].fill(0);
    }
    Ok(2 + zones.len() * 8)
}

pub fn send_packet(socket: &UdpSocket, packet: &[u8]) -> Result<()> {
    if packet.len() < 2
        || packet[0] != 0
        || packet[1] as usize > MAX_PANELS
        || packet.len() != 2 + packet[1] as usize * 8
    {
        return Err(AppError::new("invalid Nanoleaf UDP frame"));
    }
    match socket.send(packet) {
        Ok(sent) if sent == packet.len() => Ok(()),
        Ok(_) => Err(AppError::new("Nanoleaf UDP send returned a short frame")),
        Err(error) => Err(io_failure("Nanoleaf UDP send failed", &error)),
    }
}

fn expect(condition: bool, message: &'static str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(AppError::new(message))
    }
}

pub fn self_test() -> Result<()> {
    expect(
        validate_host("192.168.1.10").is_ok(),
        "device self-test: IPv4 host",
    )?;
    expect(
        validate_host("nanoleaf.local").is_ok(),
        "device self-test: DNS host",
    )?;
    expect(validate_host("::1").is_ok(), "device self-test: IPv6 host")?;
    expect(
        validate_host("host/path").is_err(),
        "device self-test: reject URL path",
    )?;
    expect(validate_token("a1b2-c3").is_ok(), "device self-test: token")?;
    expect(
        EXT_CONTROL_PAYLOAD
            == br#"{"write":{"command":"display","animType":"extControl","extControlVersion":"v2"}}"#,
        "device self-test: extControl v2 payload",
    )?;
    let cfg = Config {
        host: "::1".to_owned(),
        token: "saved-token".to_owned(),
        ..Config::default()
    };
    expect(
        POWER_OFF_PAYLOAD == br#"{"on":{"value":false}}"#
            && api_url(&cfg, "/state", Some(&cfg.token))
                .is_ok_and(|url| url.ends_with("/api/v1/saved-token/state")),
        "device self-test: documented power-off request",
    )?;
    expect(
        parse_selected_effect(b"\"Favorite scene\"").as_deref() == Some("Favorite scene")
            && parse_selected_effect(b"Favorite scene").as_deref() == Some("Favorite scene")
            && parse_selected_effect(b"\"*Static*\"").is_none()
            && parse_selected_effect(b"\"extControl\"").is_none()
            && parse_selected_effect(b"\"*ExtControl*\"").is_none()
            && parse_selected_effect(br#"{"select":"*ExtControl*"}"#).is_none()
            && parse_selected_effect(b"").is_none()
            && parse_selected_effect(b"{}").is_none(),
        "device self-test: restore only a saved selected effect",
    )?;
    expect(
        effect_selection_payload("Favorite scene")? == br#"{"select":"Favorite scene"}"#,
        "device self-test: selected-effect restore payload",
    )?;
    expect(
        api_url(&cfg, "/new", None).is_ok_and(|url| url == "http://[::1]:16021/api/v1/new"),
        "device self-test: unauthenticated pairing URL",
    )?;
    expect(
        api_url(&cfg, "/panelLayout/layout", Some(&cfg.token))
            .is_ok_and(|url| url.ends_with("/api/v1/saved-token/panelLayout/layout")),
        "device self-test: authenticated API URL",
    )?;
    let bad_token_error = api_url(&cfg, "/new", Some("secret/path")).unwrap_err();
    let bad_host = validate_host("https://leaf.invalid/secret?token=credential").unwrap_err();
    expect(
        !bad_token_error.message.contains("secret/path")
            && !bad_host
                .message
                .contains("https://leaf.invalid/secret?token=credential"),
        "device self-test: errors omit token and URL values",
    )?;
    expect(
        validate_token("secret/path").is_err(),
        "device self-test: reject token path",
    )?;

    let info = parse_device_info(
        br#"{"name":"4D","model":"NL","serialNo":"123","firmwareVersion":1,"private":"hidden","array":[],"nested":{"secret":true}}"#,
    )?;
    expect(
        info == serde_json::json!({
            "name":"4D", "model":"NL", "serialNo":"123", "firmwareVersion":1
        }),
        "device self-test: device info allowlist",
    )?;
    expect(
        parse_device_info(br#"{"serialNo":{"token":"hidden"}}"#)
            .map(|safe| safe.as_object().is_some_and(|fields| fields.is_empty()))
            .unwrap_or(false),
        "device self-test: device info nested values",
    )?;
    expect(
        parse_device_info(b"[]").is_err(),
        "device self-test: malformed device info",
    )?;
    expect(
        parse_device_info(br#"{"token":"not returned"}"#)
            .map(|safe| safe.as_object().is_some_and(|fields| fields.is_empty()))
            .unwrap_or(false),
        "device self-test: device info secrets",
    )?;

    let four = br#"{"numPanels":4,"positionData":[{"panelId":1,"x":-2,"y":3},{"panelId":2,"x":4,"y":5},{"panelId":3,"x":6,"y":7},{"panelId":65535,"x":8,"y":9}]}"#;
    let panels = parse_panel_layout(four)?;
    expect(
        panels.len() == 4 && panels[0].x == -2 && panels[3].id == u16::MAX,
        "device self-test: four-panel layout",
    )?;
    let one = parse_panel_layout(br#"{"numPanels":1,"positionData":[{"panelId":65535,"x":-2,"y":3}]}"#)?;
    expect(
        one.len() == MIN_PANELS && one[0].id == u16::MAX,
        "device self-test: minimum layout boundary",
    )?;
    let max_layout_body = serde_json::to_vec(&serde_json::json!({
        "numPanels": MAX_PANELS,
        "positionData": (0..MAX_PANELS)
            .map(|id| serde_json::json!({"panelId": id, "x": id, "y": id}))
            .collect::<Vec<_>>(),
    }))
    .map_err(|_| AppError::new("device self-test: encode maximum layout"))?;
    expect(
        parse_panel_layout(&max_layout_body)?.len() == MAX_PANELS
            && parse_panel_layout(br#"{"numPanels":41,"positionData":[]}"#).is_err(),
        "device self-test: maximum layout boundary",
    )?;
    expect(
        parse_panel_layout(br#"{"numPanels":3,"positionData":[]}"#).is_err(),
        "device self-test: reject short layout",
    )?;
    expect(
        parse_panel_layout(br#"{"numPanels":4,"positionData":[{"panelId":1,"x":0,"y":0},{"panelId":1,"x":1,"y":0},{"panelId":2,"x":2,"y":0},{"panelId":3,"x":3,"y":0}]}"#).is_err(),
        "device self-test: reject duplicate panel IDs",
    )?;

    expect(
        parse_stream_port(b"", 60222)? == 60222,
        "device self-test: empty stream response uses configured port",
    )?;
    expect(
        parse_stream_port(br#"{"streamControlPort":65535}"#, 60222)? == 65535,
        "device self-test: maximum stream port",
    )?;
    expect(
        parse_stream_port(br#"{"streamControlPort":0}"#, 60222).is_err()
            && parse_stream_port(br#"{"streamControlPort":65536}"#, 60222).is_err()
            && parse_stream_port(br#"{"streamControlPort":"60222"}"#, 60222).is_err()
            && parse_stream_port(br#"[]"#, 60222).is_err(),
        "device self-test: reject invalid stream response",
    )?;
    expect(
        http_status_failure(503).temporary && !http_status_failure(400).temporary,
        "device self-test: HTTP retry semantics",
    )?;
    expect(
        temporary_io_error(&std::io::Error::from_raw_os_error(libc::ETIMEDOUT))
            && !temporary_io_error(&std::io::Error::from_raw_os_error(libc::EINVAL)),
        "device self-test: UDP temporary versus permanent errors",
    )?;

    let mut packet = [0xff; 322];
    let zones = [
        Zone {
            id: 0x1234,
            edge: None,
            x0: 0.0,
            x1: 1.0,
            y0: 0.0,
            y1: 1.0,
        },
        Zone {
            id: 0xffff,
            edge: None,
            x0: 0.0,
            x1: 1.0,
            y0: 0.0,
            y1: 1.0,
        },
        Zone {
            id: 3,
            edge: None,
            x0: 0.0,
            x1: 1.0,
            y0: 0.0,
            y1: 1.0,
        },
        Zone {
            id: 4,
            edge: None,
            x0: 0.0,
            x1: 1.0,
            y0: 0.0,
            y1: 1.0,
        },
    ];
    let colors = [
        Color {
            r: 1.5,
            g: 2.5,
            b: -0.5,
        },
        Color {
            r: 254.6,
            g: 255.0,
            b: 99.0,
        },
        Color::default(),
        Color::default(),
    ];
    let len = encode_packet(&zones, &colors, &mut packet)?;
    expect(
        len == 34
            && packet[0] == 0
            && packet[1] == 4
            && packet[2..7] == [0x12, 0x34, 2, 3, 0]
            && packet[7..10] == [0, 0, 0]
            && packet[10..15] == [0xff, 0xff, 255, 255, 99]
            && packet[15..18] == [0, 0, 0],
        "device self-test: packet IDs and rounded colors",
    )?;
    expect(
        encode_packet(&zones[..3], &colors[..4], &mut packet).is_err()
            && encode_packet(&zones, &colors[..3], &mut packet).is_err()
            && encode_packet(
                &vec![zones[0].clone(); MAX_PANELS + 1],
                &vec![Color::default(); MAX_PANELS + 1],
                &mut packet,
            )
            .is_err(),
        "device self-test: reject oversized frame and mismatched dimensions",
    )?;
    expect(
        encode_packet(&[], &[], &mut packet)? == 2 && packet[0] == 0 && packet[1] == 0,
        "device self-test: empty frame boundary",
    )?;
    let max_zones: Vec<_> = (0..MAX_PANELS)
        .map(|id| Zone {
            id: id as u16,
            edge: None,
            x0: 0.0,
            x1: 1.0,
            y0: 0.0,
            y1: 1.0,
        })
        .collect();
    let max_colors = vec![Color::default(); MAX_PANELS];
    expect(
        encode_packet(&max_zones, &max_colors, &mut packet)? == packet.len()
            && packet[1] == MAX_PANELS as u8,
        "device self-test: maximum frame boundary",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_test_covers_device_contract() {
        self_test().unwrap();
    }

    #[test]
    fn packet_boundaries_and_invalid_colors() {
        let zone = Zone {
            id: 1,
            edge: None,
            x0: 0.0,
            x1: 1.0,
            y0: 0.0,
            y1: 1.0,
        };
        let colors = [Color::default(); 4];
        let zones: [Zone; 4] = std::array::from_fn(|_| zone.clone());
        let mut packet = [0; 322];
        assert_eq!(encode_packet(&zones, &colors, &mut packet).unwrap(), 34);
        assert_eq!(encode_packet(&[], &[], &mut packet).unwrap(), 2);
        assert!(encode_packet(
            &vec![zone.clone(); 41],
            &[Color::default(); 41],
            &mut packet
        )
        .is_err());
        assert!(encode_packet(&zones, &colors[..3], &mut packet).is_err());
        let bad_colors = [Color {
            r: f32::NAN,
            g: 0.0,
            b: 0.0,
        }; 4];
        assert!(encode_packet(&zones, &bad_colors, &mut packet).is_err());
    }

    #[test]
    fn send_packet_delivers_exact_loopback_frame() {
        let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let address = receiver.local_addr().unwrap();
        let sender = connect_udp("127.0.0.1", address.port()).unwrap();
        let zones: [Zone; 4] = std::array::from_fn(|index| Zone {
            id: 0x1234 + index as u16,
            edge: None,
            x0: 0.0,
            x1: 1.0,
            y0: 0.0,
            y1: 1.0,
        });
        let colors = [Color {
            r: 1.5,
            g: 2.5,
            b: 3.5,
        }; 4];
        let mut frame = [0; 322];
        let len = encode_packet(&zones, &colors, &mut frame).unwrap();
        send_packet(&sender, &frame[..len]).unwrap();
        let mut received = [0; 322];
        let received_len = receiver.recv(&mut received).unwrap();
        assert_eq!(received_len, len);
        assert_eq!(&received[..received_len], &frame[..len]);
    }

    #[test]
    fn allowlist_and_invalid_stream_port_are_strict() {
        let safe = parse_device_info(br#"{"name":"A","token":"secret","extra":1}"#).unwrap();
        assert_eq!(safe, serde_json::json!({"name":"A"}));
        assert!(parse_stream_port(br#"{"streamControlPort":0}"#, 60222).is_err());
        assert!(parse_stream_port(br#"{"streamControlPort":65536}"#, 60222).is_err());
        assert_eq!(parse_stream_port(b"", 60222).unwrap(), 60222);
    }
}
