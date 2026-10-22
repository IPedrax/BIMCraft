# Point clouds

On the roadmap under 0.9 Interoperability. So far the core only reads a cloud; nothing in a project holds one yet.

Point clouds (`cloud.rs`) come from public specifications: PTS and XYZ text, PLY (text or either byte order) and ASPRS LAS 1.0
to 1.4 uncompressed (`Format::from_extension` says why LAZ, E57 and ReCap's RCP/RCS don't). `cloud::read` parses a file twice,
once to measure it (`Stats`: the count, the bounds, the ranges of intensity and colour) and once to keep it: every `stride`th
point so `Options::cap` is never passed, then one in each cube of `spacing`, the same every time. A `Scan` keeps its points as
`f32` about `origin`, the middle of its bounds, so a survey's coordinates far off keep their millimetres within `MAX_SPAN`;
colour comes in eight bits whether the file has eight, sixteen or nought to one, and intensity alone is grey.
