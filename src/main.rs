//#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

#[macro_use]
extern crate log;

mod cam;
mod decoder;
mod renderer;
mod util;

use anyhow::Result;
use eframe::egui;
use hacam_lib_rs::settings::Resolution;
use image::{ImageDecoder, codecs::jpeg::JpegDecoder};
use std::{
    io::Cursor,
    sync::{Arc, Mutex, mpsc::SyncSender},
};
use tokio::runtime;

use crate::cam::CamFrame;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc; // Much faster allocator, can give 20% speedups: https://github.com/emilk/egui/pull/7029

fn main() -> Result<()> {
    lovely_env_logger::try_init(lovely_env_logger::Config {
        with_system_timestamp: true,
        reltime: false,
        short_levels: true,
        with_file_name: false,
        with_line_number: true,
        with_padding: false,
    })?;

    // Channel responsible for sending messages to the camera to the async worker.
    let (cam_in_tx, cam_in_rx) = tokio::sync::mpsc::channel::<cam::CamInMessage>(512);

    // Channel responsible for sending from to the camera to the async worker.
    let (cam_out_tx, mut cam_out_rx) = tokio::sync::mpsc::channel::<cam::CamOutMessage>(512);

    // Channel responsible for raw, undecoded frames originating from the camera to the decoder.
    let (vid_tx, vid_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(16);

    // Channel responsible for sending messages from the decoder to the renderer..
    let (frame_tx, frame_rx) = std::sync::mpsc::sync_channel::<crate::cam::CamFrame>(16);

    let rt = tokio::runtime::Runtime::new()?;

    rt.spawn({
        let frame_tx = frame_tx.clone();

        async move {
            let (mut muxer_tx, muxer_rx) = (None::<SyncSender<Option<_>>>, Arc::new(Mutex::new(None)));

            while let Some(msg) = cam_out_rx.recv().await {
                if (cam_out_rx.capacity() as f32 / cam_out_rx.max_capacity() as f32) < 0.5 {
                    warn!("cam_out_rx backpressure too high (50%)!");
                }

                match msg {
                    cam::CamOutMessage::Info(i) => info!("Received info from cam: {i}"),
                    cam::CamOutMessage::Frame { frame, typ, .. } => {
                        if let cam::CamFrameType::LiveView = typ {
                            if vid_tx.send(frame.frame.clone()).is_err() {
                                error!("Couldn't send the live view frame to the decoder!");
                            }

                            if let Some(ref muxer_tx) = muxer_tx
                                && muxer_tx.send(Some(frame.frame.clone())).is_err()
                            {
                                error!("Couldn't send frame to muxer!");
                            }
                        }

                        if let cam::CamFrameType::Photo = typ {
                            let save_fh = rfd::AsyncFileDialog::new()
                                .set_title("Pick a photo save path")
                                .set_file_name("photo.jpg")
                                .save_file()
                                .await;

                            if let Some(save_fh) = save_fh
                                && let Err(e) = std::fs::write(save_fh.path(), &frame.frame)
                            {
                                error!(
                                    "An error occured while writing photo (path: {:?}, error: {e})!",
                                    save_fh.path()
                                );
                            };

                            if let Ok(decoder) = JpegDecoder::new(Cursor::new(frame.frame)) {
                                let mut rgb_buf: Vec<u8> = vec![0; (decoder.total_bytes()) as usize];

                                let (w, h) = decoder.dimensions();
                                
                                decoder.read_image(&mut rgb_buf).unwrap();

                                let mut bgra_buf: Vec<u8> = vec![0; (w * h) as usize * 4];

                                yuv::rgb_to_bgra(&rgb_buf, w * 3, &mut bgra_buf, w * 4, w, h).unwrap();

                                frame_tx.send(CamFrame {
                                    frame: bgra_buf,
                                    width: w as usize,
                                    height: h as usize,
                                }).unwrap();
                            }
                        }
                    }
                    cam::CamOutMessage::StartRecordingStatus(_, res) => {
                        let save_fh = rfd::AsyncFileDialog::new()
                            .set_title("Pick a video save path")
                            .set_file_name("video.mp4")
                            .save_file()
                            .await;

                        let Some(save_fh) = save_fh else {
                            continue;
                        };

                        let (new_muxer_tx, new_muxer_rx) =
                            std::sync::mpsc::sync_channel::<Option<Vec<u8>>>(30);

                        *muxer_rx.lock().unwrap() = Some(new_muxer_rx);
                        muxer_tx = Some(new_muxer_tx);

                        runtime::Handle::current().spawn_blocking({
                            let muxer_rx = muxer_rx.clone();
                            move || {
                                if let Ok(mut muxer_rx) = muxer_rx.lock() {
                                    let muxer_rx =
                                        muxer_rx.as_mut().expect("Muxer must be initialized!");

                                    util::save_mp4(
                                        muxer_rx,
                                        save_fh.path().to_path_buf(),
                                        res.w() as i32,
                                        res.h() as i32,
                                    )
                                    .unwrap();
                                }
                            }
                        });
                    }
                    cam::CamOutMessage::StopRecordingStatus(_) => {
                        if let Some(ref muxer_tx) = muxer_tx
                            && muxer_tx.send(None).is_err()
                        {
                            error!("Couldn't end recording!");
                        };
                    }
                    cam::CamOutMessage::Error(cam_error) => {
                        error!("An error in camera occured! {cam_error:#?}");
                    }
                    cam::CamOutMessage::Setting { typ, value } => {
                        info!("The value of setting {typ:?} is {value}");
                    }
                }
            }
        }
    });

    rt.spawn(async move {
        cam::cam_worker(cam_in_rx, cam_out_tx).await.unwrap();
    });

    std::thread::Builder::new()
        .name("frame-decoder-thread".to_string())
        .spawn({
            let frame_tx = frame_tx.clone();
            || {
                crate::decoder::run_cam_lv(vid_rx, frame_tx).unwrap();
            }
        })?;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([750.0, 750.0]),
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
