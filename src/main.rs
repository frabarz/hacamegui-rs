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

    let (cam_in_tx, cam_in_rx) = tokio::sync::mpsc::channel::<cam::CamInMessage>(16);
    let (cam_out_tx, cam_out_rx) = tokio::sync::mpsc::channel::<cam::CamOutMessage>(16);

    let (vid_tx, vid_rx) = std::sync::mpsc::sync_channel::<hacam_lib_rs::cam::LiveViewFrame>(3);
    let (frame_tx, frame_rx) = std::sync::mpsc::sync_channel::<crate::cam::CamFrame>(3);

    let rt = tokio::runtime::Runtime::new()?;

    rt.spawn(async move {
        cam::cam_worker(cam_in_rx, cam_out_tx).await.unwrap();
    });

    /*
    tokio::task::spawn(async move {
        /*
        let mut cam = hacam_lib_rs::cam::HaCam::new().unwrap();

        cam.initialize_comm().await.unwrap();

        let res = LiveViewResolution::High;
        cam.start_live_view(res).await.unwrap();

        tokio::time::sleep(Duration::from_millis(1000)).await;

        while let Ok((_, frame)) = cam.get_live_view_frame().await {
            vid_tx.send(frame).unwrap();
        }
        */

        anyhow::Ok(())
    });
    */

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
