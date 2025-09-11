use std::{fs::File, io::{Read, Seek, SeekFrom, Write}, path::PathBuf, str::FromStr as _, sync::mpsc::Receiver};

use mp4_atom::{Atom as _, FourCC, ReadFrom as _, WriteTo as _};
use uuid::Uuid;

pub const SPHERICAL_XML_BOX_UUID: Uuid = Uuid::from_fields(0xffcc8263, 0xf855, 0x4a93, &[0x88, 0x14, 0x58, 0x7a, 0x02, 0x52, 0x1f, 0xdd]);
pub const GENERIC_SPHERICAL_XML: &[u8; 430] = indoc::indoc! {br##"
   <rdf:SphericalVideo 
       xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
       xmlns:GSpherical="http://ns.google.com/videos/1.0/spherical/"
   > 
       <GSpherical:Spherical>true</GSpherical:Spherical> 
       <GSpherical:Stitched>true</GSpherical:Stitched> 
       <GSpherical:ProjectionType>equirectangular</GSpherical:ProjectionType> 
       <GSpherical:StitchingSoftware>unknown</GSpherical:StitchingSoftware> 
   </rdf:SphericalVideo>
"##};

/// Saves the H264 data to the output MP4 file.
pub fn save_mp4(
    frame_rx: &mut Receiver<Option<Vec<u8>>>,
    output_file_path: PathBuf,
    width: i32,
    height: i32,
) -> anyhow::Result<()> {
    // This function will be improved in the future by using the mp4_atom crate directly 
    // to write the mp4 file, instead of using both.

    let mut mp4muxer = minimp4::Mp4Muxer::new(File::create(output_file_path)?);

    mp4muxer.init_video(width, height, false, "video");

    while let Ok(Some(frame)) = frame_rx.recv() {
        mp4muxer.write_video_with_fps(&frame, 30);
    }

    mp4muxer.close();

    Ok(())
}

/// Saves the H264 data to the output MP4 file with spherical metadata.
/// Saves the file to a temporary location, injects the metadata, then copies the file back.
pub fn save_mp4_with_spherical(
    frame_rx: &mut Receiver<Option<Vec<u8>>>,
    output_file_path: PathBuf,
    width: i32,
    height: i32,
) -> anyhow::Result<()> {
    let temp_file_path = std::env::temp_dir()
        .with_file_name(format!("hacam-temp-{}.mp4", Uuid::new_v4()));

    let mut mp4muxer = minimp4::Mp4Muxer::new(File::create(&temp_file_path)?);

    mp4muxer.init_video(width, height, false, "video");

    while let Ok(Some(frame)) = frame_rx.recv() {
        mp4muxer.write_video_with_fps(&frame, 30);
    }

    mp4muxer.close();

    inject_spherical_metadata(File::open(&temp_file_path)?, File::create(output_file_path)?)?;

    Ok(())
}

/// Injects spherical metadata to the MP4 files.
pub fn inject_spherical_metadata(mut input: impl Read, mut out: impl Write + Seek) -> anyhow::Result<()> {
    // Copies all the MP4 atoms from the input to the output,
    // but if it encounters the `trak` atom (with video type) of the `moov` atom,
    // it writes the `trak` atom as is and also adds the `uuid` spherical metadata atom. 
    while let Some(atom) = Option::<mp4_atom::Any>::read_from(&mut input)? {
        match atom {
            mp4_atom::Any::Moov(m) => {
                let start = out.stream_position()?;
                0u32.write_to(&mut out)?;
                mp4_atom::Moov::KIND.write_to(&mut out)?;

                m.mvhd.write_to(&mut out)?;
                m.udta.write_to(&mut out)?;
                m.mvex.write_to(&mut out)?;

                for trak in m.trak {
                    let trk_start = out.stream_position()?;
                    0u32.write_to(&mut out)?;
                    mp4_atom::Trak::KIND.write_to(&mut out)?;
                    trak.tkhd.write_to(&mut out)?;
                    trak.edts.write_to(&mut out)?;
                    trak.mdia.write_to(&mut out)?;

                    if trak.mdia.hdlr.handler == mp4_atom::FourCC::new(b"vide") {
                        let mut spherical_xml = SPHERICAL_XML_BOX_UUID.as_bytes().to_vec();

                        spherical_xml.extend_from_slice(GENERIC_SPHERICAL_XML);

                        mp4_atom::Any::Unknown(FourCC::new(b"uuid"), spherical_xml)
                            .write_to(&mut out)?;
                    }

                    let trk_end = out.stream_position()?;
                    let trk_size: u32 = (trk_end - trk_start).try_into()?;
                    out.seek(SeekFrom::Start(trk_start))?;
                    trk_size.write_to(&mut out)?;
                    out.seek(SeekFrom::Start(trk_end))?;
                }

                let end = out.stream_position()?;
                let size: u32 = (end - start).try_into()?;

                out.seek(SeekFrom::Start(start))?;
                size.write_to(&mut out)?;
                out.seek(SeekFrom::Start(end))?;
            }
            any => {
                any.write_to(&mut out)?;
            }
        }
    }

    Ok(())
}
