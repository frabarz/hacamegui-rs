//#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

#[macro_use]
extern crate log;

mod cam;
mod decoder;
mod renderer;
mod util;

use anyhow::Result;
use eframe::egui;
use hacam_lib_rs::settings::LiveViewResolution;
use std::time::Duration;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc; // Much faster allocator, can give 20% speedups: https://github.com/emilk/egui/pull/7029

fn main() -> Result<()> {
    lovely_env_logger::try_init(lovely_env_logger::Config {
        with_system_timestamp: true,
        reltime: false,
        short_levels: true,
        with_file_name: true,
        with_line_number: true,
        with_padding: false,
    })?;

    // Channel responsible for sending messages to the camera to the async worker.
    let (cam_in_tx, cam_in_rx) = tokio::sync::mpsc::channel::<cam::CamInMessage>(16);

    // Channel responsible for sending from to the camera to the async worker.
    let (cam_out_tx, mut cam_out_rx) = tokio::sync::mpsc::channel::<cam::CamOutMessage>(16);

    // Channel responsible for raw, undecoded frames originating from the camera to the decoder.
    let (vid_tx, vid_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(3);

    // Channel responsible for sending messages from the decoder to the renderer..
    let (frame_tx, frame_rx) = std::sync::mpsc::sync_channel::<crate::cam::CamFrame>(3);

    let rt = tokio::runtime::Runtime::new()?;

    rt.spawn(async move {
        while let Some(msg) = cam_out_rx.recv().await {
            match msg {
                cam::CamOutMessage::Info(i) => info!("Received info from cam: {i}"),
                cam::CamOutMessage::Frame { frame, typ, .. } => {
                    if vid_tx.send(frame.frame.clone()).is_err() {
                        error!("Couldn't send the live view frame to the decoder!");
                    }

                    if let cam::CamFrameType::Photo = typ {
                        let save_fh = rfd::AsyncFileDialog::new()
                            .set_file_name("photo.jpg")
                            .save_file()
                            .await;

                        if let Some(save_fh) = save_fh
                            && let Err(e) = std::fs::write(save_fh.path(), &frame.frame) {
                                error!("An error occured while writing photo (path: {:?}, error: {e})!", save_fh.path());
                            };
                    }
                }
                cam::CamOutMessage::Error(cam_error) => {
                    error!("An error in camera occured! {cam_error:#?}");
                },
                cam::CamOutMessage::Setting { typ, value } => {
                    info!("The value of setting {typ:?} is {value}");
                },
                
            }
        }
    });

    rt.spawn(async move {
        cam::cam_worker(cam_in_rx, cam_out_tx).await.unwrap();
    });

    std::thread::Builder::new()
        .name("frame-decoder-thread".to_string())
        .spawn(|| {
            crate::decoder::run_cam_lv(vid_rx, frame_tx).unwrap();
        })?;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([650.0, 650.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "Huawei 360° Camera GUI",
        options,
        Box::new(|cc| {
            Ok(Box::new(renderer::AppState::new(
                cc, 650, 650, frame_rx, cam_in_tx,
            )))
        }),
    )
    .unwrap();

    Ok(())
}
