use std::{fs::File, path::PathBuf, sync::mpsc::Receiver};

/// Receives frame using channel.
pub fn save_mp4(frame_rx: Receiver<Vec<u8>>,  file: PathBuf, width: i32, height: i32) -> anyhow::Result<()> {
    let mut mp4muxer = minimp4::Mp4Muxer::new(
        File::create(file)?
    );
    
    mp4muxer.init_video(width, height, false, "video");

    while let Ok(frame) = frame_rx.recv() {
        mp4muxer.write_video_with_fps(&frame, 30);
    }

    mp4muxer.close();

    Ok(())
}