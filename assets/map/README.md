# World map

`world.bin` is the offline world map the Coordinates window shows, a placeholder until a tile map: Natural Earth's 1:110m
land polygons (`ne_110m_land`) and land borders (`ne_110m_admin_0_boundary_lines_land`), from
<https://github.com/nvkelso/natural-earth-vector>. Natural Earth is in the public domain ("Made with Natural Earth").

`pack.py <folder of the two .geojson files> world.bin` writes it: `BCWM1`, a u32 count of lines, then each line as a u8 (1 a
closed land ring, 0 a border), a u16 count of points and the points as i16 longitude and latitude in hundredths of a degree,
little-endian. `crates/core/src/world.rs` reads it.
