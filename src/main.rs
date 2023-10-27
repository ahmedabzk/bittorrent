use anyhow::{Context, Ok};
use clap::{Parser, Subcommand};
use hex;
use serde;
use serde::{Deserialize, Serialize};
use serde_bencode;
use sha1::{Digest, Sha1};
use std::net::SocketAddrV4;
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

mod decode;
mod hand;
mod hashes;
mod tracker;

use decode::decode_bencoded_value;
use hashes::Hashes;
use tracker::TrackerRequest;

use crate::hand::HandShake;
use crate::tracker::TrackerResponse;

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Torrent {
    // The URL of the tracker.
    announce: String,

    // This maps to a dictionary, with keys described below.
    info: Info,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Info {
    ///The name key maps to a UTF-8 encoded string which is the suggested name to save the file (or directory) as. It is purely advisory.
    name: String,

    ///piece length maps to the number of bytes in each piece the file is split into. For the purposes of transfer,
    ///files are split into fixed-size pieces which are all the same length except for possibly the last one which may be truncated.
    ///piece length is almost always a power of two, most commonly 2 18 = 256 K (BitTorrent prior to version 3.2 uses 2 20 = 1 M as default).
    #[serde(rename = "piece length")]
    plength: usize,

    ///pieces maps to a string whose length is a multiple of 20. It is to be subdivided into strings of length 20, each of which is
    /// the SHA1 hash of the piece at the corresponding index.
    pieces: Hashes,
    ///There is also a key length or a key files, but not both or neither. If length is present then the download represents a single file,
    ///otherwise it represents a set of files which go in a directory structure.
    ///In the single file case, length maps to the length of the file in bytes.
    #[serde(flatten)]
    key: Keys,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
enum Keys {
    SingleFile { length: usize },
    MultiFile { files: Vec<File> },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct File {
    ///The length of the file, in bytes.
    length: usize,
    ///A list of UTF-8 encoded strings corresponding to subdirectory names, the last of which is the actual file name (a zero length list is an error case).
    path: Vec<String>,
}

impl Torrent {
    pub fn info_hash(&self) -> [u8; 20] {
        let info_encode = serde_bencode::to_bytes(&self.info).expect("re-encode the info section");
        let mut hasher = Sha1::new();
        hasher.update(info_encode);
        hasher
            .finalize()
            .try_into()
            .expect("GenericArray<_, 20> == [_; 20]")
    }
}

/// Commands
#[derive(Parser, Debug)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Decode { value: String },
    Info { torrent: PathBuf },
    Peers { torrent: PathBuf },
    Handshake { torrent: PathBuf, peer: String },
}
// Usage: your_bittorrent.sh decode "<encoded_value>"
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    eprintln!("Logs from your program will appear here!");

    match args.command {
        Commands::Decode { value } => {
            let decoded_value = decode_bencoded_value(&value);
            println!("{}", decoded_value.0.to_string());
        }
        Commands::Info { torrent } => {
            let content = std::fs::read(torrent).context("read torrent file")?;

            let t: Torrent = serde_bencode::from_bytes(&content).context("parse torrent file")?;
            println!("{t:?}");
            println!("{}", t.announce);
            if let Keys::SingleFile { length } = t.info.key {
                length
            } else {
                todo!();
            };
            let info_hash = t.info_hash();
            println!("Info hash: {}", hex::encode(info_hash));
            println!("Piece length: {}", t.info.plength);
            println!("Piece hashes:");
            for hash in t.info.pieces.0 {
                println!("{}", hex::encode(hash));
            }
        }
        Commands::Peers { torrent } => {
            let content = std::fs::read(torrent).context("reading torrent file")?;
            let t: Torrent = serde_bencode::from_bytes(&content).context("decode the content")?;
            let length = if let Keys::SingleFile { length } = t.info.key {
                length
            } else {
                todo!();
            };

            let info_hash = t.info_hash();

            let request = TrackerRequest {
                peer_id: String::from("00112233445566778899"),
                port: 6881,
                uploaded: 0,
                downloaded: 0,
                left: length,
                compact: 1,
            };
            let url_params =
                serde_urlencoded::to_string(&request).context("url-encode tracker parameters")?;
            let tracker_url = format!(
                "{}?{}&info_hash={}",
                t.announce,
                url_params,
                &urlencode(&info_hash)
            );
            let response = reqwest::get(tracker_url).await.context("query tracker")?;
            let response = response.bytes().await.context("fetch tracker response")?;
            let response: TrackerResponse =
                serde_bencode::from_bytes(&response).context("parse tracker response")?;
            for peer in &response.peers.0 {
                println!("{}:{}", peer.ip(), peer.port());
            }
        }
        Commands::Handshake { torrent, peer } => {
            let content = std::fs::read(torrent).context("reading torrent file")?;
            let t: Torrent = serde_bencode::from_bytes(&content).context("decode the content")?;

            let info_hash = t.info_hash();

            let peer = peer.parse::<SocketAddrV4>().context("parse peer address")?;
            let mut peer = tokio::net::TcpStream::connect(peer)
                .await
                .context("connect to peer")?;

            let mut handshake = HandShake::new(info_hash, *b"00112233445566778899");

            {
                let handshake_bytes =
                    &mut handshake as *mut HandShake as *mut [u8; std::mem::size_of::<HandShake>()];
                // Safety: Handshake is a POD with repr(c)
                let handshake_bytes: &mut [u8; std::mem::size_of::<HandShake>()] =
                    unsafe { &mut *handshake_bytes };
                peer.write_all(handshake_bytes)
                    .await
                    .context("write handshake")?;
                peer.read_exact(handshake_bytes)
                    .await
                    .context("read handshake")?;
            }

            assert_eq!(handshake.length, 19);
            assert_eq!(&handshake.bittorrent, b"BitTorrent protocol");
            println!("Peer ID: {}", hex::encode(&handshake.peer_id));
        }
    }
    Ok(())
}

fn urlencode(t: &[u8; 20]) -> String {
    let mut encoded = String::with_capacity(3 * t.len());
    for &byte in t {
        encoded.push('%');
        encoded.push_str(&hex::encode(&[byte]));
    }
    encoded
}
