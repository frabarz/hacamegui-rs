use eframe::{
    egui,
    egui_wgpu::{self, RenderState},
};

use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use std::{f32::consts::FRAC_PI_2, sync::mpsc::Receiver};
use std::mem;
use wgpu::util::DeviceExt;

use crate::cam::CamFrame;

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct CameraUniform {
    inv_proj: [[f32; 4]; 4],
    inv_view: [[f32; 4]; 4],
}

pub struct AppState {
    width: u32,
    height: u32,
    yaw: f32,
    pitch: f32,
    zoom: f32,
    rx: Receiver<CamFrame>,
    placeholder_frame: Option<CamFrame>,
}

impl AppState {
    pub fn new<'a>(
        cc: &'a eframe::CreationContext<'a>,
        width: u32,
        height: u32,
        rx: Receiver<CamFrame>,
    ) -> Self {
        let wgpu_render_state = cc.wgpu_render_state.as_ref().unwrap();

        let RenderState { device, queue, .. } = wgpu_render_state;

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());

        let camera_uniform = CameraUniform {
            inv_proj: glam::Mat4::IDENTITY.to_cols_array_2d(),
            inv_view: glam::Mat4::IDENTITY.to_cols_array_2d(),
        };

        let uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("CameraUniform"),
            contents: bytemuck::bytes_of(&camera_uniform),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(
                            (mem::size_of::<CameraUniform>()) as _,
                        ),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
            label: None,
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sphericalrenderer_shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "shader.wgsl"
            ))),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sphericalrenderer_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sphericalrenderer_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(
                    cc.wgpu_render_state.as_ref().unwrap().target_format.into(),
                )],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        wgpu_render_state
            .renderer
            .write()
            .callback_resources
            .insert(SphericalRendererResources {
                device: device.clone(),
                width,
                height,
                render_pipeline,
                uniform_buf,
                sampler,
                queue: queue.clone(),
            });

        Self {
            yaw: 0.0,
            pitch: 0.0,
            zoom: 1.0,
            width,
            height,
            rx,
            placeholder_frame: None,
        }
    }

    fn custom_painting(
        &mut self,
        ui: &mut egui::Ui,
        current_frame: Option<CamFrame>,
        next_frame: Option<CamFrame>,
    ) {
        let (rect, response) = ui.allocate_exact_size(
            egui::Vec2 {
                x: self.width as f32,
                y: self.height as f32,
            },
            egui::Sense::drag(),
        );

        let egui::Vec2 { x, y } = response.drag_motion();
        let zoom_delta_y = ui.input(|i| i.zoom_delta());

        self.yaw += x * 0.005;

        self.pitch += y * 0.005;
        self.pitch = self.pitch.clamp(-FRAC_PI_2 + 0.01, FRAC_PI_2 - 0.01);

        self.zoom += 1. - zoom_delta_y;
        self.zoom = self.zoom.clamp(0.5, 1.5);

        if current_frame.is_some() {
            self.placeholder_frame = current_frame.clone();
        }

        ui.painter().add(egui_wgpu::Callback::new_paint_callback(
            rect,
            SphericalRendererPaintCallback {
                current_frame: current_frame.or(self.placeholder_frame.clone()),
                next_frame,
                yaw: self.yaw,
                pitch: self.pitch,
                zoom: self.zoom,
            },
        ));
    }
}

impl eframe::App for AppState {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Shouldn't be too expensive to run. If this causes issues down the line,
        // we can only request a repaint each time a frame is decoded.
        ctx.request_repaint();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                //ui.spacing_mut().item_spacing.x = 0.0;
                //ui.label("The triangle is being painted using ");
                //ui.hyperlink_to("WGPU", "https://wgpu.rs");
                //ui.label(" (Portable Rust graphics API awesomeness)");
            });
            ui.label("CTRL+SCROLL to zoom.");

            egui::Frame::canvas(ui.style()).show(ui, |ui| {
                self.custom_painting(ui, self.rx.try_recv().ok(), None);
            });
        });
    }
}

struct SphericalRendererPaintCallback {
    yaw: f32,
    pitch: f32,
    zoom: f32,
    current_frame: Option<CamFrame>,
    next_frame: Option<CamFrame>,
}

impl egui_wgpu::CallbackTrait for SphericalRendererPaintCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let resources: &mut SphericalRendererResources = resources.get_mut().unwrap();
        resources.prepare(device, queue, self.yaw, self.pitch, self.zoom);
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let resources: &SphericalRendererResources = resources.get().unwrap();
        resources
            .paint(
                render_pass,
                self.current_frame.as_ref(),
                self.next_frame.as_ref(),
            )
            .unwrap();
    }
}

struct SphericalRendererResources {
    device: wgpu::Device,

    width: u32,
    height: u32,

    queue: wgpu::Queue,

    render_pipeline: wgpu::RenderPipeline,
    uniform_buf: wgpu::Buffer,
    sampler: wgpu::Sampler,
}

impl SphericalRendererResources {
    fn prepare(
        &mut self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        yaw: f32,
        pitch: f32,
        zoom: f32,
    ) {
        let proj = glam::Mat4::perspective_rh(
            std::f32::consts::FRAC_PI_2 * zoom,
            self.width as f32 / self.height as f32,
            0.1,
            10.0,
        );

        // yaw around global Y, pitch around camera's local X
        let yaw_rot = glam::Quat::from_rotation_y(yaw);
        let pitch_rot = glam::Quat::from_rotation_x(pitch);

        let orientation = yaw_rot * pitch_rot;

        // camera pos is in the middle of the sphere
        let cam_pos = glam::Vec3::ZERO;

        // camera matrix in the world space
        let cam_world = glam::Mat4::from_rotation_translation(orientation, cam_pos);

        // view matrix = inverse of camera world transform
        let view = cam_world.inverse();

        let inv_proj = proj.inverse();
        let inv_view = view.inverse();

        let cam = CameraUniform {
            inv_proj: inv_proj.to_cols_array_2d(),
            inv_view: inv_view.to_cols_array_2d(),
        };

        queue.write_buffer(&self.uniform_buf, 0, bytemuck::bytes_of(&cam));
    }

    fn paint(
        &self,
        render_pass: &mut wgpu::RenderPass<'_>,
        current_frame: Option<&CamFrame>,
        _next_frame: Option<&CamFrame>,
    ) -> Result<(), wgpu::SurfaceError> {
        let Some(CamFrame {
            frame: video_frame,
            height,
            width,
            ..
        }) = current_frame
        else {
            return Ok(());
        };

        let texture = self.device.create_texture_with_data(
            &self.queue,
            &wgpu::TextureDescriptor {
                label: Some("sphericalrenderer_texture"),
                size: wgpu::Extent3d {
                    width: *width as u32,
                    height: *height as u32,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Bgra8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            video_frame,
        );

        let texture_view = texture.create_view(&Default::default());

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sphericalrenderer_bind_group"),
            layout: &self.render_pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });

        render_pass.set_pipeline(&self.render_pipeline);
        render_pass.set_bind_group(0, &bind_group, &[]);
        render_pass.draw(0..3, 0..1);

        Ok(())
    }
}
