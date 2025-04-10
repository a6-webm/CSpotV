use clap::{command, Parser, Subcommand};
use log::{error, info, warn};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use spotify::{get_all_playlist_tracks, get_authc_sp, get_cred_sp};
use spotify_rs::model::track::Track;
use std::{
    fmt::Display,
    io::{self, Write},
    path::PathBuf,
};

mod map;
mod spotify;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Map {
        /// .csv file containing songs from your library
        #[arg(value_name = "LIBRARY_FILE")]
        lib_path: PathBuf,
        /// .csv file containing mappings from songs to spotify songs
        #[arg(value_name = "MAP_FILE")]
        map_path: PathBuf,
    },
    Check {
        /// .csv file containing mappings from songs to spotify songs
        #[arg(value_name = "MAP_FILE")]
        map_path: PathBuf,
    },
    Upload {
        /// .csv file containing mappings from songs to spotify songs
        #[arg(value_name = "MAP_FILE")]
        map_path: PathBuf,
        /// id of the playlist you want to update
        #[arg(value_name = "PLAYLIST_ID")]
        playlist_id: String,
    },
}

#[derive(Debug, Deserialize, Clone)]
struct LibRec {
    name: String,
    album: String,
    artist: String,
}

impl Display for LibRec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Name: {}\nAlbum: {}\nArtist: {}",
            self.name, self.album, self.artist
        )
    }
}

impl LibRec {
    fn to_map_record(&self, sp_id: &str) -> MapRec {
        MapRec {
            name: self.name.to_owned(),
            album: self.album.to_owned(),
            artist: self.artist.to_owned(),
            sp_id: sp_id.to_owned(),
        }
    }

    fn matches_track(&self, tr: &Track) -> bool {
        return self.name.trim().to_lowercase() == tr.name.trim().to_lowercase()
            && self.album.trim().to_lowercase() == tr.album.name.trim().to_lowercase()
            && tr.artists.first().map_or(false, |a| {
                a.name.trim().to_lowercase() == self.artist.trim().to_lowercase()
            });
    }

    fn search_str(&self) -> String {
        let mut out = String::new();
        if !self.name.trim().is_empty() {
            out += "track:";
            out += &self.name.replace(" ", "+");
            out += " ";
        }
        if !self.album.trim().is_empty() {
            out += "album:";
            out += &self.album.replace(" ", "+");
            out += " ";
        }
        if !self.artist.trim().is_empty() {
            out += "artist:";
            out += &self.artist.replace(" ", "+");
            out += " ";
        }
        if !out.is_empty() {
            out.remove(out.len() - 1);
        }
        out
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct MapRec {
    name: String,
    album: String,
    artist: String,
    sp_id: String,
}

impl MapRec {
    fn matches(&self, lib_r: &LibRec) -> bool {
        self.name == lib_r.name && self.album == lib_r.album && self.artist == lib_r.artist
    }
}

fn collect_csv<T: DeserializeOwned>(path: &PathBuf, headers: bool) -> anyhow::Result<Vec<T>> {
    let rdr = csv::ReaderBuilder::new()
        .has_headers(headers)
        .from_path(path)?;
    Ok(rdr
        .into_deserialize()
        .collect::<Result<Vec<T>, csv::Error>>()?)
}

async fn check(map_path: PathBuf) -> anyhow::Result<()> {
    let map: Vec<MapRec> = collect_csv(&map_path, true)?;
    let mut cred_sp = get_cred_sp().await?;

    for (ind, m_r) in map.iter().enumerate() {
        if m_r.sp_id == "Not found" {
            warn!("line {}, \"{}\" has id \"Not found\"", ind + 1, m_r.name);
        } else if cred_sp.track(&m_r.sp_id).get().await.is_err() {
            error!(
                "line {}, \"{}\" has invalid id \"{}\"",
                ind + 1,
                m_r.name,
                m_r.sp_id
            );
        }
    }
    Ok(())
}

async fn upload(map_path: PathBuf, playlist_id: &str) -> anyhow::Result<()> {
    struct R {
        m_r: MapRec,
        in_pl: bool,
    }
    let mut map: Vec<R> = if map_path.exists() {
        collect_csv(&map_path, true)?
            .into_iter()
            .map(|m_r| R { m_r, in_pl: false })
            .collect()
    } else {
        Vec::new()
    };
    let mut authc_sp = get_authc_sp().await?;

    let playlist = get_all_playlist_tracks(&mut authc_sp, playlist_id).await?;

    let mut to_remove: Vec<String> = Vec::new();
    for pl_tr in playlist {
        if let Some(r) = map.iter_mut().find(|r| r.m_r.sp_id == pl_tr.id) {
            r.in_pl = true;
        } else {
            to_remove.push(String::from("spotify:track:") + &pl_tr.id);
            info!(
                "playlist item {}, \"{}\" not in map, will remove from playlist",
                pl_tr.pos + 1,
                pl_tr.name,
            );
        }
    }

    let to_add: Vec<String> = map
        .into_iter()
        .enumerate()
        .filter_map(|(map_ind, R { m_r, in_pl })| {
            if in_pl || m_r.sp_id == "Not found" {
                None
            } else {
                info!(
                    "map line {}, \"{}\" not in playlist, will add to playlist",
                    map_ind + 1,
                    m_r.name
                );
                Some(String::from("spotify:track:") + &m_r.sp_id)
            }
        })
        .collect();

    if to_add.is_empty() && to_remove.is_empty() {
        info!("Nothing to change, quitting...");
        return Ok(());
    }

    print!("Proceed? (y/N): ");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    if answer.trim().to_lowercase() != "y" {
        info!("Aborting upload...");
        return Ok(());
    }

    const SEND_LIM: usize = 100;
    for chunk in to_remove.chunks(SEND_LIM) {
        info!("Removing...");
        authc_sp
            .remove_playlist_items(playlist_id, chunk)
            .send()
            .await?;
    }
    for chunk in to_add.chunks(SEND_LIM) {
        info!("Adding...");
        authc_sp
            .add_items_to_playlist(playlist_id, chunk)
            .send()
            .await?;
    }

    info!("Upload complete");

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // TODO make errors not look like ass
    // TODO maybe use console, dialoguer and indicatif crates

    colog::init();
    let cli = Cli::parse();
    match cli.command {
        Commands::Map { lib_path, map_path } => map::map(lib_path, map_path).await,
        Commands::Check { map_path } => check(map_path).await,
        Commands::Upload {
            map_path,
            playlist_id,
        } => upload(map_path, &playlist_id).await,
    }?;
    Ok(())
}
