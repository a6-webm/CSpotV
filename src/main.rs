use std::{
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

#[derive(Debug, Deserialize)]
struct LibRecord {
    name: String,
    album: String,
    artist: String,
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
}

#[derive(Debug, Serialize, Deserialize)]
struct MapRecord {
    name: String,
    album: String,
    artist: String,
    sp_id: String,
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
    },
}

fn collect_csv<T: DeserializeOwned>(path: &PathBuf) -> anyhow::Result<Vec<T>> {
    let rdr = csv::Reader::from_path(path)?;
    Ok(rdr
        .into_deserialize()
        .collect::<Result<Vec<T>, csv::Error>>()?)
}

fn metadata_match(map_r: &MapRecord, lib_r: &LibRecord) -> bool {
    map_r.name == lib_r.name && map_r.album == lib_r.album && map_r.artist == lib_r.artist
}

fn search_str(lib_r: &LibRecord) -> String {
    let mut out = String::new();
    if !lib_r.name.trim().is_empty() {
        out += "track:";
        out += &lib_r.name;
        out += " ";
    }
    if !lib_r.album.trim().is_empty() {
        out += "album:";
        out += &lib_r.album;
        out += " ";
    }
    if !lib_r.artist.trim().is_empty() {
        out += "artist:";
        out += &lib_r.artist;
        out += " ";
    }
    if !out.is_empty() {
        out.remove(out.len() - 1);
    }
    out
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

async fn map(
    lib_path: PathBuf,
    map_path: PathBuf,
    mut cred_sp: Client<Token, ClientCredsFlow, NoVerifier>,
) -> anyhow::Result<()> {
    struct R {
        m_r: MapRecord,
        keep: bool,
    }
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
                "line {lib_index} in {} has empty Name field",
                lib_path.file_name().unwrap().to_string_lossy()
            );
            continue;
        }
        // if lib_r already present in map
        if let Some(map_r) = map
            .iter_mut()
            .find(|map_r| metadata_match(&map_r.m_r, &lib_r))
        {
            info!(
                "line {lib_index}, \"{}\" already present in map",
                map_r.m_r.name
            );
            map_r.keep = true;
            continue;
        }
        // else add lib_r to map
        // TODO is there an error if the query contains a colon? (generally do we/how do we escape stuff)
        // TODO info abt searching
        let search_results = cred_sp
            .search(search_str(&lib_r), &[Item::Track])
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
            warn!("line {lib_index}, \"{}\" not found on spotify", lib_r.name);
            continue;
        }
        for (i, item) in search_results.items.iter().enumerate() {
            println!("Search result {}", i + 1);
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
            continue;
        }
        let index = answer.parse::<usize>()? - 1;
        // TODO info abt this
        map.push(R {
            m_r: lib_r.to_map_record(search_results.items[index].id.clone()),
            keep: true,
        });
    }

    // Remove entries from map that are not present in lib
    // TODO warn abt these
    let mut map: Vec<MapRecord> = map
        .into_iter()
        .filter_map(|m_r| if m_r.keep { Some(m_r.m_r) } else { None })
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

async fn check(
    _map_path: PathBuf,
    _cred_sp: Client<Token, ClientCredsFlow, NoVerifier>,
) -> anyhow::Result<()> {
    // let map: Vec<MapRecord> = collect_csv(&map_path)?;
    todo!()
}

async fn upload(
    _map_path: PathBuf,
    _cred_sp: Client<Token, ClientCredsFlow, NoVerifier>,
    _authc_sp: Client<Token, AuthCodeFlow, NoVerifier>,
) -> anyhow::Result<()> {
    // let map: Vec<MapRecord> = collect_csv(&map_path)?;
    todo!()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    colog::init();
    // TODO make errors not look like ass
    let cli = Cli::parse();

    let redirect_url = RedirectUrl::new("http://localhost/".to_owned())?;
    let auto_refresh = true;
    let scopes = vec!["playlist-read-private", "playlist-modify-private"];
    let auth_code_flow = AuthCodeFlow::new(
        "fed3e6de8e3e4fe481b4020cdb72342e",
        fs::read_to_string("client_secret.txt")?.trim(),
        scopes,
    );

    let client_creds_flow = ClientCredsFlow::new(
        "fed3e6de8e3e4fe481b4020cdb72342e",
        fs::read_to_string("client_secret.txt")?.trim(),
    );
    let cred_sp = ClientCredsClient::authenticate(client_creds_flow).await?;

    let (auth_client, url) = AuthCodeClient::new(auth_code_flow, redirect_url, auto_refresh);
    println!("Enter the following url into a browser:\n\n\t{}\n", url);
    // TODO store the auth stuff and only refresh it if its invalid
    // TODO use webbrowser crate to automatically open the url
    print!("Then paste the resuting localhost url here: ");
    io::stdout().flush()?;
    let mut auth_url = String::new();
    io::stdin().read_line(&mut auth_url)?;
    println!("\n");
    let split: Vec<&str> = auth_url.trim().split(['=', '&']).collect();
    let auth_code = split[1].to_owned();
    let csrf_token = split[3].to_owned();
    let authc_sp = auth_client.authenticate(auth_code, csrf_token).await?;

    // TODO use console, dialoguer and indicatif crates
    match cli.command {
        Commands::Map { lib_path, map_path } => map(lib_path, map_path, cred_sp).await,
        Commands::Check { map_path } => check(map_path, cred_sp).await,
        Commands::Upload { map_path } => upload(map_path, cred_sp, authc_sp).await,
    }?;
    Ok(())
}
