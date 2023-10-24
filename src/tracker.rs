use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TrackerRequest {
    // the info hash of the torrent
    // 20 bytes long, will need to be URL encoded
    pub info_hash: [u8; 20],

    // a unique identifier for your client
    // A string of length 20 that you get to pick. You can use something like 00112233445566778899.
    pub peer_id: String,

    // the port your client is listening on
    pub port: u16,

    // the total amount uploaded so far
    pub uploaded: usize,

    // the total amount downloaded so far
    pub downloaded: usize,

    // the number of bytes left to download
    pub left: usize,

    // the purposes of this challenge, set this to 1.
    pub compact: u8,
}


#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TrackerResponse{
    // An integer, indicating how often your client should make a request to the tracker.
    interval: usize,

    // A string, which contains list of peers that your client can connect to.
    // Each peer is represented using 6 bytes. The first 4 bytes are the peer's IP address and the last 2 bytes are the peer's port number.
    peers: String
}
