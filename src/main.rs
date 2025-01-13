use std::error::Error;

use serde::Deserialize;

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

fn main() -> Result<(), Box<dyn Error>> {
    let path = "bruh";
    let mut rdr = csv::Reader::from_path(path)?;
    for result in rdr.deserialize() {
        let record: LibRecord = result?;
        println!("{:?}", record);
    }
    Ok(())
}
