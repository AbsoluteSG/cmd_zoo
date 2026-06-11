//! Smoke test for the SpacetimeDB online adapter — connects to the deployed
//! module, joins the hub, moves the avatar, and prints the subscribed state.
//!
//! Run against Maincloud (the default `critter-cove` deployment):
//!
//! ```sh
//! cargo run --example stdb_smoke
//! cargo run --example stdb_smoke -- critter-cove MyName
//! ```
//!
//! Or against a local server (`spacetime start`):
//!
//! ```sh
//! cargo run --example stdb_smoke -- critter-cove Local http://127.0.0.1:3000
//! ```
//!
//! Expected: a "connected as …" line, then a zoo row on slot 0 with 100 coins at
//! the hub centre (250000, 250000), and an avatar pose at (123, 456).

use std::time::Duration;

use cmd_zoo::stdb::client::{MAINCLOUD_URI, OnlineClient};

fn main() -> anyhow::Result<()> {
    let module = std::env::args().nth(1).unwrap_or_else(|| "critter-cove".to_string());
    let name = std::env::args().nth(2).unwrap_or_else(|| "SmokeTest".to_string());
    let uri = std::env::args().nth(3).unwrap_or_else(|| MAINCLOUD_URI.to_string());
    // A stable test key so re-running reuses the same account instead of piling
    // up duplicates (pass a name to vary it).
    let player_key = format!("local:smoke-{name}");

    println!("connecting to {module} at {uri} as {player_key} …");
    let client = OnlineClient::connect(&uri, &module, &player_key, &name)?;

    // Let the async connect + subscription settle.
    std::thread::sleep(Duration::from_secs(3));
    println!("identity: {:?}", client.identity());

    println!("calling join_hub() as {player_key} …");
    client.join_hub()?;
    client.move_avatar(123.0, 456.0)?;

    // Let the reducers round-trip and the cache update.
    std::thread::sleep(Duration::from_secs(3));

    println!("--- zoos ---");
    for z in client.zoos() {
        println!("  slot {} · {} coins · plot {:?}", z.slot, z.coins, z.plot);
    }
    println!("--- avatar poses ---");
    for (id, x, y) in client.avatar_poses() {
        println!("  {id} @ ({x}, {y})");
    }

    client.disconnect();
    Ok(())
}
