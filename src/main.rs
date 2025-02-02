use std::{
    fmt::Display,
    fs,
    io::{self, Write},
    path::PathBuf,
};

use anyhow::Context;
use clap::{command, Parser, Subcommand};
use log::{error, info, warn};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use spotify_rs::{
    auth::{NoVerifier, Token},
    client::Client,
    model::{search::Item, track::Track, PlayableItem},
    AuthCodeClient, AuthCodeFlow, ClientCredsClient, ClientCredsFlow, RedirectUrl,
};

const CLIENT_ID: &str = "fed3e6de8e3e4fe481b4020cdb72342e";
const CLIENT_SECRET_PATH: &str = "client_secret.txt";

struct Tr {
    name: String,
    id: String,
    pos: u32,
}

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

#[derive(Debug, Deserialize)]
struct LibRecord {
    name: String,
    album: String,
    artist: String,
}

impl Display for LibRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Name: {}\nAlbum: {}\nArtist: {}",
            self.name, self.album, self.artist
        )
    }
}

impl LibRecord {
    fn to_map_record(&self, sp_id: &str) -> MapRecord {
        MapRecord {
            name: self.name.to_owned(),
            album: self.album.to_owned(),
            artist: self.artist.to_owned(),
            sp_id: sp_id.to_owned(),
        }
    }

    fn search_str(&self) -> String {
        let mut out = String::new();
        if !self.name.trim().is_empty() {
            out += "track:";
            out += &self.name;
            out += " ";
        }
        if !self.album.trim().is_empty() {
            out += "album:";
            out += &self.album;
            out += " ";
        }
        if !self.artist.trim().is_empty() {
            out += "artist:";
            out += &self.artist;
            out += " ";
        }
        if !out.is_empty() {
            out.remove(out.len() - 1);
        }
        out
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct MapRecord {
    name: String,
    album: String,
    artist: String,
    sp_id: String,
}

impl MapRecord {
    fn matches(&self, lib_r: &LibRecord) -> bool {
        self.name == lib_r.name && self.album == lib_r.album && self.artist == lib_r.artist
    }
}

fn collect_csv<T: DeserializeOwned>(path: &PathBuf) -> anyhow::Result<Vec<T>> {
    let rdr = csv::Reader::from_path(path)?;
    Ok(rdr
        .into_deserialize()
        .collect::<Result<Vec<T>, csv::Error>>()?)
}

fn print_track(track: &Track) {
    println!("Name: {}", track.name);
    println!("Album: {}", track.album.name);
    let artists: Vec<String> = track
        .artists
        .iter()
        .map(|artist| artist.name.to_owned())
        .collect();
    if artists.len() == 1 {
        println!("Artist: {}", artists[0]);
    } else {
        println!("Artist: {:?}", artists);
    }
    println!("Date: {}", track.album.release_date);
}

async fn get_cred_sp() -> anyhow::Result<Client<Token, ClientCredsFlow, NoVerifier>> {
    let client_creds_flow =
        ClientCredsFlow::new(CLIENT_ID, fs::read_to_string(CLIENT_SECRET_PATH)?.trim());
    Ok(ClientCredsClient::authenticate(client_creds_flow).await?)
}

async fn get_authc_sp() -> anyhow::Result<Client<Token, AuthCodeFlow, NoVerifier>> {
    let redirect_url = RedirectUrl::new("http://localhost/".to_owned())?;
    let scopes = vec![
        "playlist-read-private",
        "playlist-modify-private",
        "playlist-modify-public",
    ];
    let auth_code_flow = AuthCodeFlow::new(
        CLIENT_ID,
        fs::read_to_string(CLIENT_SECRET_PATH)?.trim(),
        scopes,
    );

    let (auth_client, url) = AuthCodeClient::new(auth_code_flow, redirect_url, true);
    println!("Enter the following url into a browser:\n\n\t{}\n", url);
    // TODO see if u can store the auth stuff and only refresh it if its invalid
    // TODO host a page with hyper, open the url with webbrowser, and get the auth automatically
    print!("Then paste the resuting localhost url here: ");
    io::stdout().flush()?;
    let mut auth_url = String::new();
    io::stdin().read_line(&mut auth_url)?;
    println!("\n");
    let split: Vec<&str> = auth_url.trim().split(['=', '&']).collect();
    let auth_code = split[1].to_owned();
    let csrf_token = split[3].to_owned();
    Ok(auth_client.authenticate(auth_code, csrf_token).await?)
}

async fn map(lib_path: PathBuf, map_path: PathBuf) -> anyhow::Result<()> {
    // TODO redo this so that it writes stuff to a tmp file so u can resume later if u abort
    struct R {
        m_r: MapRecord,
        keep: bool,
    }
    let mut cred_sp = get_cred_sp().await?;

    let lib: Vec<LibRecord> = collect_csv(&lib_path)?;
    let mut map: Vec<R> = if map_path.exists() {
        collect_csv(&map_path)?
            .into_iter()
            .map(|m_r| R { m_r, keep: false })
            .collect()
    } else {
        Vec::new()
    };

    for (lib_index, lib_r) in lib.iter().enumerate() {
        if lib_r.name.trim().is_empty() {
            warn!(
                "line {} in {} has empty Name field, skipping...",
                lib_index + 1,
                lib_path.file_name().unwrap().to_string_lossy()
            );
            continue;
        }
        // if lib_r already present in map
        if let Some(r) = map.iter_mut().find(|r| r.m_r.matches(lib_r)) {
            r.keep = true;
            info!(
                "line {}, \"{}\" already present in map",
                lib_index + 1,
                r.m_r.name
            );
            continue;
        }
        // else add lib_r to map
        // TODO is there an error if the query contains a colon? (generally do we/how do we escape stuff)
        let search_results = cred_sp
            .search(lib_r.search_str(), &[Item::Track])
            .market("GB")
            .limit(5)
            .get()
            .await?
            .tracks
            .with_context(|| "spotify search failed")?;
        if search_results.items.is_empty() {
            map.push(R {
                m_r: lib_r.to_map_record("Not found"),
                keep: true,
            });
            warn!(
                "line {}, \"{}\" not found by spotify search",
                lib_index + 1,
                lib_r.name
            );
            continue;
        }
        println!("== Track to match ==");
        println!("{lib_r}\n");
        println!("== Search results ==");
        for (i, item) in search_results.items.iter().enumerate() {
            println!("= Search result {} =", i + 1);
            print_track(item);
            println!();
        }
        print!("Pick a track to match (#/n): ");
        io::stdout().flush()?;
        let mut answer = String::new();
        io::stdin().read_line(&mut answer)?;
        println!("\n");
        answer = answer.trim().to_owned();
        if answer == "n" {
            map.push(R {
                m_r: lib_r.to_map_record("Not found"),
                keep: true,
            });
            info!(
                "line {}, \"{}\" added as \"Not found\"",
                lib_index + 1,
                lib_r.name,
            );
            continue;
        }
        let index = answer.parse::<usize>()? - 1;
        map.push(R {
            m_r: lib_r.to_map_record(&search_results.items[index].id),
            keep: true,
        });
        info!(
            "line {}, \"{}\" added with id: {}",
            lib_index + 1,
            lib_r.name,
            search_results.items[index].id.clone()
        );
    }

    // Remove entries from map that are not present in lib
    let mut map: Vec<MapRecord> = map
        .into_iter()
        .enumerate()
        .filter_map(|(map_index, r)| {
            if r.keep {
                Some(r.m_r)
            } else {
                warn!(
                    "map line {}, \"{}\" removed as not present in lib",
                    map_index + 1,
                    r.m_r.name
                );
                None
            }
        })
        .collect();
    map.sort_by_key(|m_r| m_r.name.clone());
    map.sort_by_key(|m_r| m_r.album.clone());
    map.sort_by_key(|m_r| m_r.artist.clone());

    let mut wtr = csv::Writer::from_path(&map_path)?;
    for map_r in map.into_iter() {
        wtr.serialize(map_r)?;
    }
    wtr.flush()?;
    Ok(())
}

async fn check(map_path: PathBuf) -> anyhow::Result<()> {
    let map: Vec<MapRecord> = collect_csv(&map_path)?;
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

async fn get_all_playlist_tracks(
    authc_sp: &mut Client<Token, AuthCodeFlow, NoVerifier>,
    playlist_id: &str,
) -> anyhow::Result<Vec<Tr>> {
    let mut playlist_items = Vec::new();
    let limit: u32 = 50;
    let total = authc_sp.playlist_items(playlist_id).get().await?.total;
    for offset in (0..total).step_by(limit as usize) {
        let page = authc_sp
            .playlist_items(playlist_id)
            .limit(limit)
            .offset(offset)
            .get()
            .await?;
        page.items
            .into_iter()
            .enumerate()
            .map(|(ind, pi)| match pi.track {
                PlayableItem::Track(track) => Tr {
                    name: track.name,
                    id: track.id,
                    pos: offset + ind as u32,
                },
                PlayableItem::Episode(_) => Tr {
                    name: String::new(),
                    id: String::from("episode"),
                    pos: offset + ind as u32,
                },
            })
            .for_each(|id| playlist_items.push(id));
    }
    Ok(playlist_items)
}

async fn upload(map_path: PathBuf, playlist_id: &str) -> anyhow::Result<()> {
    struct R {
        m_r: MapRecord,
        in_pl: bool,
    }
    let mut map: Vec<R> = if map_path.exists() {
        collect_csv(&map_path)?
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

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // TODO make errors not look like ass
    // TODO maybe use console, dialoguer and indicatif crates

    colog::init();
    let cli = Cli::parse();
    match cli.command {
        Commands::Map { lib_path, map_path } => map(lib_path, map_path).await,
        Commands::Check { map_path } => check(map_path).await,
        Commands::Upload {
            map_path,
            playlist_id,
        } => upload(map_path, &playlist_id).await,
    }?;
    Ok(())
}
