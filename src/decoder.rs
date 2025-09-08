use std::{
    sync::{
        mpsc::{Receiver, SyncSender},
    },
};

use anyhow::Result;
use openh264::formats::YUVSource;
use yuv::{yuv420_to_bgra, YuvPlanarImage, YuvRange, YuvStandardMatrix};
use hacam_lib_rs::{cam::LiveViewFrame};
use crate::cam::CamFrame;

pub fn run_cam_lv(vid_rx: Receiver<LiveViewFrame>, frame_tx: SyncSender<CamFrame>) -> Result<()> {
    let mut decoder = openh264::decoder::Decoder::new()?;

    loop {
        let LiveViewFrame { data: vid, .. } = vid_rx.recv()?;

        if vid.is_empty() {
            continue;
        }

        for packet in openh264::nal_units(&vid) {
            let decoded_res = decoder.decode(packet);

            if let Ok(Some(yuv_frame)) = decoded_res {
                let (w, h) = yuv_frame.dimensions();
                let (y, u, v) = yuv_frame.strides();

                let yuv_image = YuvPlanarImage {
                    y_plane: yuv_frame.y(),
                    y_stride: y as u32,
                    u_plane: yuv_frame.u(),
                    u_stride: u as u32,
                    v_plane: yuv_frame.v(),
                    v_stride: v as u32,
                    width: w as u32,
                    height: h as u32,
                };

                let mut bgra_image = vec![0; w * h * 4];

                yuv420_to_bgra(
                    &yuv_image, 
                    &mut bgra_image, 
                    4 * w as u32, 
                    YuvRange::Full, 
                    YuvStandardMatrix::Bt601
                )?;

                let frame = CamFrame {
                    frame: bgra_image,
                    width: w,
                    height: h,
                };

                if let Err(e) = frame_tx.send(frame) {
                    error!("Can't send decoded frame: {e}");
                } else {
                    trace!("Decoded frame sent!");
                }
            }
        }
    }
}