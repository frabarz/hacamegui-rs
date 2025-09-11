use std::cell::Cell;

use anyhow::Result;
use hacam_lib_rs::{
    CamError,
    cam::{HaCam, ThermalStatus},
    settings::{
        self, LiveViewResolution, PhotoResolution, PictureOrientation, Resolution, VideoResolution,
    },
    util::CamUtil,
};
use tokio::{
    runtime::Handle,
    sync::{
        OnceCell,
        mpsc::{Receiver, Sender},
    },
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CamState {
    #[default]
    None = 0,
    Initialized = 1,
    LiveViewStreaming,
    TakingPicture,
    Recording
}

#[derive(Clone, Debug)]
pub struct CamFrame {
    pub frame: Vec<u8>,
    pub width: usize,
    pub height: usize,
}

#[derive(Debug, Copy, Clone)]
pub enum CamInMessage {
    Init,

    StartLiveView(settings::LiveViewResolution),
    StopLiveView,

    StartRecording,
    StopRecording,

    TakePhoto {
        orientation: PictureOrientation,
    },

    GetFrame,

    PowerOff,

    Keepalive,

    WriteSetting {
        typ: settings::SettingType,
        value: u8,
    },

    ReadSetting {
        typ: settings::SettingType,
    },
}

#[derive(Debug)]
pub enum CamOutMessage {
    Info(String),
    StartRecordingStatus(bool, VideoResolution),
    StopRecordingStatus(bool),
    Frame {
        frame: CamFrame,
        typ: CamFrameType,
        thermal_status: Option<ThermalStatus>,
    },
    Setting {
        typ: settings::SettingType,
        value: u8,
    },
    Error(std::result::Result<CamError, String>),
}

#[derive(Debug, Clone, Copy)]
pub enum CamFrameType {
    LiveView,
    Photo,
    Thumbnail,
}

pub async fn cam_worker(mut rx: Receiver<CamInMessage>, tx: Sender<CamOutMessage>) -> Result<()> {
    let mut cam = OnceCell::new();
    let lv_res = Cell::new(LiveViewResolution::Low);
    let was_lv_initialized = OnceCell::new();

    while let Some(in_msg) = rx.recv().await {
        if let CamInMessage::Init = in_msg {
            if !cam.initialized() {
                let cam_inst = match HaCam::new() {
                    Ok(cam_inst) => cam_inst,
                    Err(e) => {
                        tx.send(CamOutMessage::Error(Ok(e))).await?;
                        continue;
                    }
                };

                if cam.set(cam_inst).is_err() {
                    tx.send(CamOutMessage::Error(Err(
                        "Camera cannot be initialized twice!".to_owned(),
                    )))
                    .await?;
                }
            }

            let Some(cam) = cam.get_mut() else {
                tx.send(CamOutMessage::Error(Err(
                    "Camera is not initialized!".to_owned()
                )))
                .await?;
                continue;
            };

            cam.initialize_comm().await?;

            continue;
        }

        if !cam.initialized() && let CamInMessage::Keepalive = in_msg {
            continue;
        }

        let Some(cam) = cam.get_mut() else {
            tx.send(CamOutMessage::Error(Err(
                "Camera initialization error!".to_owned()
            )))
            .await?;
            continue;
        };

        match in_msg {
            CamInMessage::StartLiveView(live_view_resolution) => {
                info!("Requested to start live view! Resolution: {live_view_resolution:#?}");
                cam.start_live_view(live_view_resolution).await?;
                lv_res.replace(live_view_resolution);
                let _ = was_lv_initialized.set(());
            }
            CamInMessage::StopLiveView => {
                info!("Requested to stop live view!");
                cam.stop_live_view().await?;
            }
            CamInMessage::StartRecording => {
                info!("Requested to start recording!");

                if cam.check_live_view_status().await? {
                    info!("Live view is on, requesting stopping!");
                    cam.stop_live_view().await?;
                }

                let video_res = VideoResolution::try_from(
                    cam.read_setting(settings::SettingType::VideoResolution)
                        .await? as i8,
                )
                .unwrap();

                let as_lv_res = match video_res {
                    VideoResolution::High => LiveViewResolution::High,
                    VideoResolution::Low => LiveViewResolution::Low,
                    VideoResolution::Unknown => LiveViewResolution::Low,
                };

                lv_res.replace(as_lv_res);

                cam.start_recording().await?;

                let rec_req = cam.check_start_recording_request().await?;
                tx.send(CamOutMessage::StartRecordingStatus(rec_req, video_res))
                    .await?;
            }
            CamInMessage::StopRecording => {
                info!("Requested to stop recording!");
                cam.stop_recording().await?;

                let stop_rec_req = cam.check_start_recording_request().await?;
                tx.send(CamOutMessage::StopRecordingStatus(stop_rec_req))
                    .await?;
            }
            CamInMessage::TakePhoto { orientation } => {
                info!("Requested to take photo! Orientation: {orientation:#?}");
                let on_thumbnail = |thumb| {
                    info!("Received thumbnail!");

                    Handle::current().spawn({
                        let tx = tx.clone();

                        async move {
                            tx.send(CamOutMessage::Frame {
                                frame: CamFrame {
                                    frame: thumb,
                                    width: 272,
                                    height: 272,
                                },
                                typ: CamFrameType::Thumbnail,
                                thermal_status: None,
                            })
                            .await
                            .unwrap();
                        }
                    });
                };

                let photo_res = PhotoResolution::try_from(
                    cam.read_setting(settings::SettingType::PhotoResolution)
                        .await? as i8,
                )
                .unwrap();

                let photo = cam
                    .take_picture_and_get(
                        orientation,
                        Some(on_thumbnail),
                        was_lv_initialized.initialized(),
                    )
                    .await?;

                tx.send(CamOutMessage::Frame {
                    frame: CamFrame {
                        frame: photo,
                        width: photo_res.w() as usize,
                        height: photo_res.h() as usize,
                    },
                    typ: CamFrameType::Photo,
                    thermal_status: None,
                })
                .await?;
            }

            CamInMessage::GetFrame => {
                let (thermal_status, frame) = cam.get_live_view_frame().await?;

                let res = lv_res.get();

                tx.send(CamOutMessage::Frame {
                    frame: CamFrame {
                        frame: frame.data,
                        width: res.w() as usize,
                        height: res.h() as usize,
                    },
                    typ: CamFrameType::LiveView,
                    thermal_status: Some(thermal_status),
                })
                .await?;
            }

            CamInMessage::Keepalive => {
                cam.send_keepalive().await?;
            }

            CamInMessage::PowerOff => {
                cam.power_off().await?;
            }

            CamInMessage::ReadSetting { typ } => {
                let value = cam.read_setting(typ).await?;
                tx.send(CamOutMessage::Setting { typ, value }).await?;
            }

            CamInMessage::WriteSetting { typ, value } => {
                cam.write_setting(typ, value).await?;
            }

            _ => {}
        }
    }

    Ok(())
}
