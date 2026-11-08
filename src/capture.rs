use crate::gpu::{self, Gpu};
use crate::kms::{self, GammaTracker, HdrMetadata, KmsClient, Monitor};
use crate::Result;

pub struct Capture {
    monitor: Monitor,
    gpu: Gpu,
    kms: KmsClient,
    gamma: Option<GammaTracker>,
    diagnostics: Vec<PlaneDiagnostic>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaneDiagnostic {
    pub index: usize,
    pub plane_type: u32,
    pub width: u32,
    pub height: u32,
    pub pixel_format: u32,
    pub modifier: u64,
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
    pub sample_count: usize,
    pub r: f32,
    pub g: f32,
    pub b: f32,
    /// Mean alpha from the imported framebuffer pixels; DRM plane alpha is not included.
    pub a: f32,
}

pub struct DiagnosticFrame<'a> {
    pub frame: Frame<'a>,
    pub planes: &'a [PlaneDiagnostic],
    pub crtc_gamma_applied: bool,
}

pub struct Frame<'a> {
    pub data: &'a [u8],
    pub width: usize,
    pub height: usize,
    pub stride: usize,
}

impl Capture {
    pub fn new(monitor_name: &str) -> Result<Self> {
        let monitor = kms::select_monitor(monitor_name)?;
        let kms = KmsClient::new(&monitor)?;
        let gpu = Gpu::new(&monitor)?;
        let gamma = GammaTracker::new(&monitor);
        Ok(Self {
            monitor,
            gpu,
            kms,
            gamma,
            diagnostics: Vec::new(),
        })
    }
    pub fn frame(&mut self, area_filter: bool) -> Result<Frame<'_>> {
        let planes = self.kms.capture()?;
        let gamma = self.gamma.as_mut().and_then(GammaTracker::current);
        self.gpu.render(&self.monitor, planes, area_filter, gamma)?;
        let (data, width, height, stride) = self.gpu.frame();
        Ok(Frame {
            data,
            width,
            height,
            stride,
        })
    }

    pub fn diagnostic_frame(&mut self, area_filter: bool) -> Result<DiagnosticFrame<'_>> {
        let planes = self.kms.capture()?;
        let gamma = self.gamma.as_mut().and_then(GammaTracker::current);
        self.gpu
            .render_diagnostic(&self.monitor, planes, &mut self.diagnostics, area_filter, gamma)?;
        let (data, width, height, stride) = self.gpu.frame();
        let frame = Frame {
            data,
            width,
            height,
            stride,
        };
        Ok(DiagnosticFrame {
            frame,
            planes: &self.diagnostics,
            crtc_gamma_applied: self.gpu.crtc_gamma_active(),
        })
    }

    pub fn monitor(&self) -> &Monitor {
        &self.monitor
    }
}

pub fn self_test() -> Result<()> {
    gpu::self_test()
}

pub fn synthetic_render_test(monitor_name: &str) -> Result<()> {
    let monitor = kms::select_monitor(monitor_name)?;
    gpu::synthetic_render_test(&monitor)
}
