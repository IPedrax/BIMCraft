# World map

`world.bin` is the offline world map the Coordinates window shows, a placeholder until a tile map: Natural Earth's 1:110m
land polygons (`ne_110m_land`) and land borders (`ne_110m_admin_0_boundary_lines_land`), from
<https://github.com/nvkelso/natural-earth-vector>. Natural Earth is in the public domain ("Made with Natural Earth").

`pack.py <folder of the two .geojson files> world.bin` writes it: `BCWM1`, a u32 count of lines, then each line as a u8 (1 a
closed land ring, 0 a border), a u16 count of points and the points as i16 longitude and latitude in hundredths of a degree,
little-endian. `crates/core/src/world.rs` reads it.

`places.tsv` is the places the Coordinates window finds a map system by: Natural Earth's countries (`ne_50m_admin_0_countries`)
and populated places (`ne_10m_populated_places_simple`), public domain too. `places.py <folder of the two .geojson files>
places.tsv` writes it: a comment line, then a line a country, `C`, its ISO 3166-1 code, the longitude and latitude of its
label and its names joined by `|` (English first), and a line a city, biggest first, `P`, the country's code, longitude,
latitude, its name, its ASCII spelling if that differs, and its state or province, tab-separated. `crates/core/src/places.rs`
reads it.
