use std::{error::Error, path::PathBuf};

use clap::{command, Parser, Subcommand};
use serde::{de::DeserializeOwned, Deserialize};

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

fn map(lib_path: PathBuf, map_path: PathBuf) -> Result<(), Box<dyn Error>> {
    let lib: Vec<LibRecord> = collect_csv(lib_path)?;
    let map: Vec<MapRecord> = collect_csv(map_path)?;
    todo!();
}

fn check(map_path: PathBuf) -> Result<(), Box<dyn Error>> {
    let map: Vec<MapRecord> = collect_csv(map_path)?;
    todo!();
}

fn upload(map_path: PathBuf) -> Result<(), Box<dyn Error>> {
    let map: Vec<MapRecord> = collect_csv(map_path)?;
    todo!();
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Map { lib_path, map_path } => map(lib_path, map_path),
        Commands::Check { map_path } => check(map_path),
        Commands::Upload { map_path } => upload(map_path),
    }
}
