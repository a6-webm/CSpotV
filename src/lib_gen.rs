use anyhow::anyhow;
use csv::Writer;
use log::warn;
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
};
use symphonia::core::{io::MediaSourceStream, meta::StandardTagKey, probe::Hint};

use crate::LibRec;

fn get_metadata(path: &Path) -> anyhow::Result<LibRec> {
    let extension = path.extension();
    let src = std::fs::File::open(&path)?;
    let mss = MediaSourceStream::new(Box::new(src), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = extension {
        if let Some(ext_str) = ext.to_str() {
            hint.with_extension(ext_str);
        }
    }
    let mut probed = symphonia::default::get_probe().format(
        &hint,
        mss,
        &Default::default(),
        &Default::default(),
    )?;
    let mut metadata = match probed.metadata.get() {
        Some(m) => m,
        None => probed.format.metadata(),
    };
    let tags = metadata
        .skip_to_latest()
        .ok_or(anyhow!("Could not get current metadata from file"))?
        .tags();

    let get_tag_str_val = |tags: &[symphonia::core::meta::Tag], tag_target| -> String {
        let val = tags
            .iter()
            .find(|t| t.std_key.map_or(false, |t| t == tag_target));
        if val.is_none() {
            return String::new();
        }
        if let symphonia::core::meta::Value::String(ref str_val) = val.unwrap().value {
            return str_val.to_owned();
        }
        return String::new();
    };
    if get_tag_str_val(tags, StandardTagKey::TrackTitle).is_empty() {
        dbg!(&path);
    }
    Ok(LibRec {
        name: get_tag_str_val(tags, StandardTagKey::TrackTitle),
        album: get_tag_str_val(tags, StandardTagKey::Album),
        artist: get_tag_str_val(tags, StandardTagKey::Artist),
    })
}

fn write_all_metadata(dir: &Path, wtr: &mut Writer<File>) -> anyhow::Result<()> {
    // TODO turn off symphonia logging
    if dir.is_dir() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                write_all_metadata(&path, wtr)?;
            } else {
                match get_metadata(&path) {
                    Ok(rec) => wtr.serialize(rec)?,
                    Err(err) => warn!("{}: {}", path.to_string_lossy(), err),
                }
            }
        }
    }
    Ok(())
}

pub fn gen_lib(music_path: PathBuf, lib_path: PathBuf) -> anyhow::Result<()> {
    let mut wtr = csv::Writer::from_path(&lib_path)?;
    write_all_metadata(&music_path, &mut wtr)?;
    Ok(())
}
