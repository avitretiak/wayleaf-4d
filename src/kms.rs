use crate::{AppError, Result};
use serde::Serialize;
use std::ffi::CStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::mem::size_of;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::ptr::NonNull;
use std::slice;
use std::thread;
use std::time::{Duration, Instant};

const PROTOCOL_VERSION: u32 = 6;
const RESULT_OK: i32 = 0;
const RESULT_MAX: i32 = 4;
const REQUEST_GET_KMS: i32 = 1;
const HDR_STATIC_METADATA_TYPE1: u32 = 0;
const HDR_EOTF_SMPTE_ST2084: u8 = 2;
const MAX_ITEMS: usize = 8;
const MAX_DMA_BUFS: usize = 4;
const MAX_FDS: usize = MAX_ITEMS * MAX_DMA_BUFS;
const HELPER: &str = "/usr/bin/gsr-kms-server";
const DRM_MODE_CONNECTED: i32 = 1;
const DRM_MODE_OBJECT_CONNECTOR: u32 = 0xc0c0_c0c0;

#[derive(Clone, Serialize)]
pub struct Monitor {
    pub name: String,
    pub card_path: String,
    pub connector_id: u32,
    pub width: u32,
    pub height: u32,
}

pub struct DmaBuf {
    pub fd: OwnedFd,
    pub pitch: u32,
    pub offset: u32,
}

pub struct Plane {
    pub buffers: Vec<DmaBuf>,
    pub width: u32,
    pub height: u32,
    pub pixel_format: u32,
    pub modifier: u64,
    pub connector_id: u32,
    pub plane_type: u32,
    pub rotation: u32,
    pub src_x: i32,
    pub src_y: i32,
    pub src_w: i32,
    pub src_h: i32,
    pub dst_x: i32,
    pub dst_y: i32,
    pub dst_w: i32,
    pub dst_h: i32,
    pub zpos: i32,
    pub hdr: Option<HdrMetadata>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HdrMetadata {
    pub metadata_type: u32,
    pub infoframe_metadata_type: u8,
    pub eotf: u8,
    pub max_luminance: u16,
    pub min_luminance: u16,
    pub max_cll: u16,
    pub max_fall: u16,
}

pub struct KmsClient {
    monitor: Monitor,
    stream: UnixStream,
    child: HelperChild,
    _private_dir: PrivateDir,
    // GSR 6.1.4 retries connect() until EISCONN after the first accepted connection.
    listener: Option<UnixListener>,
    planes: Vec<Plane>,
    active_planes: usize,
    control: Vec<usize>,
    poisoned: bool,
}

impl KmsClient {
    pub fn new(monitor: &Monitor) -> Result<Self> {
        validate_monitor(monitor)?;
        let current = monitors()?;
        if !current.iter().any(|m| {
            m.card_path == monitor.card_path
                && m.connector_id == monitor.connector_id
                && m.width == monitor.width
                && m.height == monitor.height
        }) {
            return Err(AppError::temporary(format!(
                "monitor {} on {} is no longer active; refresh the monitor selection",
                monitor.name, monitor.card_path
            )));
        }

        let private_dir = PrivateDir::create()?;
        let socket_path = private_dir.0.join("s");
        let listener = UnixListener::bind(&socket_path)
            .map_err(|e| AppError::temporary(format!("cannot create private KMS socket: {e}")))?;
        fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))
            .map_err(|e| AppError::temporary(format!("cannot secure private KMS socket: {e}")))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| AppError::temporary(format!("cannot configure KMS socket: {e}")))?;

        let helper_meta = fs::metadata(HELPER).map_err(|e| {
            AppError::new(format!(
                "KMS helper {HELPER} is unavailable ({e}); install GPU Screen Recorder 6.1.4"
            ))
        })?;
        if !helper_meta.is_file() {
            return Err(AppError::new(format!(
                "KMS helper {HELPER} is not a regular file; reinstall GPU Screen Recorder 6.1.4"
            )));
        }
        let mut child = HelperChild::spawn(&socket_path, &monitor.card_path)?;
        let stream = accept_helper(&listener, &mut child)?;
        verify_helper_peer(&stream, child.id())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .map_err(|e| AppError::temporary(format!("cannot set KMS read timeout: {e}")))?;
        stream
            .set_write_timeout(Some(Duration::from_secs(3)))
            .map_err(|e| AppError::temporary(format!("cannot set KMS write timeout: {e}")))?;

        let planes = (0..MAX_ITEMS).map(|_| empty_plane()).collect();
        let control_bytes = cmsg_space(MAX_FDS * size_of::<RawFd>());
        let control = vec![0usize; control_bytes.div_ceil(size_of::<usize>())];
        Ok(Self {
            monitor: monitor.clone(),
            stream,
            child,
            _private_dir: private_dir,
            listener: Some(listener),
            planes,
            active_planes: 0,
            control,
            poisoned: false,
        })
    }

    pub fn capture(&mut self) -> Result<&[Plane]> {
        self.active_planes = 0;
        for plane in &mut self.planes {
            plane.buffers.clear();
            plane.hdr = None;
        }
        if self.poisoned {
            return Err(AppError::temporary(
                "KMS helper connection is unusable; recreate the capture client",
            ));
        }

        let request = WireRequest {
            version: PROTOCOL_VERSION,
            kind: REQUEST_GET_KMS,
            new_connection_fd: 0,
        };
        let request_bytes = unsafe {
            slice::from_raw_parts(
                (&request as *const WireRequest).cast::<u8>(),
                size_of::<WireRequest>(),
            )
        };
        if let Err(e) = self.stream.write_all(request_bytes) {
            self.poisoned = true;
            return Err(AppError::temporary(format!(
                "failed to request KMS planes: {e}"
            )));
        }

        let response_result = self.receive_response();
        drop(self.listener.take());
        let (response, mut received_fds) = match response_result {
            Ok(response) => response,
            Err(error) => {
                self.poisoned = true;
                return Err(error);
            }
        };
        let fd_count = match validate_response(&response) {
            Ok(count) => count,
            Err(error) => {
                self.poisoned = true;
                return Err(error);
            }
        };
        if let Err(error) = validate_fd_count(fd_count, received_fds.count) {
            self.poisoned = true;
            return Err(error);
        }
        if response.result != RESULT_OK {
            return Err(server_result_error(&response));
        }
        received_fds.validate_cloexec()?;

        let mut fd_index = 0;
        for item_index in 0..response.num_items as usize {
            let item = &response.items[item_index];
            let nbufs = item.num_dma_bufs as usize;
            if item.connector_id == self.monitor.connector_id {
                let plane = &mut self.planes[self.active_planes];
                plane.width = item.width;
                plane.height = item.height;
                plane.pixel_format = item.pixel_format;
                plane.modifier = item.modifier;
                plane.connector_id = item.connector_id;
                plane.plane_type = item.plane_type as u32;
                plane.rotation = item.rotation as u32;
                plane.src_x = item.src_x;
                plane.src_y = item.src_y;
                plane.src_w = item.src_w;
                plane.src_h = item.src_h;
                plane.dst_x = item.dst_x;
                plane.dst_y = item.dst_y;
                plane.dst_w = item.dst_w;
                plane.dst_h = item.dst_h;
                plane.zpos = item.zpos;
                plane.hdr = if item.has_hdr_metadata == 1 {
                    hdr_metadata(&item.hdr_metadata)
                } else {
                    None
                };
                for buffer_index in 0..nbufs {
                    let fd = received_fds.take(fd_index + buffer_index);
                    let descriptor = &item.dma_bufs[buffer_index];
                    plane.buffers.push(DmaBuf {
                        fd,
                        pitch: descriptor.pitch,
                        offset: descriptor.offset,
                    });
                }
                self.active_planes += 1;
            }
            fd_index += nbufs;
        }

        if self.active_planes == 0 {
            return Err(AppError::temporary(format!(
                "KMS helper returned no scanout planes for monitor {} (connector {})",
                self.monitor.name, self.monitor.connector_id
            )));
        }
        Ok(&self.planes[..self.active_planes])
    }

    fn receive_response(&mut self) -> Result<(WireResponse, ReceivedFds)> {
        let mut bytes = [0u8; size_of::<WireResponse>()];
        let mut received_fds = ReceivedFds::new();
        let mut iov = libc::iovec {
            iov_base: bytes.as_mut_ptr().cast(),
            iov_len: bytes.len(),
        };
        let mut message: libc::msghdr = unsafe { std::mem::zeroed() };
        message.msg_iov = &mut iov;
        message.msg_iovlen = 1;
        message.msg_control = self.control.as_mut_ptr().cast();
        message.msg_controllen = self.control.len() * size_of::<usize>();
        let nread = unsafe {
            libc::recvmsg(
                self.stream.as_raw_fd(),
                &mut message,
                libc::MSG_CMSG_CLOEXEC,
            )
        };
        if nread < 0 {
            return Err(AppError::temporary(format!(
                "failed to receive KMS response: {}",
                io::Error::last_os_error()
            )));
        }
        parse_control_messages(&message, &mut received_fds);
        if nread == 0 {
            return Err(AppError::temporary("KMS helper closed the connection"));
        }
        if message.msg_flags & (libc::MSG_CTRUNC | libc::MSG_TRUNC) != 0 {
            return Err(AppError::new(
                "KMS protocol error: truncated response or SCM_RIGHTS descriptor list",
            ));
        }
        if received_fds.invalid {
            return Err(AppError::new(
                "KMS protocol error: malformed or unexpected ancillary data",
            ));
        }

        let mut filled = nread as usize;
        if filled < size_of::<u32>() {
            self.stream
                .read_exact(&mut bytes[filled..size_of::<u32>()])
                .map_err(|e| AppError::temporary(format!("incomplete KMS response: {e}")))?;
            filled = size_of::<u32>();
        }
        let version = u32::from_ne_bytes(bytes[..size_of::<u32>()].try_into().unwrap());
        if version != PROTOCOL_VERSION {
            return Err(AppError::new(format!(
                "KMS protocol version mismatch: helper uses {version}, client requires {PROTOCOL_VERSION}; reinstall GPU Screen Recorder 6.1.4"
            )));
        }
        if filled < bytes.len() {
            self.stream
                .read_exact(&mut bytes[filled..])
                .map_err(|e| AppError::temporary(format!("incomplete KMS response: {e}")))?;
        }
        let response = unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast::<WireResponse>()) };
        Ok((response, received_fds))
    }
}

impl Drop for KmsClient {
    fn drop(&mut self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
        self.child.wait_after_disconnect();
    }
}

pub fn monitors() -> Result<Vec<Monitor>> {
    let entries = fs::read_dir("/sys/class/drm")
        .map_err(|e| AppError::temporary(format!("cannot enumerate DRM devices: {e}")))?;
    let mut cards = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|e| AppError::temporary(format!("cannot enumerate DRM devices: {e}")))?;
        let Some(file_name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if file_name
            .strip_prefix("card")
            .is_some_and(|suffix| !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()))
        {
            cards.push(PathBuf::from(format!("/dev/dri/{file_name}")));
        }
    }
    cards.sort();

    let mut active = Vec::new();
    for card_path in cards {
        active.extend(enumerate_card(&card_path)?);
    }
    active.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then_with(|| a.card_path.cmp(&b.card_path))
            .then_with(|| a.connector_id.cmp(&b.connector_id))
    });
    Ok(active)
}

pub fn select_monitor(name: &str) -> Result<Monitor> {
    let available = monitors()?;
    select_from(name, &available)
}

pub fn render_node_for_card(card_path: &str) -> Result<PathBuf> {
    let card_name = Path::new(card_path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| {
            name.strip_prefix("card").is_some_and(|suffix| {
                !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit())
            })
        })
        .ok_or_else(|| AppError::new(format!("invalid DRM card path: {card_path}")))?;
    let card_device = fs::canonicalize(Path::new("/sys/class/drm").join(card_name).join("device"))
        .map_err(|e| AppError::temporary(format!("cannot resolve DRM device {card_path}: {e}")))?;
    let entries = fs::read_dir("/sys/class/drm")
        .map_err(|e| AppError::temporary(format!("cannot enumerate DRM render nodes: {e}")))?;
    let mut render_nodes = Vec::new();
    for entry in entries {
        let entry = entry
            .map_err(|e| AppError::temporary(format!("cannot enumerate DRM render nodes: {e}")))?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !name
            .strip_prefix("renderD")
            .is_some_and(|suffix| !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()))
        {
            continue;
        }
        if fs::canonicalize(entry.path().join("device")).is_ok_and(|path| path == card_device) {
            render_nodes.push(PathBuf::from(format!("/dev/dri/{name}")));
        }
    }
    render_nodes.sort();
    match render_nodes.len() {
        0 => Err(AppError::new(format!(
            "no DRM render node is available for {card_path}; direct KMS composition requires a render node on the same GPU"
        ))),
        1 => Ok(render_nodes.remove(0)),
        _ => Err(AppError::new(format!(
            "multiple DRM render nodes match {card_path}; refusing ambiguous GPU selection"
        ))),
    }
}

pub fn self_test() -> Result<()> {
    if size_of::<WireRequest>() != 12
        || size_of::<WireDmaBuf>() != 12
        || size_of::<HdrInfoframe>() != 26
        || size_of::<HdrOutputMetadata>() != 32
        || size_of::<WireItem>() != 160
        || size_of::<WireResponse>() != 1424
        || std::mem::offset_of!(WireResponse, items) != 136
        || std::mem::offset_of!(WireResponse, num_items) != 1416
        || std::mem::offset_of!(WireItem, has_hdr_metadata) != 80
        || std::mem::offset_of!(WireItem, rotation) != 84
        || std::mem::offset_of!(WireItem, hdr_metadata) != 124
    {
        return Err(AppError::new(
            "KMS protocol 6 wire layout does not match the installed helper ABI",
        ));
    }

    let mut response = empty_wire_response();
    response.version = PROTOCOL_VERSION;
    response.result = RESULT_OK;
    response.num_items = 1;
    response.items[0].connector_id = 7;
    response.items[0].num_dma_bufs = 1;
    response.items[0].width = 1920;
    response.items[0].height = 1080;
    response.items[0].plane_type = 1;
    response.items[0].rotation = 0;
    response.items[0].src_w = 1920;
    response.items[0].src_h = 1080;
    response.items[0].dst_w = 1920;
    response.items[0].dst_h = 1080;
    response.items[0].has_hdr_metadata = 1;
    response.items[0].hdr_metadata.metadata_type = HDR_STATIC_METADATA_TYPE1;
    response.items[0]
        .hdr_metadata
        .hdmi_metadata_type1
        .metadata_type = HDR_STATIC_METADATA_TYPE1 as u8;
    response.items[0].hdr_metadata.hdmi_metadata_type1.eotf = HDR_EOTF_SMPTE_ST2084;
    if validate_response(&response)? != 1
        || validate_fd_count(1, 1).is_err()
        || hdr_metadata(&response.items[0].hdr_metadata).is_none()
    {
        return Err(AppError::new(
            "KMS protocol self-check rejected valid HDR metadata",
        ));
    }
    response.items[0].hdr_metadata.metadata_type = 1;
    if hdr_metadata(&response.items[0].hdr_metadata).is_some() {
        return Err(AppError::new(
            "KMS protocol self-check exposed unsupported outer HDR metadata",
        ));
    }
    response.items[0].hdr_metadata.metadata_type = HDR_STATIC_METADATA_TYPE1;
    response.items[0]
        .hdr_metadata
        .hdmi_metadata_type1
        .metadata_type = 1;
    if hdr_metadata(&response.items[0].hdr_metadata).is_some() {
        return Err(AppError::new(
            "KMS protocol self-check exposed unsupported infoframe metadata",
        ));
    }
    response.items[0]
        .hdr_metadata
        .hdmi_metadata_type1
        .metadata_type = HDR_STATIC_METADATA_TYPE1 as u8;
    response.items[0].hdr_metadata.hdmi_metadata_type1.eotf = 1;
    if hdr_metadata(&response.items[0].hdr_metadata).is_some() {
        return Err(AppError::new(
            "KMS protocol self-check exposed unsupported HDR EOTF",
        ));
    }
    response.items[0].hdr_metadata.hdmi_metadata_type1.eotf = HDR_EOTF_SMPTE_ST2084;
    if validate_fd_count(1, 0).is_ok() {
        return Err(AppError::new(
            "KMS protocol self-check accepted a mismatched descriptor count",
        ));
    }
    response.items[0].num_dma_bufs = (MAX_DMA_BUFS + 1) as i32;
    if validate_response(&response).is_ok() {
        return Err(AppError::new(
            "KMS protocol self-check accepted an oversized DMA-BUF count",
        ));
    }

    let monitor = Monitor {
        name: "DP-1".to_string(),
        card_path: "/dev/dri/card0".to_string(),
        connector_id: 7,
        width: 1920,
        height: 1080,
    };
    if select_from("", std::slice::from_ref(&monitor))?.connector_id != monitor.connector_id
        || select_from("DP-1", std::slice::from_ref(&monitor))?.connector_id != monitor.connector_id
        || select_from("", &[]).is_ok()
        || select_from("missing", std::slice::from_ref(&monitor)).is_ok()
        || select_from("DP-1", &[monitor.clone(), monitor]).is_ok()
    {
        return Err(AppError::new("KMS monitor selection self-check failed"));
    }
    Ok(())
}

fn empty_plane() -> Plane {
    Plane {
        buffers: Vec::with_capacity(MAX_DMA_BUFS),
        width: 0,
        height: 0,
        pixel_format: 0,
        modifier: 0,
        connector_id: 0,
        plane_type: 0,
        rotation: 0,
        src_x: 0,
        src_y: 0,
        src_w: 0,
        src_h: 0,
        dst_x: 0,
        dst_y: 0,
        dst_w: 0,
        dst_h: 0,
        zpos: 0,
        hdr: None,
    }
}

fn validate_monitor(monitor: &Monitor) -> Result<()> {
    let valid_card = Path::new(&monitor.card_path)
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix("card"))
        .is_some_and(|suffix| !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()));
    if !monitor.card_path.starts_with("/dev/dri/")
        || !valid_card
        || monitor.connector_id == 0
        || monitor.width == 0
        || monitor.height == 0
    {
        return Err(AppError::new("invalid DRM monitor descriptor"));
    }
    Ok(())
}

fn select_from(name: &str, available: &[Monitor]) -> Result<Monitor> {
    if name.is_empty() {
        return match available {
            [] => Err(AppError::new("no connected active DRM monitors were found")),
            [monitor] => Ok(monitor.clone()),
            _ => Err(AppError::new(format!(
                "multiple active monitors are available; choose one with --monitor NAME: {}",
                available
                    .iter()
                    .map(|m| format!("{} ({}, connector {})", m.name, m.card_path, m.connector_id))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
        };
    }
    let mut matches = available.iter().filter(|monitor| monitor.name == name);
    let Some(first) = matches.next() else {
        return Err(AppError::new(format!(
            "monitor {name:?} is not connected and active; refresh the monitor list"
        )));
    };
    if matches.next().is_some() {
        return Err(AppError::new(format!(
            "monitor name {name:?} is ambiguous across DRM cards; select a uniquely named connector"
        )));
    }
    Ok(first.clone())
}

fn enumerate_card(card_path: &Path) -> Result<Vec<Monitor>> {
    let fd = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC)
        .open(card_path)
        .map_err(|e| {
            AppError::temporary(format!(
                "cannot inspect DRM card {}: {e}",
                card_path.display()
            ))
        })?;
    let resources =
        NonNull::new(unsafe { drmModeGetResources(fd.as_raw_fd()) }).ok_or_else(|| {
            AppError::temporary(format!(
                "cannot query DRM resources on {}: {}",
                card_path.display(),
                io::Error::last_os_error()
            ))
        })?;
    let resources = DrmResources(resources);
    let res = unsafe { resources.0.as_ref() };
    if res.count_connectors < 0 || (res.count_connectors > 0 && res.connectors.is_null()) {
        return Err(AppError::temporary(format!(
            "DRM returned invalid connector resources for {}",
            card_path.display()
        )));
    }

    let card_string = card_path.to_string_lossy().into_owned();
    let mut found = Vec::new();
    for index in 0..res.count_connectors as usize {
        let connector_id = unsafe { *res.connectors.add(index) };
        let Some(connector) =
            NonNull::new(unsafe { drmModeGetConnectorCurrent(fd.as_raw_fd(), connector_id) })
        else {
            return Err(AppError::temporary(format!(
                "DRM connector {connector_id} changed during monitor enumeration on {}",
                card_path.display()
            )));
        };
        let connector = DrmConnector(connector);
        let details = unsafe { connector.0.as_ref() };
        if details.connection != DRM_MODE_CONNECTED {
            continue;
        }
        let crtc_id = connector_crtc(fd.as_raw_fd(), details)?;
        let Some(crtc_id) = crtc_id.filter(|id| *id != 0) else {
            continue;
        };
        let Some(crtc) = NonNull::new(unsafe { drmModeGetCrtc(fd.as_raw_fd(), crtc_id) }) else {
            return Err(AppError::temporary(format!(
                "active DRM connector {connector_id} has no readable CRTC mode on {}",
                card_path.display()
            )));
        };
        let crtc = DrmCrtc(crtc);
        let mode = unsafe { crtc.0.as_ref() };
        if mode.mode_valid == 0 || mode.mode.hdisplay == 0 || mode.mode.vdisplay == 0 {
            continue;
        }
        let type_name = unsafe { drmModeGetConnectorTypeName(details.connector_type) };
        let type_name = if type_name.is_null() {
            "Unknown".to_string()
        } else {
            unsafe { CStr::from_ptr(type_name) }
                .to_string_lossy()
                .into_owned()
        };
        found.push(Monitor {
            name: format!("{type_name}-{}", details.connector_type_id),
            card_path: card_string.clone(),
            connector_id: details.connector_id,
            width: u32::from(mode.mode.hdisplay),
            height: u32::from(mode.mode.vdisplay),
        });
    }
    Ok(found)
}

fn connector_crtc(fd: RawFd, connector: &DrmModeConnector) -> Result<Option<u32>> {
    if let Some(value) = object_property(
        fd,
        connector.connector_id,
        DRM_MODE_OBJECT_CONNECTOR,
        "CRTC_ID",
    ) {
        let id = u32::try_from(value).map_err(|_| {
            AppError::temporary(format!(
                "invalid CRTC_ID on DRM connector {}",
                connector.connector_id
            ))
        })?;
        if id != 0 {
            return Ok(Some(id));
        }
    }
    if connector.encoder_id == 0 {
        return Ok(None);
    }
    let encoder =
        NonNull::new(unsafe { drmModeGetEncoder(fd, connector.encoder_id) }).ok_or_else(|| {
            AppError::temporary(format!(
                "cannot read active encoder on DRM connector {}",
                connector.connector_id
            ))
        })?;
    let encoder = DrmEncoder(encoder);
    let crtc_id = unsafe { encoder.0.as_ref() }.crtc_id;
    Ok((crtc_id != 0).then_some(crtc_id))
}

fn object_property(fd: RawFd, object_id: u32, object_type: u32, wanted: &str) -> Option<u64> {
    let props = NonNull::new(unsafe { drmModeObjectGetProperties(fd, object_id, object_type) })?;
    let props = DrmObjectProperties(props);
    let details = unsafe { props.0.as_ref() };
    if details.count_props > 0 && (details.props.is_null() || details.prop_values.is_null()) {
        return None;
    }
    for index in 0..details.count_props as usize {
        let property_id = unsafe { *details.props.add(index) };
        let Some(property) = NonNull::new(unsafe { drmModeGetProperty(fd, property_id) }) else {
            continue;
        };
        let property = DrmProperty(property);
        let property_name = unsafe { CStr::from_ptr(property.0.as_ref().name.as_ptr()) };
        if property_name.to_bytes() == wanted.as_bytes() {
            return Some(unsafe { *details.prop_values.add(index) });
        }
    }
    None
}

fn parse_control_messages(message: &libc::msghdr, received: &mut ReceivedFds) {
    let control = message.msg_control.cast::<u8>();
    let total = message.msg_controllen;
    let header_size = size_of::<libc::cmsghdr>();
    let data_offset = cmsg_align(header_size);
    let mut offset = 0usize;
    let mut rights_messages = 0usize;
    while offset + header_size <= total {
        let header =
            unsafe { std::ptr::read_unaligned(control.add(offset).cast::<libc::cmsghdr>()) };
        let remaining = total - offset;
        let declared = header.cmsg_len;
        if header.cmsg_level == libc::SOL_SOCKET && header.cmsg_type == libc::SCM_RIGHTS {
            rights_messages += 1;
            if declared < data_offset {
                received.invalid = true;
                break;
            }
            let payload_bytes = declared
                .saturating_sub(data_offset)
                .min(remaining.saturating_sub(data_offset));
            if payload_bytes == 0 || payload_bytes % size_of::<RawFd>() != 0 || declared > remaining
            {
                received.invalid = true;
            }
            for index in 0..payload_bytes / size_of::<RawFd>() {
                let raw = unsafe {
                    std::ptr::read_unaligned(
                        control
                            .add(offset + data_offset + index * size_of::<RawFd>())
                            .cast::<RawFd>(),
                    )
                };
                received.push(raw);
            }
        } else {
            received.invalid = true;
        }
        if declared < data_offset || declared > remaining {
            received.invalid = true;
            break;
        }
        let step = cmsg_align(declared);
        if step == 0 || step > remaining {
            if declared != remaining {
                received.invalid = true;
            }
            break;
        }
        offset += step;
    }
    if offset < total && total - offset >= header_size {
        received.invalid = true;
    }
    if rights_messages > 1 {
        received.invalid = true;
    }
}

fn validate_fd_count(expected: usize, actual: usize) -> Result<()> {
    if expected != actual {
        return Err(AppError::new(format!(
            "KMS protocol error: response declares {expected} DMA-BUFs but transferred {actual} descriptors"
        )));
    }
    Ok(())
}

fn validate_response(response: &WireResponse) -> Result<usize> {
    if !(0..=RESULT_MAX).contains(&response.result) {
        return Err(AppError::new(format!(
            "KMS protocol error: invalid result code {}",
            response.result
        )));
    }
    if response.version != PROTOCOL_VERSION {
        return Err(AppError::new(format!(
            "KMS protocol version mismatch: helper uses {}, client requires {PROTOCOL_VERSION}",
            response.version
        )));
    }
    if !(0..=MAX_ITEMS as i32).contains(&response.num_items) {
        return Err(AppError::new(format!(
            "KMS protocol error: invalid plane count {}",
            response.num_items
        )));
    }
    let mut fd_count = 0usize;
    for item in response.items.iter().take(response.num_items as usize) {
        if !(1..=MAX_DMA_BUFS as i32).contains(&item.num_dma_bufs) {
            return Err(AppError::new(format!(
                "KMS protocol error: invalid DMA-BUF count {}",
                item.num_dma_bufs
            )));
        }
        if item.width == 0
            || item.height == 0
            || item.rotation < 0
            || item.rotation > 3
            || !(0..=2).contains(&item.plane_type)
            || item.has_hdr_metadata > 1
            || item.src_w <= 0
            || item.src_h <= 0
            || item.dst_w <= 0
            || item.dst_h <= 0
        {
            return Err(AppError::new("KMS protocol error: invalid plane metadata"));
        }
        fd_count += item.num_dma_bufs as usize;
    }
    if response.result == RESULT_OK && response.num_items == 0 {
        return Err(AppError::new(
            "KMS protocol error: helper returned an empty successful response",
        ));
    }
    Ok(fd_count)
}

fn server_result_error(response: &WireResponse) -> AppError {
    let end = response
        .err_msg
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(response.err_msg.len());
    let text = String::from_utf8_lossy(&response.err_msg[..end]);
    let detail = if text.is_empty() {
        format!("helper result {}", response.result)
    } else {
        format!("helper result {}: {text}", response.result)
    };
    if response.result == 1 {
        AppError::new(format!("KMS helper rejected the request: {detail}"))
    } else {
        let lower = text.to_ascii_lowercase();
        let permission_hint = if lower.contains("permission denied")
            || lower.contains("operation not permitted")
        {
            "; ensure /usr/bin/gsr-kms-server retains its installed CAP_SYS_ADMIN permission (no pkexec was invoked)"
        } else {
            ""
        };
        AppError::temporary(format!("KMS capture failed: {detail}{permission_hint}"))
    }
}

fn hdr_metadata(metadata: &HdrOutputMetadata) -> Option<HdrMetadata> {
    let frame = &metadata.hdmi_metadata_type1;
    if metadata.metadata_type != HDR_STATIC_METADATA_TYPE1
        || frame.metadata_type as u32 != HDR_STATIC_METADATA_TYPE1
        || frame.eotf != HDR_EOTF_SMPTE_ST2084
    {
        return None;
    }
    Some(HdrMetadata {
        metadata_type: metadata.metadata_type,
        infoframe_metadata_type: frame.metadata_type,
        eotf: frame.eotf,
        max_luminance: frame.max_display_mastering_luminance,
        min_luminance: frame.min_display_mastering_luminance,
        max_cll: frame.max_cll,
        max_fall: frame.max_fall,
    })
}

fn empty_wire_response() -> WireResponse {
    unsafe { std::mem::zeroed() }
}

fn cmsg_align(len: usize) -> usize {
    let align = size_of::<usize>();
    (len + align - 1) & !(align - 1)
}

fn cmsg_space(payload_len: usize) -> usize {
    cmsg_align(size_of::<libc::cmsghdr>()) + cmsg_align(payload_len)
}

struct ReceivedFds {
    descriptors: [Option<OwnedFd>; MAX_FDS],
    count: usize,
    invalid: bool,
}

impl ReceivedFds {
    fn new() -> Self {
        Self {
            descriptors: std::array::from_fn(|_| None),
            count: 0,
            invalid: false,
        }
    }

    fn push(&mut self, fd: RawFd) {
        if fd < 0 || self.count == MAX_FDS {
            if fd >= 0 {
                unsafe { drop(OwnedFd::from_raw_fd(fd)) };
            }
            self.invalid = true;
            return;
        }
        self.descriptors[self.count] = Some(unsafe { OwnedFd::from_raw_fd(fd) });
        self.count += 1;
    }

    fn validate_cloexec(&mut self) -> Result<()> {
        for descriptor in self.descriptors.iter().take(self.count) {
            let Some(fd) = descriptor.as_ref() else {
                return Err(AppError::new(
                    "KMS protocol error: missing transferred descriptor",
                ));
            };
            let raw = fd.as_raw_fd();
            let flags = unsafe { libc::fcntl(raw, libc::F_GETFD) };
            if flags < 0 {
                return Err(AppError::new(format!(
                    "KMS protocol error: invalid transferred descriptor: {}",
                    io::Error::last_os_error()
                )));
            }
            if flags & libc::FD_CLOEXEC == 0
                && unsafe { libc::fcntl(raw, libc::F_SETFD, flags | libc::FD_CLOEXEC) } < 0
            {
                return Err(AppError::temporary(format!(
                    "cannot secure DMA-BUF descriptor: {}",
                    io::Error::last_os_error()
                )));
            }
        }
        Ok(())
    }

    fn take(&mut self, index: usize) -> OwnedFd {
        self.descriptors[index]
            .take()
            .expect("validated SCM_RIGHTS descriptor index")
    }
}

struct PrivateDir(PathBuf);

impl PrivateDir {
    fn create() -> Result<Self> {
        let uid = unsafe { libc::geteuid() };
        let mut random = [0u8; 16];
        let mut entropy = File::open("/dev/urandom").map_err(|e| {
            AppError::temporary(format!("cannot obtain KMS socket randomness: {e}"))
        })?;
        for _ in 0..8 {
            entropy.read_exact(&mut random).map_err(|e| {
                AppError::temporary(format!("cannot obtain KMS socket randomness: {e}"))
            })?;
            let suffix = random
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            let path = PathBuf::from(format!("/tmp/.wayleaf-kms-{uid}-{suffix}"));
            let mut builder = fs::DirBuilder::new();
            builder.mode(0o700);
            match builder.create(&path) {
                Ok(()) => {
                    let directory = Self(path);
                    fs::set_permissions(&directory.0, fs::Permissions::from_mode(0o700)).map_err(
                        |e| {
                            AppError::temporary(format!("cannot secure private KMS directory: {e}"))
                        },
                    )?;
                    let metadata = fs::symlink_metadata(&directory.0).map_err(|e| {
                        AppError::temporary(format!("cannot inspect private KMS directory: {e}"))
                    })?;
                    if !metadata.file_type().is_dir()
                        || metadata.uid() != uid
                        || metadata.mode() & 0o777 != 0o700
                    {
                        return Err(AppError::new(
                            "private KMS socket directory failed ownership/mode validation",
                        ));
                    }
                    return Ok(directory);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(AppError::temporary(format!(
                        "cannot create private KMS socket directory: {error}"
                    )))
                }
            }
        }
        Err(AppError::temporary(
            "could not allocate a unique private KMS socket directory",
        ))
    }
}

impl Drop for PrivateDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct HelperChild(Child);

impl HelperChild {
    fn spawn(socket_path: &Path, card_path: &str) -> Result<Self> {
        let child = Command::new(HELPER)
            .arg(socket_path)
            .arg(card_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .map_err(|e| {
                if e.kind() == io::ErrorKind::PermissionDenied {
                    AppError::temporary(format!(
                        "cannot execute {HELPER}: {e}; preserve the installed GPU Screen Recorder helper permissions (CAP_SYS_ADMIN); no pkexec was invoked"
                    ))
                } else {
                    AppError::temporary(format!("cannot start KMS helper {HELPER}: {e}"))
                }
            })?;
        Ok(Self(child))
    }

    fn id(&self) -> u32 {
        self.0.id()
    }

    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.0.try_wait()
    }

    fn wait_after_disconnect(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            match self.0.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                _ => break,
            }
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Drop for HelperChild {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn accept_helper(listener: &UnixListener, child: &mut HelperChild) -> Result<UnixStream> {
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        match listener.accept() {
            Ok((stream, _)) => return Ok(stream),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                return Err(AppError::temporary(format!(
                    "cannot accept KMS helper: {error}"
                )))
            }
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|e| AppError::temporary(format!("cannot inspect KMS helper process: {e}")))?
        {
            return Err(helper_exit_error(status));
        }
        if Instant::now() >= deadline {
            return Err(AppError::temporary(format!(
                "timed out waiting for {HELPER} to connect; check DRM card permissions and ensure the installed helper retains its existing CAP_SYS_ADMIN permission (no pkexec was invoked)"
            )));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn helper_exit_error(status: ExitStatus) -> AppError {
    AppError::temporary(format!(
        "KMS helper exited before connecting ({status}); if DRM access was denied, repair the installed GPU Screen Recorder helper permissions/CAP_SYS_ADMIN; no pkexec was invoked"
    ))
}

fn verify_helper_peer(stream: &UnixStream, child_pid: u32) -> Result<()> {
    let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
    let mut length = size_of::<libc::ucred>() as libc::socklen_t;
    let status = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    };
    if status != 0 {
        return Err(AppError::new(format!(
            "cannot verify KMS helper peer credentials: {}",
            io::Error::last_os_error()
        )));
    }
    if length as usize != size_of::<libc::ucred>()
        || credentials.pid < 0
        || credentials.pid as u32 != child_pid
        || credentials.uid != unsafe { libc::geteuid() }
    {
        return Err(AppError::new(
            "rejected unexpected process connected to the private KMS socket",
        ));
    }
    Ok(())
}

#[repr(C)]
#[derive(Clone, Copy)]
struct WireRequest {
    version: u32,
    kind: i32,
    new_connection_fd: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct WireDmaBuf {
    _fd: i32,
    pitch: u32,
    offset: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct HdrInfoframe {
    eotf: u8,
    metadata_type: u8,
    _display_primaries: [[u16; 2]; 3],
    _white_point: [u16; 2],
    max_display_mastering_luminance: u16,
    min_display_mastering_luminance: u16,
    max_cll: u16,
    max_fall: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct HdrOutputMetadata {
    metadata_type: u32,
    hdmi_metadata_type1: HdrInfoframe,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct WireItem {
    dma_bufs: [WireDmaBuf; MAX_DMA_BUFS],
    num_dma_bufs: i32,
    width: u32,
    height: u32,
    pixel_format: u32,
    modifier: u64,
    connector_id: u32,
    plane_type: i32,
    has_hdr_metadata: u8,
    rotation: i32,
    src_x: i32,
    src_y: i32,
    src_w: i32,
    src_h: i32,
    dst_x: i32,
    dst_y: i32,
    dst_w: i32,
    dst_h: i32,
    zpos: i32,
    hdr_metadata: HdrOutputMetadata,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct WireResponse {
    version: u32,
    result: i32,
    err_msg: [u8; 128],
    items: [WireItem; MAX_ITEMS],
    num_items: i32,
}

#[repr(C)]
struct DrmModeResources {
    _count_fbs: libc::c_int,
    _fbs: *mut u32,
    _count_crtcs: libc::c_int,
    _crtcs: *mut u32,
    count_connectors: libc::c_int,
    connectors: *mut u32,
    _count_encoders: libc::c_int,
    _encoders: *mut u32,
    _min_width: u32,
    _max_width: u32,
    _min_height: u32,
    _max_height: u32,
}

#[repr(C)]
struct DrmModeModeInfo {
    _clock: u32,
    hdisplay: u16,
    _hsync_start: u16,
    _hsync_end: u16,
    _htotal: u16,
    _hskew: u16,
    vdisplay: u16,
    _vsync_start: u16,
    _vsync_end: u16,
    _vtotal: u16,
    _vscan: u16,
    _vrefresh: u32,
    _flags: u32,
    _mode_type: u32,
    _name: [libc::c_char; 32],
}

#[repr(C)]
struct DrmModeConnector {
    connector_id: u32,
    encoder_id: u32,
    connector_type: u32,
    connector_type_id: u32,
    connection: libc::c_int,
    _mm_width: u32,
    _mm_height: u32,
    _subpixel: libc::c_int,
    _count_modes: libc::c_int,
    _modes: *mut DrmModeModeInfo,
    _count_props: libc::c_int,
    _props: *mut u32,
    _prop_values: *mut u64,
    _count_encoders: libc::c_int,
    _encoders: *mut u32,
}

#[repr(C)]
struct DrmModeEncoder {
    _encoder_id: u32,
    _encoder_type: u32,
    crtc_id: u32,
    _possible_crtcs: u32,
    _possible_clones: u32,
}

#[repr(C)]
struct DrmModeCrtc {
    _crtc_id: u32,
    _buffer_id: u32,
    _x: u32,
    _y: u32,
    _width: u32,
    _height: u32,
    mode_valid: libc::c_int,
    mode: DrmModeModeInfo,
    _gamma_size: libc::c_int,
}

#[repr(C)]
struct DrmModeObjectProperties {
    count_props: u32,
    props: *mut u32,
    prop_values: *mut u64,
}

#[repr(C)]
struct DrmModeProperty {
    _prop_id: u32,
    _flags: u32,
    name: [libc::c_char; 32],
    _count_values: libc::c_int,
    _values: *mut u64,
    _count_enums: libc::c_int,
    _enums: *mut libc::c_void,
    _count_blobs: libc::c_int,
    _blob_ids: *mut u32,
}

#[link(name = "drm")]
extern "C" {
    fn drmModeGetResources(fd: libc::c_int) -> *mut DrmModeResources;
    fn drmModeFreeResources(resources: *mut DrmModeResources);
    fn drmModeGetConnectorCurrent(fd: libc::c_int, connector_id: u32) -> *mut DrmModeConnector;
    fn drmModeFreeConnector(connector: *mut DrmModeConnector);
    fn drmModeGetEncoder(fd: libc::c_int, encoder_id: u32) -> *mut DrmModeEncoder;
    fn drmModeFreeEncoder(encoder: *mut DrmModeEncoder);
    fn drmModeGetCrtc(fd: libc::c_int, crtc_id: u32) -> *mut DrmModeCrtc;
    fn drmModeFreeCrtc(crtc: *mut DrmModeCrtc);
    fn drmModeObjectGetProperties(
        fd: libc::c_int,
        object_id: u32,
        object_type: u32,
    ) -> *mut DrmModeObjectProperties;
    fn drmModeFreeObjectProperties(properties: *mut DrmModeObjectProperties);
    fn drmModeGetProperty(fd: libc::c_int, property_id: u32) -> *mut DrmModeProperty;
    fn drmModeFreeProperty(property: *mut DrmModeProperty);
    fn drmModeGetConnectorTypeName(connector_type: u32) -> *const libc::c_char;
}

struct DrmResources(NonNull<DrmModeResources>);
struct DrmConnector(NonNull<DrmModeConnector>);
struct DrmEncoder(NonNull<DrmModeEncoder>);
struct DrmCrtc(NonNull<DrmModeCrtc>);
struct DrmObjectProperties(NonNull<DrmModeObjectProperties>);
struct DrmProperty(NonNull<DrmModeProperty>);

impl Drop for DrmResources {
    fn drop(&mut self) {
        unsafe { drmModeFreeResources(self.0.as_ptr()) }
    }
}
impl Drop for DrmConnector {
    fn drop(&mut self) {
        unsafe { drmModeFreeConnector(self.0.as_ptr()) }
    }
}
impl Drop for DrmEncoder {
    fn drop(&mut self) {
        unsafe { drmModeFreeEncoder(self.0.as_ptr()) }
    }
}
impl Drop for DrmCrtc {
    fn drop(&mut self) {
        unsafe { drmModeFreeCrtc(self.0.as_ptr()) }
    }
}
impl Drop for DrmObjectProperties {
    fn drop(&mut self) {
        unsafe { drmModeFreeObjectProperties(self.0.as_ptr()) }
    }
}
impl Drop for DrmProperty {
    fn drop(&mut self) {
        unsafe { drmModeFreeProperty(self.0.as_ptr()) }
    }
}
