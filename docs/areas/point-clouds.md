# Point clouds

On the roadmap under 0.9 Interoperability. A project holds scans and places them, and the 3D view draws them; plans and
sections don't yet.

Point clouds (`cloud.rs`) come from public specifications: PTS and XYZ text, PLY (text or either byte order) and ASPRS LAS 1.0
to 1.4 uncompressed (`Format::from_extension` says why LAZ, E57 and ReCap's RCP/RCS don't). `cloud::read` parses a file twice,
once to measure it (`Stats`: the count, the bounds, the ranges of intensity and colour) and once to keep it: every `stride`th
point so `Options::cap` is never passed, then one in each cube of `spacing`, the same every time. A `Scan` keeps its points as
`f32` about `origin`, the middle of its bounds, so a survey's coordinates far off keep their millimetres within `MAX_SPAN`;
colour comes in eight bits whether the file has eight, sixteen or nought to one, and intensity alone is grey.

A project holds each scan once, by name, as a `Cloud` in `Model::clouds` (a library entry of kind `Cloud`, so it has an id and a
GUID): its points and colours shared behind an `Arc`, the file it came from, and where its middle was there. A project file holds
it as `CloudFile`, the points as little-endian `f32` and the colours as bytes, each deflated and in base64, packed once and kept
(`Cloud::file`); coming back, every check `Cloud::new` makes is made again, and what unpacks past `MAX_CLOUD_POINTS` stops there.
A project holds up to `MAX_CLOUDS` scans and `MAX_CLOUD_POINTS` points among them (`Model::check`), so the `point_cloud` command
thins a new one to what the others leave.

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
that, as Revit's density does. The view fits round its scans, and a level's plane reaches past them (`Model::model_extent`).
Views hide and override point clouds by element or by the category `pointcloud` (`graphics::SCANS`, apart from `MODEL`:
Revit has a tab of its own for them in Visibility/Graphics, and Object Styles no row). The graphics card doesn't draw them
yet: `gpu::Mesh::new` refuses a view that shows one, and the processor draws it. Sections and the wireframe style leave them
out.
