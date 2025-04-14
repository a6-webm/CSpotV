use std::{
    fs::{self, File},
    io::{self, Seek, Write},
    path::PathBuf,
    time::Duration,
};

use anyhow::{anyhow, Context};
use log::{info, warn};
use spotify_rs::{
    auth::{NoVerifier, Token},
    client::Client,
    model::{search::Item, track::Track},
    ClientCredsFlow,
};
use tokio::time::sleep;

use crate::{
    ask, collect_csv,
    spotify::{get_cred_sp, print_track, search_str},
    LibRec, MapRec,
};

struct ProgMap {
    recs: Vec<MapRec>,
    writer: csv::Writer<File>,
    lib_name: String,
}

impl ProgMap {
    fn new(prog_path: &PathBuf, lib_path: &PathBuf) -> anyhow::Result<Self> {
        let prog_file = if prog_path.exists() {
            let answer = ask("In progress search detected, would you like to continue from this backup? (if not, this will overwrite the backup file)[Y/n]:", &["y", "n", ""])?;
            if answer == "n" {
                File::create(prog_path)?
            } else {
                let mut prog_file = File::options().write(true).open(prog_path)?;
                prog_file.seek(io::SeekFrom::End(0))?;
                prog_file
            }
        } else {
            File::create(prog_path)?
        };
        Ok(Self {
            recs: collect_csv(prog_path, false)?,
            writer: csv::WriterBuilder::new()
                .has_headers(false)
                .from_writer(prog_file),
            lib_name: lib_path.file_name().unwrap().to_string_lossy().into_owned(),
        })
    }

    fn index(&self) -> usize {
        self.recs.len()
    }

    // TODO make these a little nicer to read in the terminal
    fn push_rec(&mut self, case: Prog) -> anyhow::Result<()> {
        let rec = match case {
            Prog::AutomaticallyChosenSearch(map_rec) => {
                info!(
                    "line {}, \"{}\" automatically added with id: {}",
                    self.index() + 1,
                    map_rec.name,
                    map_rec.sp_id,
                );
                map_rec
            }
            Prog::ChosenSearch(map_rec) => {
                info!(
                    "line {}, \"{}\" added with id: {}",
                    self.index() + 1,
                    map_rec.name,
                    map_rec.sp_id,
                );
                map_rec
            }
            Prog::RejectedSearch(lib_rec) => {
                info!(
                    "line {}, \"{}\" added as \"Not found\"",
                    self.index() + 1,
                    lib_rec.name
                );
                lib_rec.to_map_record("Not found")
            }
            Prog::NotFoundSearch(lib_rec) => {
                warn!(
                    "line {}, \"{}\" not found by spotify search",
                    self.index() + 1,
                    lib_rec.name
                );
                lib_rec.to_map_record("Not found")
            }
            Prog::PresentInMap(lib_rec) => {
                info!(
                    "line {}, \"{}\" already present in map",
                    self.index() + 1,
                    lib_rec.name,
                );
                MapRec::default()
            }
            Prog::MissingName => {
                warn!(
                    "line {} in {} has empty Name field, skipping...",
                    self.index() + 1,
                    self.lib_name,
                );
                MapRec::default()
            }
        };
        self.writer.serialize(&rec)?;
        self.writer.flush()?;
        self.recs.push(rec);
        Ok(())
    }

    fn recs(self) -> Vec<MapRec> {
        self.recs
    }
}

#[derive(Debug)]
enum Prog {
    AutomaticallyChosenSearch(MapRec),
    ChosenSearch(MapRec),
    RejectedSearch(LibRec),
    NotFoundSearch(LibRec),
    PresentInMap(LibRec),
    MissingName,
}

// TODO if search returns no results, gradually widen the search parameters ideally until you have 5 results
async fn search_tracks(
    lib_r: &LibRec,
    cred_sp: &mut Client<Token, ClientCredsFlow, NoVerifier>,
) -> anyhow::Result<Vec<Track>> {
    let mut res = vec![];
    let mut search_lvl = 0;
    while res.len() < 5 && search_lvl <= 2 {
        let search = match search_lvl {
            0 => {
                cred_sp
                    .search(lib_r.search_str(), &[Item::Track])
                    .market("GB")
                    .limit(5)
                    .get()
                    .await
            }
            1 => {
                cred_sp
                    .search(
                        search_str(&lib_r.name, "", &lib_r.album, ""),
                        &[Item::Track],
                    )
                    .market("GB")
                    .limit(5)
                    .get()
                    .await
            }
            2 => {
                let mut s_str = String::new();
                s_str += &lib_r.name;
                s_str += " ";
                s_str += &lib_r.artist;
                cred_sp
                    .search(search_str(&s_str, "", "", ""), &[Item::Track])
                    .market("GB")
                    .limit(5)
                    .get()
                    .await
            }
            _ => unreachable!(),
        };
        if let Err(spotify_rs::Error::Spotify {
            status: 429, // rate limiting
            message: _,
        }) = search
        {
            sleep(Duration::from_secs(1)).await;
            continue;
        } else {
            let mut new_res = search?
                .tracks
                .with_context(|| "spotify search failed")?
                .items;
            res.append(&mut new_res);
        }
        search_lvl += 1;
    }
    res.truncate(5);
    Ok(res)
}

pub async fn map(lib_path: PathBuf, map_path: PathBuf) -> anyhow::Result<()> {
    let mut cred_sp = get_cred_sp().await?;

    let lib: Vec<LibRec> = collect_csv(&lib_path, true)?;
    let map: Vec<MapRec> = if map_path.exists() {
        collect_csv(&map_path, true)?
    } else {
        Vec::new()
    };

    // TODO add argument to manually specify bak file
    let prog_path = {
        let mut file_name = lib_path.file_name().unwrap().to_owned();
        file_name.push("_progress.bak");
        PathBuf::from(file_name)
    };

    let mut prog_map = ProgMap::new(&prog_path, &lib_path)?;
    while prog_map.index() < lib.len() {
        let lib_r = lib[prog_map.index()].clone();
        if lib_r.name.trim().is_empty() {
            prog_map.push_rec(Prog::MissingName)?;
            continue;
        }
        // if lib_r already present in map
        if map.iter().any(|m_r| m_r.matches(&lib_r)) {
            prog_map.push_rec(Prog::PresentInMap(lib_r))?;
            continue;
        }
        // else add lib_r to map
        // TODO let user choose market
        let search_results = search_tracks(&lib_r, &mut cred_sp).await?;
        if search_results.is_empty() {
            prog_map.push_rec(Prog::NotFoundSearch(lib_r))?;
            continue;
        }
        if let Some(track) = search_results.iter().find(|tr| lib_r.matches_track(tr)) {
            prog_map.push_rec(Prog::AutomaticallyChosenSearch(
                lib_r.to_map_record(&track.id),
            ))?;
            continue;
        }
        println!("=== Track to match ==============================");
        println!("{lib_r}\n");
        println!("=== Search results ====================");
        for (i, item) in search_results.iter().enumerate() {
            println!("= Search result {} =", i + 1);
            print_track(item);
            println!();
        }
        let tracks_len = search_results.len();
        enum Ans {
            NotFound,
            Ind(usize),
            Manual(String),
        }
        let answer = (|| -> anyhow::Result<Ans> {
            let mut answer = String::new();
            loop {
                print!("Pick a track to match (#/s/n): ");
                io::stdout().flush()?;
                io::stdin().read_line(&mut answer)?;
                answer = answer.trim().to_lowercase();
                if answer == "n" {
                    return Ok(Ans::NotFound);
                }
                if answer == "s" {
                    print!("Please manually enter the spotify id: ");
                    answer = String::new();
                    io::stdout().flush()?;
                    io::stdin().read_line(&mut answer)?;
                    return Ok(Ans::Manual(answer.trim().to_owned()));
                }
                if let Ok(i) = answer.parse::<usize>() {
                    if i > 0 && i < tracks_len + 1 {
                        return Ok(Ans::Ind(i));
                    }
                }
                answer = String::new();
            }
        })()?;
        match answer {
            Ans::NotFound => prog_map.push_rec(Prog::RejectedSearch(lib_r))?,
            Ans::Ind(index) => prog_map.push_rec(Prog::ChosenSearch(
                lib_r.to_map_record(&search_results[index - 1].id),
            ))?,
            Ans::Manual(id) => prog_map.push_rec(Prog::ChosenSearch(lib_r.to_map_record(&id)))?,
        }
    }

    // my fweaking GIWLFWIEND made me write this comment :P

    // Remove entries from map that are not present in lib
    let mut map: Vec<MapRec> = map
        .into_iter()
        .enumerate()
        .filter_map(|(map_i, map_rec)| {
            if lib.iter().any(|lib_rec| map_rec.matches(&lib_rec)) {
                Some(map_rec)
            } else {
                warn!(
                    "map line {}, \"{}\" removed as not present in lib",
                    map_i + 1,
                    map_rec.name
                );
                None
            }
        })
        .collect();
    // Add new entries
    prog_map
        .recs()
        .into_iter()
        .filter(|rec| !rec.name.is_empty())
        .for_each(|rec| map.push(rec));

    map.sort_by_key(|m_r| m_r.name.clone());
    map.sort_by_key(|m_r| m_r.album.clone());
    map.sort_by_key(|m_r| m_r.artist.clone());

    let temp_map_path = {
        let mut file_name = map_path.file_name().unwrap().to_owned();
        file_name.push(".tmp");
        PathBuf::from(file_name)
    };
    if temp_map_path.exists() {
        return Err(anyhow!(format!(
            "Path {} already exists",
            temp_map_path.to_string_lossy()
        )));
    }
    let mut wtr = csv::Writer::from_path(&temp_map_path)?;
    for map_r in map.into_iter() {
        wtr.serialize(map_r)?;
    }
    wtr.flush()?;
    fs::rename(temp_map_path, map_path)?;
    fs::remove_file(prog_path)?;
    Ok(())
}
