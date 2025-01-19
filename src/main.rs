use std::{
    error::Error,
    fs,
    io::{self, Write},
    path::PathBuf,
};

use clap::{command, Parser, Subcommand};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use spotify_rs::{
    model::search::Item, AuthCodeClient, AuthCodeFlow, ClientCredsClient, ClientCredsFlow,
    RedirectUrl,
};

// TODO does the following matter
//     By default, struct field names are deserialized based on the position of
//     a corresponding field in the CSV data's header record.

#[derive(Debug, Deserialize)]
struct LibRecord {
    name: String,
    album: String,
    artist: String,
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

fn collect_csv<T: DeserializeOwned>(path: &PathBuf) -> Result<Vec<T>, csv::Error> {
    let rdr = csv::Reader::from_path(path)?;
    rdr.into_deserialize()
        .collect::<Result<Vec<T>, csv::Error>>()
}

fn metadata_match(map_r: &MapRecord, lib_r: &LibRecord) -> bool {
    map_r.name == lib_r.name && map_r.album == lib_r.album && map_r.artist == lib_r.artist
}

async fn map(lib_path: PathBuf, map_path: PathBuf) -> Result<(), Box<dyn Error>> {
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
        if let Some(map_r) = map
            .iter_mut()
            .find(|map_r| metadata_match(&map_r.0, &lib_r))
        {
            map_r.1 = true;
            continue;
        }
        // TODO lib_r not found in map, so add it to the map by searching for stuff on spotify
    }

    // Remove entries from map that are not present in lib
    let mut map: Vec<MapRecord> = map
        .into_iter()
        .filter_map(|m_r| if m_r.1 { Some(m_r.0) } else { None })
        .collect();
    map.sort_by_key(|m_r| m_r.name.clone());
    map.sort_by_key(|m_r| m_r.album.clone());
    map.sort_by_key(|m_r| m_r.artist.clone());

    fs::remove_file(&map_path)?;
    fs::File::create(&map_path)?;
    let mut wtr = csv::Writer::from_path(&map_path)?;
    for map_r in map.into_iter() {
        wtr.serialize(map_r)?;
    }
    wtr.flush()?;
    Ok(())
}

async fn check(map_path: PathBuf) -> Result<(), Box<dyn Error>> {
    let map: Vec<MapRecord> = collect_csv(&map_path)?;
    todo!()
}

async fn upload(map_path: PathBuf) -> Result<(), Box<dyn Error>> {
    let map: Vec<MapRecord> = collect_csv(&map_path)?;
    todo!()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
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
    let mut cred_sp = ClientCredsClient::authenticate(client_creds_flow).await?;

    let (auth_client, url) = AuthCodeClient::new(auth_code_flow, redirect_url, auto_refresh);
    println!("Enter the following url into a browser:\n\n\t{}\n", url);
    // TODO store the auth stuff and only refresh it if its invalid
    print!("Then paste the resuting localhost url here: ");
    io::stdout().flush()?;
    let mut auth_url = String::new();
    let stdin = io::stdin();
    stdin.read_line(&mut auth_url)?;
    let split: Vec<&str> = auth_url.trim().split(['=', '&']).collect();
    let auth_code = split[1].to_owned();
    let csrf_token = split[3].to_owned();
    let mut authc_sp = auth_client.authenticate(auth_code, csrf_token).await?;

    dbg!(cred_sp
        .search("track:you are my sunshine", &[Item::Track])
        .market("GB")
        .limit(5)
        .get()
        .await?
        .tracks
        .unwrap()
        .items[0]
        .id
        .clone());

    // TODO use console, dialoguer and indicatif crates
    match cli.command {
        Commands::Map { lib_path, map_path } => map(lib_path, map_path).await,
        Commands::Check { map_path } => check(map_path).await,
        Commands::Upload { map_path } => upload(map_path).await,
    }
}
