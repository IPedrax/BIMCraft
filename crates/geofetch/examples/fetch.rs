//! A developer's tool to try the real service, which no test does: Overpass for a box's buildings, written
//! to a file. It is not part of BIMCraft and is not shipped.
//!
//! ```text
//! cargo run -p bimcraft-geofetch --example fetch -- osm SOUTH WEST NORTH EAST out.json
//! ```
//!
//! Replies and tiles are kept in a folder of the temp folder, so a second run is the cache's.

use std::sync::Arc;

use bimcraft_core::geo::{Bbox, GeoSource, Layers, overpass_query};
use bimcraft_geofetch::Geofetch;
use bimcraft_geofetch::cache::Cache;
use bimcraft_geofetch::net::{Allow, OVERPASS, Ureq};

fn number(args: &[String], i: usize, what: &str) -> Result<f64, String> {
    args.get(i).and_then(|a| a.parse().ok()).ok_or_else(|| format!("{what}: a number is wanted"))
}

fn run(args: &[String]) -> Result<(), String> {
    let kind = args.first().map(String::as_str).unwrap_or_default();
    let bbox = Bbox::new(number(args, 1, "south")?, number(args, 2, "west")?, number(args, 3, "north")?, number(args, 4, "east")?)?;
    bbox.check_area()?;
    let cache = Arc::new(Cache::new(Some(std::env::temp_dir().join("bimcraft-geofetch-cache"))));
    let fetcher = Geofetch::new(Arc::new(Ureq::new(Allow::standard())), cache, OVERPASS);
    match kind {
        "osm" => {
            let out = args.get(5).ok_or("the file to write is wanted")?;
            let reply = fetcher.overpass(&bbox, &overpass_query(&bbox, Layers::default()))?;
            std::fs::write(out, &reply.text).map_err(|e| format!("{out}: {e}"))?;
            println!(
                "{} bytes{} to {out}\n{}",
                reply.text.len(),
                if reply.cached { " (from the cache)" } else { "" },
                fetcher.attribution("osm")
            );
        }
        _ => return Err("the first argument is osm".into()),
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(why) = run(&args) {
        eprintln!("{why}");
        std::process::exit(1);
    }
}
