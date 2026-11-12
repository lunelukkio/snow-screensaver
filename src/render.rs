//! Direct3D 11 renderer.
//!
//! Each monitor window owns a flip-model swap chain whose back buffer may be
//! smaller than the monitor (the compositor stretches it). All flakes of a
//! monitor are drawn with one instanced call: the instance buffer holds one
//! disc centre per flake and the pixel shader computes the soft disc
//! analytically, so no flake image is ever rasterised on the CPU.

use std::ffi::c_void;
use std::mem::size_of;

use windows::core::{s, Interface, Result};
use windows::Win32::Foundation::{E_FAIL, HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_10_0, D3D_FEATURE_LEVEL_10_1,
    D3D_FEATURE_LEVEL_11_0, D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_IGNORE, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_R32G32_FLOAT,
    DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::*;

static VS_BYTES: &[u8] = include_bytes!("../shaders/snow_vs.cso");
static PS_BYTES: &[u8] = include_bytes!("../shaders/snow_ps.cso");

/// Bytes per instance: one `float2` disc centre.
const INSTANCE_STRIDE: u32 = 8;

/// Everything that decides how a flake looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    /// Disc diameter in monitor pixels.
    pub size: f32,
    /// Edge softness in monitor pixels.
    pub edge_blur: f32,
    pub color: [u8; 3],
    /// Back-buffer size in percent of the monitor size.
    pub resolution: u32,
}

/// Mirrors `cbuffer Frame` in `shaders/snow.hlsl`.
#[repr(C)]
struct Constants {
    clip_scale: [f32; 2],
    radius: f32,
    soft: f32,
    color: [f32; 4],
}

struct Target {
    swap_chain: IDXGISwapChain1,
    // One-element arrays so they can be handed to Direct3D as slices without
    // cloning (and therefore without touching reference counts) every frame.
    rtv: [Option<ID3D11RenderTargetView>; 1],
    constants: [Option<ID3D11Buffer>; 1],
    instances: Option<ID3D11Buffer>,
    monitor_width: u32,
    monitor_height: u32,
    viewport: D3D11_VIEWPORT,
    /// Back-buffer pixels per monitor pixel.
    scale: f32,
}

pub struct Renderer {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    targets: Vec<Target>,
    /// Maximum number of flakes per monitor.
    capacity: usize,
    resolution: u32,
}

/// Back-buffer size for a monitor at `resolution` percent.
pub fn buffer_size(width: u32, height: u32, resolution: u32) -> (u32, u32) {
    let scaled = |v: u32| ((v as u64 * resolution as u64 + 50) / 100).max(1) as u32;
    (scaled(width), scaled(height))
}

unsafe fn create_device() -> Result<(ID3D11Device, ID3D11DeviceContext)> {
    let levels = [
        D3D_FEATURE_LEVEL_11_0,
        D3D_FEATURE_LEVEL_10_1,
        D3D_FEATURE_LEVEL_10_0,
    ];
    let mut last_error = None;
    // WARP (software) is only a fallback for machines without a usable GPU.
    for driver in [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP] {
        let (mut device, mut context) = (None, None);
        let created = D3D11CreateDevice(
            None,
            driver,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_SINGLETHREADED,
            Some(&levels),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        );
        match (created, device, context) {
            (Ok(()), Some(device), Some(context)) => return Ok((device, context)),
            (Err(e), ..) => last_error = Some(e),
            _ => {}
        }
    }
    Err(last_error.unwrap_or_else(|| E_FAIL.into()))
}

/// (Re)creates the render-target view after the back buffer was created or resized.
unsafe fn bind_back_buffer(device: &ID3D11Device, target: &mut Target) -> Result<()> {
    let back: ID3D11Texture2D = target.swap_chain.GetBuffer(0)?;
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    back.GetDesc(&mut desc);
    let mut rtv = None;
    device.CreateRenderTargetView(&back, None, Some(&mut rtv))?;
    target.rtv = [rtv];
    target.viewport = D3D11_VIEWPORT {
        TopLeftX: 0.0,
        TopLeftY: 0.0,
        Width: desc.Width as f32,
        Height: desc.Height as f32,
        MinDepth: 0.0,
        MaxDepth: 1.0,
    };
    target.scale = desc.Width as f32 / target.monitor_width as f32;
    Ok(())
}

unsafe fn write_constants(context: &ID3D11DeviceContext, target: &Target, look: &Look) {
    let [r, g, b] = look.color.map(|c| c as f32 / 255.0);
    let constants = Constants {
        clip_scale: [2.0 / target.viewport.Width, 2.0 / target.viewport.Height],
        radius: look.size * 0.5 * target.scale,
        // Keep at least half a pixel of ramp so a hard edge is still antialiased.
        soft: (look.edge_blur * 2.0 * target.scale).max(0.5),
        color: [r, g, b, 1.0],
    };
    if let Some(buffer) = &target.constants[0] {
        let data = &constants as *const Constants as *const c_void;
        context.UpdateSubresource(buffer, 0, None, data, 0, 0);
    }
}

impl Renderer {
    /// Creates the device and one swap chain per `(window, width, height)`.
    pub unsafe fn new(
        windows: &[(HWND, u32, u32)],
        look: &Look,
        capacity: usize,
    ) -> Result<Renderer> {
        let (device, context) = create_device()?;
        let factory: IDXGIFactory2 = device.cast::<IDXGIDevice>()?.GetAdapter()?.GetParent()?;

        let (mut vs, mut ps, mut layout, mut blend, mut raster) = (None, None, None, None, None);
        device.CreateVertexShader(VS_BYTES, None, Some(&mut vs))?;
        device.CreatePixelShader(PS_BYTES, None, Some(&mut ps))?;
        let element = D3D11_INPUT_ELEMENT_DESC {
            SemanticName: s!("POSITION"),
            SemanticIndex: 0,
            Format: DXGI_FORMAT_R32G32_FLOAT,
            InputSlot: 0,
            AlignedByteOffset: 0,
            InputSlotClass: D3D11_INPUT_PER_INSTANCE_DATA,
            InstanceDataStepRate: 1,
        };
        device.CreateInputLayout(&[element], VS_BYTES, Some(&mut layout))?;

        // Premultiplied-alpha "over": overlapping flakes blend instead of clipping.
        let mut blend_desc = D3D11_BLEND_DESC::default();
        blend_desc.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
            BlendEnable: true.into(),
            SrcBlend: D3D11_BLEND_ONE,
            DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
            BlendOp: D3D11_BLEND_OP_ADD,
            SrcBlendAlpha: D3D11_BLEND_ONE,
            DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
            BlendOpAlpha: D3D11_BLEND_OP_ADD,
            RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
        };
        device.CreateBlendState(&blend_desc, Some(&mut blend))?;
        let raster_desc = D3D11_RASTERIZER_DESC {
            FillMode: D3D11_FILL_SOLID,
            CullMode: D3D11_CULL_NONE,
            DepthClipEnable: true.into(),
            ..Default::default()
        };
        device.CreateRasterizerState(&raster_desc, Some(&mut raster))?;

        // This state survives Present, so it is set once. The context keeps
        // its own references to the bound objects.
        context.IASetInputLayout(layout.as_ref());
        context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
        context.VSSetShader(vs.as_ref(), None);
        context.PSSetShader(ps.as_ref(), None);
        context.OMSetBlendState(blend.as_ref(), None, 0xffff_ffff);
        context.RSSetState(raster.as_ref());

        let mut renderer = Renderer {
            device,
            context,
            targets: Vec::with_capacity(windows.len()),
            capacity,
            resolution: look.resolution,
        };
        for &(hwnd, width, height) in windows {
            let target = renderer.create_target(&factory, hwnd, width, height, look)?;
            renderer.targets.push(target);
        }
        Ok(renderer)
    }

    unsafe fn create_target(
        &self,
        factory: &IDXGIFactory2,
        hwnd: HWND,
        width: u32,
        height: u32,
        look: &Look,
    ) -> Result<Target> {
        let (buffer_width, buffer_height) = buffer_size(width, height, look.resolution);
        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: buffer_width,
            Height: buffer_height,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
            AlphaMode: DXGI_ALPHA_MODE_IGNORE,
            ..Default::default()
        };
        let swap_chain = factory.CreateSwapChainForHwnd(&self.device, hwnd, &desc, None, None)?;
        // The window never resizes or enters exclusive full screen on its own.
        let _ =
            factory.MakeWindowAssociation(hwnd, DXGI_MWA_NO_ALT_ENTER | DXGI_MWA_NO_WINDOW_CHANGES);

        let instance_desc = D3D11_BUFFER_DESC {
            ByteWidth: self.capacity as u32 * INSTANCE_STRIDE,
            Usage: D3D11_USAGE_DYNAMIC,
            BindFlags: D3D11_BIND_VERTEX_BUFFER.0 as u32,
            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
            ..Default::default()
        };
        let mut instances = None;
        self.device
            .CreateBuffer(&instance_desc, None, Some(&mut instances))?;

        let constant_desc = D3D11_BUFFER_DESC {
            ByteWidth: size_of::<Constants>() as u32,
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
            ..Default::default()
        };
        let mut constants = None;
        self.device
            .CreateBuffer(&constant_desc, None, Some(&mut constants))?;

        let mut target = Target {
            swap_chain,
            rtv: [None],
            constants: [constants],
            instances,
            monitor_width: width,
            monitor_height: height,
            viewport: D3D11_VIEWPORT::default(),
            scale: 1.0,
        };
        bind_back_buffer(&self.device, &mut target)?;
        write_constants(&self.context, &target, look);
        Ok(target)
    }

    /// Applies a changed size, softness, colour or resolution.
    pub unsafe fn set_look(&mut self, look: &Look) -> Result<()> {
        let resize = look.resolution != self.resolution;
        if resize {
            // The back buffers cannot be resized while anything refers to them.
            self.context.OMSetRenderTargets(None, None);
        }
        for target in &mut self.targets {
            if resize {
                let (width, height) =
                    buffer_size(target.monitor_width, target.monitor_height, look.resolution);
                target.rtv = [None];
                target.swap_chain.ResizeBuffers(
                    0,
                    width,
                    height,
                    DXGI_FORMAT_UNKNOWN,
                    DXGI_SWAP_CHAIN_FLAG(0),
                )?;
                bind_back_buffer(&self.device, target)?;
            }
            write_constants(&self.context, target, look);
        }
        self.resolution = look.resolution;
        Ok(())
    }

    /// Draws and presents one monitor. `fill` receives the instance buffer
    /// (`count` x/y pairs) and the back-buffer scale, and must write the disc
    /// centres in back-buffer pixels. An error means the device was lost.
    pub unsafe fn draw(
        &mut self,
        index: usize,
        count: usize,
        fill: impl FnOnce(&mut [f32], f32),
    ) -> Result<()> {
        let target = &self.targets[index];
        let context = &self.context;
        let (Some(instances), Some(rtv)) = (&target.instances, &target.rtv[0]) else {
            return Err(E_FAIL.into());
        };
        let count = count.min(self.capacity);

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        context.Map(instances, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped))?;
        let centres = std::slice::from_raw_parts_mut(mapped.pData as *mut f32, count * 2);
        fill(centres, target.scale);
        context.Unmap(instances, 0);

        // Flip-model presentation unbinds the back buffer, so rebind it every frame.
        context.OMSetRenderTargets(Some(&target.rtv), None);
        context.RSSetViewports(Some(&[target.viewport]));
        context.VSSetConstantBuffers(0, Some(&target.constants));
        context.PSSetConstantBuffers(0, Some(&target.constants));
        context.IASetVertexBuffers(
            0,
            1,
            Some(&target.instances),
            Some(&INSTANCE_STRIDE),
            Some(&0u32),
        );
        context.ClearRenderTargetView(rtv, &[0.0, 0.0, 0.0, 1.0]);
        context.DrawInstanced(4, count as u32, 0, 0);
        target.swap_chain.Present(1, DXGI_PRESENT(0)).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_size_scales_and_never_reaches_zero() {
        assert_eq!(buffer_size(3840, 2160, 100), (3840, 2160));
        assert_eq!(buffer_size(3840, 2160, 50), (1920, 1080));
        assert_eq!(buffer_size(1920, 1080, 33), (634, 356));
        assert_eq!(buffer_size(1920, 1080, 25), (480, 270));
        assert_eq!(buffer_size(3, 2, 10), (1, 1));
    }

    #[test]
    fn constants_match_the_shader_layout() {
        assert_eq!(size_of::<Constants>(), 32);
    }
}
