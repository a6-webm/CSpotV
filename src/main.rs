use std::{
    fmt::Display,
    fs,
    io::{self, Write},
    path::PathBuf,
};

use anyhow::Context;
use clap::{command, Parser, Subcommand};
use log::{info, warn};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use spotify_rs::{
    auth::{NoVerifier, Token},
    client::Client,
    model::{search::Item, track::Track},
    AuthCodeClient, AuthCodeFlow, ClientCredsClient, ClientCredsFlow, RedirectUrl,
};

const CLIENT_ID: &str = "fed3e6de8e3e4fe481b4020cdb72342e";
const CLIENT_SECRET_PATH: &str = "client_secret.txt";

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
    fn to_map_record<T: Into<String>>(&self, sp_id: T) -> MapRecord {
        MapRecord {
            name: self.name.to_owned(),
            album: self.album.to_owned(),
            artist: self.artist.to_owned(),
            sp_id: sp_id.into(),
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
    let scopes = vec!["playlist-read-private", "playlist-modify-private"];
    let auth_code_flow = AuthCodeFlow::new(
        CLIENT_ID,
        fs::read_to_string(CLIENT_SECRET_PATH)?.trim(),
        scopes,
    );

    let (auth_client, url) = AuthCodeClient::new(auth_code_flow, redirect_url, true);
    println!("Enter the following url into a browser:\n\n\t{}\n", url);
    // TODO see if u can store the auth stuff and only refresh it if its invalid
    // TODO use webbrowser crate to automatically open the url or make an http request directly
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
                "line {} in {} has empty Name field",
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
            m_r: lib_r.to_map_record(search_results.items[index].id.clone()),
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
                    "line {}, \"{}\" removed from map as not present in lib",
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

async fn check(_map_path: PathBuf) -> anyhow::Result<()> {
    // let map: Vec<MapRecord> = collect_csv(&map_path)?;
    // let mut cred_sp = get_cred_sp().await?;
    todo!()
}

async fn upload(_map_path: PathBuf) -> anyhow::Result<()> {
    // let map: Vec<MapRecord> = collect_csv(&map_path)?;
    // let mut cred_sp = get_cred_sp().await?;
    // let mut authc_sp = get_authc_sp().await?;
    todo!()
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
        Commands::Upload { map_path } => upload(map_path).await,
    }?;
    Ok(())
}
