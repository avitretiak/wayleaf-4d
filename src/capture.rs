use crate::gpu::{self, Gpu};
use crate::kms::{self, KmsClient, Monitor};
use crate::Result;

pub struct Capture {
    monitor: Monitor,
    gpu: Gpu,
    kms: KmsClient,
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
        Ok(Self { monitor, gpu, kms })
    }

    pub fn frame(&mut self) -> Result<Frame<'_>> {
        let planes = self.kms.capture()?;
        self.gpu.render(&self.monitor, planes)?;
        let (data, width, height, stride) = self.gpu.frame();
        Ok(Frame {
            data,
            width,
            height,
            stride,
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
