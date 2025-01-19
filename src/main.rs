use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
};

use clap::{command, Parser, Subcommand};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use spotify_rs::{
    auth::{NoVerifier, Token},
    client::Client,
    model::search::Item,
    AuthCodeClient, AuthCodeFlow, ClientCredsClient, ClientCredsFlow, RedirectUrl,
};

#[derive(Debug, Deserialize)]
struct LibRecord {
    name: String,
    album: String,
    artist: String,
}

impl LibRecord {
    fn to_map_record<T: Into<String>>(self, sp_id: T) -> MapRecord {
        MapRecord {
            name: self.name,
            album: self.album,
            artist: self.artist,
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

async fn map(
    lib_path: PathBuf,
    map_path: PathBuf,
    mut cred_sp: Client<Token, ClientCredsFlow, NoVerifier>,
) -> anyhow::Result<()> {
    let lib: Vec<LibRecord> = collect_csv(&lib_path)?;
    let mut map: Vec<(MapRecord, bool)> = if map_path.exists() {
        collect_csv(&map_path)?
            .into_iter()
            .map(|m_r| (m_r, false))
            .collect()
    } else {
        Vec::new()
    };

    for lib_r in lib {
        // if lib_r already present in map
        if let Some(map_r) = map
            .iter_mut()
            .find(|map_r| metadata_match(&map_r.0, &lib_r))
        {
            map_r.1 = true;
            continue;
        }
        // else add lib_r to map
        // TODO is there an error if the query contains a colon?
        let search_results = cred_sp
            .search(
                format!(
                    "track:{} album:{} artist:{}",
                    lib_r.name, lib_r.album, lib_r.artist
                ),
                &[Item::Track],
            )
            .market("GB")
            .limit(5)
            .get()
            .await?
            .tracks;
        let Some(search_results) = search_results else {
            map.push((lib_r.to_map_record("Not found"), true));
            continue;
        };
        println!("Pick a track to match:");
        // TODO
    }

    // Remove entries from map that are not present in lib
    let mut map: Vec<MapRecord> = map
        .into_iter()
        .filter_map(|m_r| if m_r.1 { Some(m_r.0) } else { None })
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
    map_path: PathBuf,
    cred_sp: Client<Token, ClientCredsFlow, NoVerifier>,
) -> anyhow::Result<()> {
    let map: Vec<MapRecord> = collect_csv(&map_path)?;
    todo!()
}

async fn upload(
    map_path: PathBuf,
    cred_sp: Client<Token, ClientCredsFlow, NoVerifier>,
    authc_sp: Client<Token, AuthCodeFlow, NoVerifier>,
) -> anyhow::Result<()> {
    let map: Vec<MapRecord> = collect_csv(&map_path)?;
    todo!()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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
    let stdin = io::stdin();
    stdin.read_line(&mut auth_url)?;
    let split: Vec<&str> = auth_url.trim().split(['=', '&']).collect();
    let auth_code = split[1].to_owned();
    let csrf_token = split[3].to_owned();
    let authc_sp = auth_client.authenticate(auth_code, csrf_token).await?;

    // dbg!(
    //     cred_sp
    //         .search("track::) artist:japanese", &[Item::Track])
    //         .market("GB")
    //         .limit(5)
    //         .get()
    //         .await?
    //         .tracks
    //         .unwrap()
    //         .items
    // );

    // TODO use console, dialoguer and indicatif crates
    match cli.command {
        Commands::Map { lib_path, map_path } => map(lib_path, map_path, cred_sp).await,
        Commands::Check { map_path } => check(map_path, cred_sp).await,
        Commands::Upload { map_path } => upload(map_path, cred_sp, authc_sp).await,
    }?;
    Ok(())
}
