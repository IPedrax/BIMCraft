# Point clouds

On the roadmap under 0.9 Interoperability. A project holds scans and places them, the 3D view, plans and sections draw them,
plans snap to them, and the desktop brings them in (Insert > Point Cloud). IFC leaves them out, as Revit's export does: IFC4 has
no point cloud, and `ifc_leaves_scans_out_as_revit_does` keeps the file what it was without them.

Point clouds (`cloud.rs`) come from public specifications: PTS and XYZ text, PLY (text or either byte order), ASPRS LAS 1.0
to 1.4, LAZ and COPC, and ASTM E57 (`Format::from_extension` says why ReCap's RCP/RCS don't). LAZ (`cloud/laz.rs`) is LAS whose
points LASzip compressed, and COPC (`.copc.laz`) is LAZ 1.4 with its chunks indexed, so both are `Format::Laz`: the header is
LAS's (`LasHead`, shared with `las`), LASzip's record of its own ("laszip encoded", 22204) is found among the file's records,
and the laz crate (Apache-2.0, without its `parallel` feature, whose decompressor trusts the table) gives back the records LAS
would hold. The crate is Apache-2.0 only, so it sits behind `bimcraft-core`'s `laz` feature, on by default (as `crates/rvt`
keeps its licence apart): a build without it refuses LAZ and says so, and a binary built with it is Apache-2.0. A file whose point format has LASzip's bit is read as LAZ whatever its name. A chunked file's table of chunks is read
and checked before anything is held (`chunks`): no more chunks than the points and the file could fill, their points adding up
to the header's, their bytes fitting between the points and the table. Its chunks are then decompressed side by side
(`side_by_side`): twice as many at a time as rayon has threads, no more than `PAR_BATCH` of points, each by a decompressor of
its own over its bytes (`decompressor`, the items' version choosing point by point or layered), and handed on in the file's
order. A file with no table, or a chunk that decompresses past `MAX_CHUNK`, is decompressed as one stream, a megabyte at a
time. LASzip can't step within a chunk, so a LAZ file is read once, not twice (`once`): its header says how many points it
holds, so every `stride`th is held as it was in the file (`Held`, its shades in `f32`, which holds sixteen bits exactly) while
the bounds and shades are measured, and kept (`Keep`) once they are known, which is what two passes keep.

A file is never held whole, so its size takes no memory and isn't what limits it (`MAX_BYTES` is a terabyte, a sanity cap).
`cloud::read_from` reads a `Source` (anything that reads and seeks: the file itself in `point_cloud`, unbuffered, or bytes in
memory through `cloud::read`) a piece at a time: text a line at a time (none longer than `MAX_LINE`), PLY's header from its
first megabyte and its body after it, LAS's records a megabyte at a time, or, thinned hard, one by one by seeking to every
`stride`th (`records`), so the second pass reads only what it keeps. Each reader hands a point with its place among the file's
points, which is how a reader that steps over records and one that doesn't thin the same. What a read takes is the points it
keeps: `point_cloud` asks `memory::Budget` for `KEPT_BYTES` a point of what the project leaves before reading, and the read
looks at the budget every `LOOK` points, so on a machine without the memory it stops with the budget's message instead of
running out. The budget carries the command's progress: "Measuring the scan" (uncounted), then "Reading the points" (counted;
a LAZ file, read once, has this stage alone), and Cancel stops it there. The reference survey (1.01 billion points, 5 million
kept) reads from a 17 GB E57 in four and a half minutes in under 140 MB, and from an 8.8 GB LAZ in 72 seconds in about 400 MB
(nine and a half minutes when it was decompressed twice, one chunk after another; 32 seconds natively on Windows).

`cloud::read_from` parses a file twice, once to measure it (`Stats`: the count, the bounds, the ranges of intensity and colour) and once to keep it: every `stride`th
point so `Options::cap` is never passed, then one in each cube of `spacing`, the same every time. A `Scan` keeps its points as
`f32` about `origin`, the middle of its bounds, so a survey's coordinates far off keep their millimetres within `MAX_SPAN`;
colour comes in eight bits whether the file has eight, sixteen or nought to one, and intensity alone is grey.

E57 (ASTM E2807, `cloud/e57.rs`) is read from the standard with no library: the 48-byte header, pages of 1024 bytes whose last
four are a CRC-32C (`Paged` reads the 1020 logical bytes of each; the checksums aren't checked, so a page that lies reads as
points that lie, never a crash), and the XML section, read by a small parser of its own (elements, attributes, text, CDATA;
capped in depth and size, and up to 256 MiB of it held). The file is never held: `Paged` reads it through `Seek`, a window of
1 MiB at a time, and serves the pages from that window until a read falls outside it; reading on keeps what's left of the
window and never seeks, so a pass reads each byte about once, and a file of hundreds of gigabytes takes the memory of a small one. Every `data3D` scan comes in, placed by its `pose` (a quaternion and a translation), as one `Scan`
whose `read` is the sum of their `recordCount`s. Each scan's points are a CompressedVector: a section header, then data packets
whose bytestreams, one for each field of the `prototype`, are bit-packed and run on from packet to packet; `Pack` decodes
Float (single or double), Integer and ScaledInteger, and only bit-packing, the one codec E57 writes, is read. Cartesian or
spherical coordinates both come in, a point marked invalid doesn't, and colour is scaled from `colorLimits` (or its field's
range) to eight bits. `images2D` is passed over. A file is held to `Caps` before anything is unpacked: up to 4096 scans, 100 billion
points in all of them (a sanity cap that keeps a lying count from hours of unpacking; what's kept is the cloud's to thin), 32 fields to a point, no two scans on one section, a section with room for the bits its count asks for (a
field of no bits is legal, so eight points can share a byte), and no stream more than 4 MiB ahead of the others. The tests write E57 files of their own (`e57` in the module's tests), with
values that cross packets and pages; the reference survey's 177 MB file reads its 6.6 million points in under two seconds.

A project holds each scan once, by name, as a `Cloud` in `Model::clouds` (a library entry of kind `Cloud`, so it has an id and a
GUID): its points and colours shared behind an `Arc`, the file it came from, and where its middle was there. A project file holds
it as `CloudFile`, the points as little-endian `f32` and the colours as bytes, each deflated and in base64, packed once and kept
(`Cloud::file`); coming back, every check `Cloud::new` makes is made again, and what unpacks past `MAX_CLOUD_POINTS` stops there.
A project holds up to `MAX_CLOUDS` scans and `MAX_CLOUD_POINTS` points among them (`Model::check`), so the `point_cloud` command
thins a new one to what the others leave. A project file is held to the same while it is read, not after: `Model::clouds` reads
through `cloud::deserialize_scans`, which refuses the entry past the count, and the scan that takes the points past the budget,
before unpacking the next (unpacking is where the memory goes, and the file is read twice, by the lenient walk and the strict read).

A survey's stations come in as one scan, as ReCap gathers a project's scans and Revit links the project: `path` may be a list of
up to `MAX_STATIONS` files. Each is read with an even share of what the project leaves, and `cloud::merge` puts them about the
middle of them all, keeps one point in each cube of `spacing` across them (so where stations overlap they aren't kept twice), and
greys a file with no colours among ones with. The `Cloud` keeps a `Station` for each file, its name and its share of the points
in order, so a view may one day show or hide a station; it is known by the first file's name, and the element's JSON lists the
stations. A list of one is that file alone.

A `PointCloud` element places a scan, as a component places a family: its `cloud` (a library name, a GUID in a file), its `level`,
its middle at `at` and `offset` over the level, turned `angle` degrees about it; `Model::cloud_place` is that transform and
`Model::cloud_box` the box round what it places. It holds no `params`, so phases, design options and paint don't reach it. It moves,
turns and copies as any element; it doesn't mirror, since a scan is what was measured. The `point_cloud` command brings a file in
(`path`) or places a held scan again (`cloud`); by default its middle goes on `at` and its foot on the level (Revit's Auto - Center
to Center), and with `origin: file` where the file's coordinates put it (Origin to Origin) when that is within `MAX_COORD`. A scan
that nothing places any more goes after the command that left it so (`Model::unplaced_clouds`, in `Session::execute`), and undo
brings it back with its element. Insert from File brings the scan with the element. IFC export leaves scans out.

The 3D view (`view3d.rs`) keeps each placed scan as a `Scan`: its points and colours shared with the model's, and where they
go. After the opaque surfaces are rasterised, `Scene::dots` draws each point as a square [`DOT`] pixels wide into the same
depth buffer, so what is nearer hides a scan and a scan hides what is behind it, and what the view sees through is drawn over
it. A point's pixel holds its scan's slot after the triangles', so it picks, box-selects and tints as its element; it has no
face to paint and draws no edge. Points are not lit: they keep the scan's colours, grey (`cloud::GREY`) for a scan with none,
or the colour the view's projection fill gives. A picture draws at most `DRAWN` points, an even share of each scan's past
that, as Revit's density does. Automatic 3D framing (parallel and perspective) fits the visible built geometry and its levels;
scans contribute only when no built geometry is shown. A level's default plane reaches past the model's solids, grids and CAD
drawings (`Model::model_extent`), falling back to scans when those are absent. Uncropped sections span these level extents,
so a distant site survey does not shrink the building's default views. Explicit cameras, level extents and crops keep their
own framing; section boxes still enclose scans when fitted with `section_box`.
A 3D view's section box keeps a scan's points inside it and no others, so its floors and ceilings can be cut away to show the walls to
model against ([views and graphics](views-and-graphics.md)); in a plan the view range does it, or the box itself when the plan follows it. Views hide and override point clouds by element or by the category `pointcloud` (`graphics::SCANS`, apart from `MODEL`:
Revit has a tab of its own for them in Visibility/Graphics, and Object Styles no row). The graphics card draws them as the
processor does (`Mesh::dots`, the same every so many points, [the 3D plan](../3d-acceleration-plan.md)). The wireframe style leaves them out.

A section's or an elevation's picture is the same rasteriser with a `Clip`, and `Scene::dots` keeps a point as `raster` keeps a
pixel: between the line and the far clip, inside the crop. A plan draws what its view range sees, from the cut plane down to
the view depth (`render::scans`, under everything else), from above: each point a dot `DOT` pixels wide in its colour or the
view's, the highest at each pixel on top, `DRAWN` among the scans as in 3D. Each scan is one `Prim::Image` owned by its
element, no bigger than its dots and clear between them; a picture picks where it isn't clear (`drawn_near` in `Drawing::pick_all`),
so a click between a scan's points finds what is there, and a sheet's opaque pictures pick as they did. A PDF masks a picture
that isn't all there with its alpha (`/SMask`), and an SVG holds it as a PNG. A reflected ceiling plan leaves scans out. The
desktop's plan canvas loads a drawing's pictures as textures once for as long as it is shown (`PlanCache::pictures`) and, on a
dark canvas, dims them so the lightened ink over them still reads. A scan the view fills with one colour (its projection's fill,
`Look::filled`) is ink itself: its picture is marked `ink` and the dark canvas leaves it undimmed.

A scan's own colours are mid-tones that read poorly over paper or the dark canvas, so the view control bar (`footer.rs`) has
High Contrast Point Clouds wherever the project or a model it links holds a scan (the bar asks the model the views draw whether it holds
one, which is cheap because a command drops a scan nothing places): it fills the view's `pointcloud` category with `HIGH_CONTRAST`, a
crimson that reads on both and apart from the model's black lines (or the colour the user picks in Settings > Graphics, kept
as `high_contrast` in `settings.json` and read back only when it is `[r, g, b]`), through the `graphics` command (merged into the category's
override, so its halftone or lines stay), and again takes the fill away. It is lit whenever the category has a fill, whoever set
it, and greyed out when a view template controls the categories. It sets the category's override and no other: `Look::of` applies the
category's, then each view filter's, then the element's own, the last prevailing, so a scan given a colour of its own in
Visibility/Graphics (or picked out by a filter that fills) keeps it whatever the button does. The button is lit for the category
and can't tell, since what the scans come to is the whole `Look` for every frame of the bar; its tooltip says so instead.

A view colours a scan's points by what its override's `scan` says (`graphics::ScanColour`, Revit's Color Mode): `normal`, their
own colours; `elevation`, their height in the model through `graphics::RAMP` (blue, cyan, green, yellow, red) from `low` to `high`
(mm; the scan's own foot and head where not given, so each scan runs the whole ramp); or `intensity`, through the same ramp from the
weakest return (blue) to the strongest (red), as a surveyor's viewer shows it, each by its rank among the scan's intensities
(`Cloud::intensity_levels`, histogram equalization: however the returns crowd, the colours spread over the whole ramp;
`graphics::intensity_colour`). A scan keeps each point's intensity for that (`Cloud::intensity`, a byte over the file's range, in the project
file as `intensity` beside the colours; an older file has none, and a scan whose file had none, or all alike, keeps its own
colours). It resolves as any override does, the category's, then each view filter's, then the scan's own, and a projection `fill`
(High Contrast's) still colours them one colour over it. The 3D view, the graphics card and sections colour through
`view3d::Paint`, worked out once a scan for each picture; a plan through `render::scans`. The view control bar has Point Cloud
Colours beside High Contrast (Normal takes the category's mode away); Visibility/Graphics has a Point Cloud Colours group on the
Model Categories tab (the mode, and an elevation's Low and High, typed in the project's units) and the same for a scan picked on
its Elements tab (`clouds::scan_colour_rows`).

A plan snaps to a scan where its cut plane cuts it, to trace what was measured: of the points within `snap::CUT_BAND` of the cut,
the nearest to the cursor seen from above (`Kind::Point`, "Point Cloud", switched by `Snaps::cloud` in Manage > Snaps). It comes
after what the model has to snap to (ends, middles, centres, crossings), so a wall's end beats a point nearer the cursor, and
before the perpendicular, the angle guides and a wall's own line. `snap::cloud_point` turns the cursor into each shown scan's
own terms and asks `Cloud::nearest`, which looks only in the squares of the scan's `Grid` (its points by `CELL` mm squares from
above, made the first time something snaps to it and shared by its clones) that the reach touches, so a snap on millions of points
costs microseconds.

In the desktop (`clouds.rs`), Insert > Point Cloud, in the Insert tab's Link panel beside Link Model as in Revit, asks for one
file or several (`BimApp::ask_files`: a dialog's answer is a list of paths now) and opens `CloudBox`: its name (the first file's),
the units of its coordinates, a spacing, and Revit's positioning, Auto - Center to Center or Auto - Origin to Origin. OK runs
`point_cloud` on the level of the plan on screen and selects what it placed; a refusal stays in the box with its reason
(`point_cloud_paths` opens the box, what a test hands the files to). Files of `IN_BACKGROUND` (16 MB) or more together are read
first on a thread of their own: `Session::read_scan_ahead` runs `point_cloud` on a session of its own sharing the project, which
keeps the scan it read (`ScanRead`, for the files, their unit, the spacing and the room the project left), while the window draws
on and a box shows the read's stage, a bar and Cancel (`cloud_reading_box`). When it's read, `Session::give_scan` hands it to the
window's session, and `point_cloud` there takes it instead of reading the files again: what it places is one undo step, as a
small file's is. A read for other files or options, or before the project's other scans took some of its room, isn't taken, and
it's taken once. A refusal, or Cancel, comes back to the box with its reason. The Properties palette shows a point cloud's level and offset
(edited), its scan, file and points, its origin and angle; Visibility/Graphics lists `pointcloud` after the model's categories.

## Every point, read as a view zooms in: a scan's index

A project holds no more than `MAX_CLOUD_POINTS` (five million) points of its scans, so a scan of a billion is a thin one in it. Its
index keeps every point instead, in a file beside the project, and a view reads from it what it shows at the detail it shows it,
as Revit reads a ReCap project. `point_cloud`'s `index` (true, false, or `auto`, the default: files of more than 64 MB) builds
it at `<project>_clouds/<scan>.bimpc` beside the project's file, or at `<scan>.bimpc` beside the scan's first file while the
project has none (the project holds its full path then); the project holds the top of it
as the scan, and `IndexRef` (`cloud/lod.rs`) in the scan's entry of the project file: its path from the project's folder, the path
it was built at in full (where it's looked for after the project is saved elsewhere), its id, its count and the levels the scan
holds, and the files it was built from with their unit (what Rebuild Index reads again). An older BIMCraft leaves `index` out and
shows the top. `spacing` isn't used for an indexed scan. An index is a cache of the scan's files, always built again from them, not a
format to exchange. A user's folder of indexes (`point_cloud`'s `index_dir`, `Session::index_dir`, the desktop's Point cloud
index folder) takes a new one as `<project>_<scan>.bimpc`, and is where `open` looks for one the project's paths don't find, so
a project copied without its indexes finds them there (`lod::places`: the project's path, the full one, the folder).
`point_cloud_index {action}` says where an index is and what it takes (`info`), builds it again from its files (`rebuild`),
deletes its file and lets it go, the scan keeping its top (`delete`), and deletes the indexes in the project's folders no scan
names, as deleted or undone scans leave them (`purge`). A session that may only read builds and deletes none. A Co-Work guest's
copy holds the index's reference but opens nothing of the host's disk, so it draws the top.

**The file** (`cloud/index.rs`, BIMCraft's own, little-endian). A header of 128 bytes: `BIMPC\0\r\n`, the version (1), flags
(colours, intensities), a random id (`u128`), the count, the scan's middle in its files' coordinates (as a `Cloud`'s origin), the
root cube's lowest corner from it and its width, mm, and where the table of nodes starts and how many it has. Then each node's
points, deflated; then the table, 32 bytes a node: its level, its key (x, y, z at that level), where its points are, their bytes and
how many. A point's place is a `u32` along each axis of the root cube, so the node at level `l` holding it is its top `l` bits; a
node stores the bits below them, the top 16 where that's within half a millimetre (`FINEST`) and all 32 where not, axis by axis,
then each point's colour (3 bytes) and intensity (1), as the scan's are made (`Shade`, the file's range). Each point is in one node.
A node keeps one point in each cell of a 128³ grid of its cube and hands the rest to its children, down to a leaf of no more than
32,768 (Potree's method): so the points of a level are `edge / 2^(l + 7)` apart at least, and the levels from the root are a
sample that gets finer as it goes down. An index is hostile when read: its header, the table (the nodes' levels and keys in the
cube, in the order of their places with no two in one place, their bytes in the file, their points the header's count; no more
than `MAX_NODES`, four million) and each node's length after inflating are checked, and a node that can't be read gives no points.
The table is searched, not hashed, and a node is read at its place in the file (`read_exact_at`, `seek_read`), with no lock, so
threads read side by side. Nodes are deflated (zlib, `zlib-rs`): measured on the reference survey's nodes, zstd (`ruzstd`)
decoded 1.4 times as fast (850 against 595 MB/s) but encoded at half the speed into files a tenth bigger, so zlib stays.

**Building one** (`index::build`) reads the files twice: once to measure them (`Stats`: bounds and shades), once to sort each
point into a bucket file of the node of its cube at the level where a bucket holds about two million (up to 512 of them, in a
`.bimpc.tmp` folder beside the index). Buckets are built side by side, as many as the memory the machine has free leaves room for
(`Budget::left`); one with more is split into its eight. A bucket's subtree is built in memory, top down, and written as it goes,
but for its top, which the levels above take their sample from, bottom up (`climb`). The table and the header are written last,
the file renamed from `.bimpc.part`, the buckets removed, whatever ends it. Its stages report to the progress ("Measuring the
scan", "Sorting the points", "Building the index") and stop at Cancel. The scan the project holds is the levels from the root that
fit in what the project has room for, whole (`Built::levels`; a thinned root when even it doesn't), so a view draws below them.

**Reading it** (`cloud/lod.rs`). Views ask for nodes through `cloud::source::NodeSource`, never of the `.bimpc` format, so another
octree (COPC) is a second source. `open` opens every scan's index (`Cloud::attach_in`, shared by every copy of the scan) and lists
those it can't as `clouds_not_indexed`; their views draw the top. `point_cloud_index {cloud, path?}` finds one again. A view asks
`lod::select` for the nodes below each scan's levels that it shows, of all the scans at once, until they hold so many points:
one queue ranked by how far apart each node's own points are on the screen, roots too, so the scan the view is near gets the
budget, not an even share of it. A node is read below one whose points are more than `SPREAD` (1.5) px apart on the screen
there, and only where the view shows it: the picture, the section box, a plan's slab from its cut to its view depth. Nodes read
are kept in one cache for every index (`lod::Cache`), the node asked for longest ago going first, whichever scan it's of, once
they together take more than `lod::memory()`: what the app set, or a quarter of the memory the machine has free, between 256 MB
and 8 GB, halved while it runs short. A node two threads ask for is read once, the second waiting for it. The 3D view (`view3d::scan_detail`, drawn by
`Scene::render_detailed` or as the mesh's dots by `Detail::dots`) and plans (`render::scans`) draw a node's points as the scan's.
A plan and an export read the nodes they show; the window draws a plan from what's read (`render::from_cache`, which says how
many it didn't have) and has a thread read the rest. A snap looks among every point near the cursor (`Cloud::nearest_of_all`,
a few nodes read).

A billion points take about a billion times ten bytes before deflating, and the build needs about sixteen bytes a point free on
the disk while it sorts them.

In the desktop, the Point Cloud box has Keep full detail (index beside the project), on for files of more than 64 MB, and says
where the index goes when the project has no file yet (beside the scan's); an indexed scan is always built on a thread of its own, behind the progress box. The 3D view
streams the indexes' nodes (`stream.rs`, `Orbit::stream_nodes`), as Potree does; it's described below. A plan is drawn from what's read and,
where that left nodes out, drawn again by the plan worker, which reads them (`PlanCache::complete`). Settings > Graphics has Point
cloud memory (`lod::set_memory`), Points drawn in 3D (Auto: ten million on the card, three on the processor) and Point cloud
index folder. The Project Browser lists the point clouds with what an open index takes; a scan whose index isn't open says
"(index missing)" ("(index on host)" for a Co-Work guest), and its menu has Find Index…, Reload Index, Rebuild Index and Delete
Index…, which asks first (`point_cloud_index`).

The 3D view streams the nodes rather than waiting for the camera to stand still. Each frame the camera, the model or the look
changed, `view3d::scan_wanted` ranks the nodes the picture wants across every scan (no more than Points drawn in 3D), reading
nothing; three loader threads (`stream::Pool`) take the top of that queue, and each new ranking replaces the queue, so what the
camera moved away from before a loader took it is never read (a node being read is read once, and kept if it's still wanted). On
the processor a loader reads the node into the indexes' cache, and the picture draws what that holds (`scan_detail` without
reading, then `Scene::render_detailed`), drawn again as nodes come in, at most every 250 ms while they're still coming. On the card a
loader also packs the node for the mesh and the look (`view3d::node_dots`: 12 bytes a point, its place as 16-bit numbers in the
node's box, and its colour as the view gives it), and the window sends up to 16 MB of them a frame (`Gpu::upload_nodes`), each a
buffer of its own kept in a cache by node: one budget for all of them (1 GiB, less where the card's buffers are smaller or an
allocation fails), the nodes used longest ago and not wanted let go first. The view draws those the card holds of what it wants
(`Gpu::draw_nodes`), each by the `bimcraft-3d-nodes` pipeline with its box, step and owner bound at its slot of a uniform buffer and
unpacked in `vs_node`, the same points as the mesh's own (`fs_dot`, cut by the section box whole or not at all). Nodes that come
while the camera stays are drawn over the picture's targets rather than the picture again, unless the view sees through something.
Another mesh or look packs them again; the card's held ones are let go. Nothing of this runs where no scan's index is open.
Eye-dome lighting (Graphic Display Options) darkens each point by how far it lies behind its neighbours two pixels off, as
`view3d::eye_dome` does on the processor: on the card in the edges' pass (`dome` in `gpu.wgsl`), off the opaque depth, the points'
pixels known by their plane; in a parallel view `cs_near` first finds the nearest depth in the picture. Off, it costs nothing.
