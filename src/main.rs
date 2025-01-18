use std::{
    error::Error,
    fs,
    io::{self, Write},
    path::PathBuf,
};

use clap::{command, Parser, Subcommand};
use serde::{de::DeserializeOwned, Deserialize};
use spotify_rs::{AuthCodeClient, AuthCodeFlow, RedirectUrl};

// TODO does the following matter
//     By default, struct field names are deserialized based on the position of
//     a corresponding field in the CSV data's header record.

#[derive(Debug, Deserialize)]
struct LibRecord {
    name: String,
    album: String,
    artist: String,
}

#[derive(Debug, Deserialize)]
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

fn collect_csv<T: DeserializeOwned>(path: PathBuf) -> Result<Vec<T>, csv::Error> {
    let rdr = csv::Reader::from_path(path)?;
    rdr.into_deserialize()
        .collect::<Result<Vec<T>, csv::Error>>()
}

async fn map(lib_path: PathBuf, map_path: PathBuf) -> Result<(), Box<dyn Error>> {
    let lib: Vec<LibRecord> = collect_csv(lib_path)?;
    let map: Vec<MapRecord> = collect_csv(map_path)?;
    todo!();
}

async fn check(map_path: PathBuf) -> Result<(), Box<dyn Error>> {
    let map: Vec<MapRecord> = collect_csv(map_path)?;
    todo!();
}

async fn upload(map_path: PathBuf) -> Result<(), Box<dyn Error>> {
    let map: Vec<MapRecord> = collect_csv(map_path)?;
    todo!();
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();

    let redirect_url = RedirectUrl::new("http://localhost/".to_owned())?;
    let auto_refresh = true;
    let scopes = vec!["playlist-read-private", "playlist-modify-private"];
    let auth_code_flow = AuthCodeFlow::new(
        "fed3e6de8e3e4fe481b4020cdb72342e",
        fs::read_to_string("client_secret.txt")?,
        scopes,
    );

    let (auth_client, url) = AuthCodeClient::new(auth_code_flow, redirect_url, auto_refresh);
    println!("Enter the following url into a browser:\n\n\t{}\n", url);

    // TODO add option to store the auth stuff and only refresh it if its invalid
    print!("Then paste the resuting localhost url here: ");
    io::stdout().flush()?;
    let mut auth_url = String::new();
    let stdin = io::stdin();
    stdin.read_line(&mut auth_url)?;
    let split: Vec<&str> = auth_url.trim().split(['=', '&']).collect();
    let auth_code = split[1].to_owned();
    let csrf_token = split[3].to_owned();

    let mut auth_sp = auth_client.authenticate(auth_code, csrf_token).await?;

    match cli.command {
        Commands::Map { lib_path, map_path } => map(lib_path, map_path).await,
        Commands::Check { map_path } => check(map_path).await,
        Commands::Upload { map_path } => upload(map_path).await,
    }
}
