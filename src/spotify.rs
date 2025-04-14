use std::{
    fs,
    io::{self, Write},
};

use spotify_rs::{
    auth::{NoVerifier, Token},
    client::Client,
    model::{track::Track, PlayableItem},
    AuthCodeClient, AuthCodeFlow, ClientCredsClient, ClientCredsFlow, RedirectUrl,
};

const CLIENT_ID: &str = "fed3e6de8e3e4fe481b4020cdb72342e";
const CLIENT_SECRET_PATH: &str = "client_secret.txt";

pub struct Tr {
    pub name: String,
    pub id: String,
    pub pos: u32,
}

pub fn search_str(q: &str, track: &str, album: &str, artist: &str) -> String {
    let mut out = String::new();
    if !q.trim().is_empty() {
        out += &q.replace(" ", "+");
        out += " ";
    }
    if !track.trim().is_empty() {
        out += "track:";
        out += &track.replace(" ", "+");
        out += " ";
    }
    if !album.trim().is_empty() {
        out += "album:";
        out += &album.replace(" ", "+");
        out += " ";
    }
    if !artist.trim().is_empty() {
        out += "artist:";
        out += &artist.replace(" ", "+");
        out += " ";
    }
    if !out.is_empty() {
        out.remove(out.len() - 1);
    }
    out
}

pub fn print_track(track: &Track) {
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

pub async fn get_all_playlist_tracks(
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

pub async fn get_cred_sp() -> anyhow::Result<Client<Token, ClientCredsFlow, NoVerifier>> {
    let client_creds_flow =
        ClientCredsFlow::new(CLIENT_ID, fs::read_to_string(CLIENT_SECRET_PATH)?.trim());
    Ok(ClientCredsClient::authenticate(client_creds_flow).await?)
}

pub async fn get_authc_sp() -> anyhow::Result<Client<Token, AuthCodeFlow, NoVerifier>> {
    let redirect_url = RedirectUrl::new("http://127.0.0.1".to_owned())?;
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
