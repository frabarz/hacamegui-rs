use std::cell::{Cell, RefCell};

use anyhow::Result;
use hacam_lib_rs::{
    cam::{HaCam, ThermalStatus},
    settings::{self, LiveViewResolution, PhotoResolution, PictureOrientation, Resolution},
    util::CamUtil,
};
use tokio::sync::{
    OnceCell,
    mpsc::{Receiver, Sender},
};

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

    WriteSetting {
        typ: settings::SettingType,
        value: u8,
    },

    ReadSetting {
        typ: settings::SettingType,
    },
}

#[derive(Debug, Clone)]
pub enum CamOutMessage {
    Info(String),
    Frame {
        frame: CamFrame,
        typ: CamFrameType,
        thermal_status: Option<ThermalStatus>,
    },
    Setting {
        typ: settings::SettingType,
        value: u8,
    },
    Error(String),
}

#[derive(Debug, Clone, Copy)]
pub enum CamFrameType {
    LiveView,
    Video,
    Photo,
    Thumbnail,
}

pub async fn cam_worker(mut rx: Receiver<CamInMessage>, tx: Sender<CamOutMessage>) -> Result<()> {
    let mut cam = OnceCell::new();
    let lv_res = Cell::new(LiveViewResolution::Low);
    let was_lv_initialized = OnceCell::new();

    while let Some(in_msg) = rx.recv().await {
        if let CamInMessage::Init = in_msg {
            if cam.set(HaCam::new()?).is_err() {
                tx.send(CamOutMessage::Error(
                    "Camera cannot be initialized twice!".to_owned(),
                ))
                .await?;
            }

            let Some(cam) = cam.get_mut() else {
                tx.send(CamOutMessage::Error(
                    "Camera initialization error!".to_owned(),
                ))
                .await?;
                continue;
            };

            cam.initialize_comm().await?;

            continue;
        }

        let Some(cam) = cam.get_mut() else {
            tx.send(CamOutMessage::Error(
                "Camera initialization error!".to_owned(),
            ))
            .await?;
            continue;
        };

        match in_msg {
            CamInMessage::StartLiveView(live_view_resolution) => {
                cam.start_live_view(live_view_resolution).await?;
                lv_res.replace(live_view_resolution);
                was_lv_initialized.set(())?;
            }
            CamInMessage::StopLiveView => {
                cam.stop_live_view().await?;
            }
            CamInMessage::StartRecording => {
                cam.start_recording().await?;
            }
            CamInMessage::StopRecording => {
                cam.start_recording().await?;
            }
            CamInMessage::TakePhoto { orientation } => {
                let on_thumbnail = |thumb| {
                    tx.blocking_send(CamOutMessage::Frame {
                        frame: CamFrame {
                            frame: thumb,
                            width: 272,
                            height: 272,
                        },
                        typ: CamFrameType::Thumbnail,
                        thermal_status: None,
                    })
                    .unwrap();
                };

                let photo_res = PhotoResolution::try_from(
                    cam.read_setting(settings::SettingType::PhotoResolution)
                        .await? as i8,
                )
                .expect("Invalid setting format received!");

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
