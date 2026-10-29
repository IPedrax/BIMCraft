# The desktop app: browser, docking, settings, picking, snapping, clicks, startup, tags

Moved from the root `CLAUDE.md`, word for word: the root keeps the rules and the map, and this file is read when the area is touched.

The Project Browser (`browser.rs`) builds its tree as `Node`s and draws it through its tab (`browser::Tab`) and its search
(`browser::spans`, wildcards and all, a pure function with its tests): a new kind of entry is a `Node` in `browser_tree`.
Palettes dock and float (`dock.rs`): `Docking` is where they are (stacks of tabs down the left or right side or along the
bottom, or floating windows), apart from its drawing, so tests and the settings file read it; a new palette is a row of
`Palette` and its body in `palette_body`. A view's tab floats it in a window (`BimApp::floating`; `views` are the tabs and
`front` the one the views' area shows): the active view (`BimApp::sheet`) takes the tools, any other on screen draws through
`view_pane` as it is, with the canvas `at_rest` and `passive` set so it takes no clicks, and a click makes it active. Widget
ids inside a view are per view (`push_id`), so two views on screen never clash. The start page is `start.rs` (`BimApp::home`; the desktop's `main` shows it when given no file, the quick access bar's Home and Ctrl+D bring it back over a project and go back to it, `BimApp::toggle_home`, and `Home::over` says there is one behind it): its tiles' pictures are drawn on a thread (`start::picture`, a `.bimcraft` file's 3D view) and a test waits for them with `Home::settled`. File > Settings is `settings.rs`, the user's
`settings.json` beside the favorites (`BimApp::use_settings`, which the desktop's `main` calls; tests never write it unless
they hand it a file): what it keeps is `Settings`, written when it changes, between drags; a setting still to come goes on
its page greyed out (`later`).

View > Dark Mode opens a small theme menu: As the System, Light, Dark, or Dark UI, Light Canvas. The same choices are in
Settings > User Interface. The last choice keeps plans, ceiling plans, sections, elevations and 3D views on a light background
while the interface stays dark; sheets stay white paper in every mode. `Settings::light_canvas` is saved alongside `theme`
in `settings.json`, defaults to false for older files, and is only read with the dark theme (`Settings::from_json` drops it from
any other, so a hand-edited file never leaves the menu with no choice picked). `style::dark` chooses interface colours;
`style::canvas_dark` controls canvas ink, fill and background conversion, and the section/3D image cache keys. Both software
and GPU 3D painters use the canvas background. What the drawing paints in a palette (the fill of what is selected or hovered,
a crop region's outline) takes it from `style::canvas_pal`, which follows the canvas, not `pal`, which follows the interface:
`DARK`'s fills are near-black on a white drawing. A light canvas has a palette of its own (`CANVAS_LIGHT`: a deeper hover stroke and
selection tint than the interface's `LIGHT`, which keeps the tints it always had, so the plain Light window's chrome is unchanged). A scan is dimmed only on a dark canvas (`canvas_dark`), so a dark interface
around a light drawing leaves it as it is.

New and Open (`BimApp::reopened`) keep the look: a new `BimApp` would run `style::apply` again, which sets the theme
preference back to the system's (and `keep_settings` would then write that over the user's choice), so `reopened` carries
`styled`, `theme_wanted` and `light_canvas` over.

The system's file dialogs (Open, Save As, the exports, Load Family, Link Model, a material's picture) are asked for through
`BimApp::ask_file` (`dialog.rs`), never with a blocking `rfd::FileDialog`: a native dialog run inside the window's event
handler nests a run loop in it, and on macOS a drag reaching the window meanwhile re-enters the handler and aborts the app
(issue 43). The dialog (`rfd::AsyncFileDialog`) is begun on the window's thread, where AppKit wants it and where a sheet does not
block, and waited for on a thread of its own; `ask_file` is given what to do with the path, and `BimApp::dialog_answered` does it in a later frame (a cancel does nothing; one dialog is open at a time). A step
that goes on after a save (the unsaved-changes prompt's Save) is that `then`. A file dropped on the window is queued in
`opens`, as one the system asked for. A test hands the app a `Dialog` whose answer it sends itself (`tests/dialog.rs`).

The window's size (`window.rs`, issue 37) is `Settings::window` (its size in points and whether it was maximized, never its place:
Wayland tells an app none, and the window starts centred): `BimApp::watch_window` notes it once a frame, keeps it once it has held
still for half a second, and writes nothing for a window left as it started; `main` starts the window at the size the file holds
(`settings::window_in`). A window too big for its screen (the default 1440 x 900 on a laptop's, or a kept size after the screen
changed) is made smaller, once, in the first frame that knows the screen (`fitted`: 96% by 90% of it, never below the least a
window can be), and centred when it has taken the size.

The control port (`control.rs`, issue 41) is how an agent works on the project the window has open: `bimcraft --control PORT` binds
only 127.0.0.1, makes a token of 128 random bits and writes both to `bridge.json` beside the favorites (for the user alone; the
window takes it away when it closes), and `bimcraft-cli mcp --connect` runs its tools on the window's session. The protocol, and
what an agent may not do (`bridge::REFUSED`: `new`, `open`, `recover`, `highrise`, by any name, and a script is read through before it
runs), are `bimcraft_core::bridge`. A connection thread reads a line (at most 16 MB), checks the token, and hands the request to the
window through a channel; `BimApp::answer_agents` runs it in the frame as `BimApp::run` runs a button's command (`made_in`, then
`Session::execute`) and sends the reply back, up to sixteen a frame, and not while a file is being read or an update installed. A
request wakes the window (`Context::request_repaint`, from the context the first frame left in a `OnceLock`). Eight connections at
most; ten seconds, in all, to say the first line whole (a byte now and then does not count), and what is not a request, a web page's
HTTP say, is answered with an error and closed. A test (`tests/control.rs`) plays the agent on a real connection while the harness runs frames.

The interface's language is `lang::Lang`: a project's language, or Simplified Chinese (issue 38), which only the window speaks
(a drawing's lettering, `font.rs`, is drawn in code and has no hanzi, so a project is never in Chinese and `Lang::project()` gives
it English). Its words, including the schedule headings shown in the window, are in `translations/ui/*.po`;
missing or unfinished translations show English. Project text has separate catalogs; see [translations](translations.md).
egui's own fonts have no hanzi, so the window in Chinese needs a font the system has: `lang::use_chinese_font` puts one behind egui's
(`lang/font.rs` tries each system's known places, then the Linux font folders, taking Noto Sans CJK's Simplified face) the first frame the
language is Chinese. Where there is none, `set_lang` leaves the language as it was and the status says why, and a settings file that says
Chinese on such a machine starts the window in English. The app holds the finder (`font_finder`) so a test can say there is none.

What a view hides or reveals for now (Temporary Hide/Isolate, Reveal Hidden Elements; `footer.rs`) is the window's, per `Sheet`, never the
project's: the plan, the section and the 3D view draw `BimApp::view_model`, a copy of the model whose view graphics went through the `graphics`
command on a scratch `Session`, made once for each model and each setting. A tool that needs the view's real model (select all, commands)
still reads `session.model`. The Door and Window tools preview with `opening_of`, which the `door` and `window` commands insert too, so a
sketch is the element the click makes (`render::sketch`). A selected door or window shows two temporary dimensions from its middle
to the faces of the walls meeting its wall either side of it, or to its wall's ends where none does (`Model::opening_span`), as Revit's
measure to wall faces; typing one counts from that face. Placed or dragged alone within `SNAP` px of the middle between those faces, a
door or window lands on it (`Model::centred`): the click sends that `offset`, the drag slides by what reaches it.
A selected column or component, and the Column and Component tools' sketches, show up to four: how far it stands from the nearest wall centre
line or grid east, north, west and south (`Model::clearances`, which finds them in the core; `standing_dimensions` draws them); a typed one runs
`move` by the difference along that way.

Align (AL) highlights the exact edge or axis under the cursor before either pick, leaving the selected reference as an
extended dashed line. It searches the candidates under the pointer for a usable line rather than letting annotation mask
one. `align::lines` in the core supplies wall faces and ends, opening jambs, grids, column faces and axes, structural and
MEP runs, components, slab and roof boundaries, stairs, ramps, railings and detail geometry. Straight segments of a
polyline or sketch are usable; curved edges are not replaced with straight chords. The `align` command turns the target
by the smaller angle about the picked line's midpoint and translates it onto the reference, as one undo step through the
usual transform and validation path. Hosted doors/windows can slide along their wall, and hosted railings follow their
stair or ramp. A refusal keeps the reference selected. On the second pick a floor's or region's model pattern remains
alignable when its line is closer than the boundary; picking the boundary aligns the element instead.

While a sketch is active, Align takes its reference from either model geometry or another sketch line, with a coincident
draft line taking priority over the saved boundary. Its second pick edits a sketch line. The same exact segment highlights
on hover before either pick, including straight segments of polylines. `align::sketch_lines` applies the rigid alignment to
the draft curve, keeps its id, GUID and style, and moves joined endpoints so a closed boundary stays connected. A sketch
reference stays fixed. Changes use the sketch's Ctrl+Z history and reach the model only through Finish Edit Mode; Cancel
leaves the model as it was. Invalid geometry leaves the draft and reference intact, and undo or Escape drops stale picks.

The sketch's contextual ribbon includes Align, Trim/Extend to Corner, Trim/Extend Single/Multiple Elements, Split and
Offset. Trim highlights sketch candidates and keeps its first pick lit; its prompt says which line to pick and to click
the part to keep. A failed trim leaves that first pick available for another target; a successful edit clears the error.
Multiple Trim keeps the boundary across successive target edits while that indexed line is unchanged, and drops it if
another edit removes, renumbers or changes it. Escape clears the first pick; Ctrl+Z takes back each trim as one draft step.

A point picked in a plan lands where `snap::snap` says, the core's so every front end agrees: on the end, middle or centre of
what's drawn (`Scene::of`), a round detail line's quadrants (`rounds`: a circle's, an ellipse's or an arc's points along its axes),
where two straight lines (walls, grids; not arcs) or a detail line's chords cross (never a line's chords with each other), where a
line from the last point touches a circle or an arc (`tangents`), else a scan's point where the cut plane cuts it
([point clouds](point-clouds.md)), the foot of the perpendicular from the last point
to one, a guide through that point at
a multiple of `Snaps::angle` from east or from the line before it (where the guide crosses a wall or grid it lands there, and a
grid point lying on it beats a rounded length), a wall's centre line or the nearest point of a detail line, else the nearest 10 mm.
Revit's Quadrants (SQ) and Tangents (ST) are snaps of their own, in Manage > Snaps with the rest. The desktop's `BimApp::snap` gives
it the reach in px (`SNAP`, `PICK`) and the snaps in force (Manage > Snaps, kept while the window is open, or for one point
a shortcut's override: `snap_once`, `Snaps::only`, `Snaps::none`); `found` and
`show_snap` draw what a pick would land on, and a point that moves with the cursor goes through them. A selected wall's ends are
grips (`grip_drag`): a drag runs `move` with `end`, Ctrl keeps the length; the angle beside each end runs `rotate` about it. A
wall joined at the end that moves keeps its joint, which slides along the wall it meets, so lengths change.

A click is made where the button was, not where the hand has got to. egui reads a click's position, and what lay under it, from the
pointer's last position in the frame, and counts two clicks in a frame as one, so a hand that moves on in the time between frames
(fast drawing) would click elsewhere, or once: `BimApp::pace` (run by `raw_input_hook`, with `show` asking for the frame that takes
the rest) cuts a window's events after each button press or release, and tests hand events over through `deliver`. egui also takes a
press and release more than six points apart for no click (a drag, if a frame falls between), which a hand clicking on the move
makes, so for every tool but Modify that is a click where the button went down (`BimApp::clicked_at`, where the plan, sections and
sheets take their clicks), and what lay under it is read there (`under_at`), not where the hand has got to. In Modify a drag is its
move and its box, so only a press let go within `QUICK` (0.2 s) is a click wherever the hand went: the drag it began is not
finished (`Canvas::quick`, `BimApp::finish_drag`: grips, crop regions, a sheet's views, the plan's move and box) and the selection
that drag's start replaced comes back (`BimApp::grab`) for the click to pick from. A slower press is the drag it is. In the 3D view,
where every drag orbits, a quick press is a click whatever the tool, and the camera the hand turned meanwhile goes back
(`Orbit::before`).

Startup (`apps/bimcraft/src/startup.rs`). A release build on Windows has no console, so a start that fails must say itself: `bimcraft.log`
and `graphics.json` sit beside the favorites (`startup::Files::beside`). `startup::begin` runs once the arguments are read (never for
`--version` or `--help`, which return before it), writes the log's first line and the panic hook, and picks the graphics for this start
(`Mark`: the choice, and whether the last start finished). A start is marked unfinished until its window has stayed up for `SURVIVE` (15 s)
or the app is closed the ordinary way (`startup::up`, `closed`); one that dies without a word, as a graphics driver can make it, leaves the
mark, and the next start takes the next choice on `LADDER` (wgpu's own, DirectX 12, Vulkan, OpenGL: Windows only), keeping the first that
survives. `graphics.json` keeps the backend's name (`{"backend": "DirectX 12", "starting": false}`), never its place on the ladder, so
reordering the ladder cannot change what a kept mark means; 0.8.0's marks (`"choice"`, a place on the old order) are read through that order. An error from `run_native` leaves the mark too, is logged, and on Windows shown in a message box (`startup::tell`: its words are
entries in the interface's PO catalogs). `WGPU_BACKEND` set by the user switches all of it off. A new way for the start to fail goes through `startup::note`, not `eprintln!` alone.

Tags. A `Tag` names an element at a `host` by a `label`: the name of one of the fields its category's schedule has (`schedule::field_text` reads it
as the schedule shows it, in the project's units), or, with none, the category's usual one (`Tag::usual`: a door's or window's `Mark`, a room's `Name`, else
the `Type`), `?` while that has no value. The host is anything `Model::anchor` finds a point for (a door or window's centre, the middle of a wall's or a run's
centre line, a column's, a component's or a room's point, the centre of a floor's, ceiling's or roof's outline, a stair's or ramp's start): a new kind of host is an arm there,
and `Model::check` refuses a tag whose host has none, or whose `label` isn't a field of it, saying which fields there are. A tag sits `offset` mm from that point (`Model::tag_offset`),
is drawn in plans only (a door's in a box, a window's in a hexagon, any other's as plain text, with a leader line when it has one), follows its host, and goes
with it. The Tag tool picks whatever `anchor` finds a point for, and the Options Bar's Label field is the `label` the click passes on; Tag All (`tag` without `ids`)
still takes doors and windows unless given a `category`.

Type Properties (`typeprops.rs`). Every kind of type opens in one dialog, after Revit's: `title`, then `head` (Family, shown and not picked, and
Type, with Duplicate… and Rename… beside it; either opens a Name row under it, Enter or its button doing it), then `table` (Parameter and Value
under group bars that fold, `Table::row` and its helpers for a field, a check box, a drop-down, a dragged number, a colour, a value worked out, and
`Table::full` for what spans both columns, a type's layers or a sample), then `foot` (what stopped it, and OK, Cancel and Apply). Each dialog keeps its
state and its commands: the project's own types and a family's in `properties.rs` (`type`, `family`, and `rename_type` for Rename), text types in
`notes.rs` (`text_type`, with `rename`), filled region types in `fillpatterns.rs`, where Duplicate and Rename change the draft OK applies and a
pattern's … opens the Patterns tab. The dialogs are one width, and the table's body one height where the window has room, known from the first frame:
a body that fitted its rows grew over a few frames, and a click meant for one row landed on the next. A refusal is said in the dialog.

The section box (`section_box.rs`). The 3D view's Properties have an Extents group: Section Box, a checkbox, and while it's on the
West, East, South, North, Bottom and Top faces as lengths in the project's units; BX turns it on or off (opening the 3D view). They run
`section_box`. In the view, `orbit.rs` draws the box's edges and, in Modify, an arrow on each face pointing out along its axis (a square
for a face square to the screen); a primary drag from one (`section_box::grip_at`, within `GRIP` points) moves that face by
`Camera::along` and `move_box_face`, the picture cut to the box as it goes (`Look::with_section_box`, part of the `Shot` it caches), and
runs `section_box` when the drag ends. The grip takes the drag, not the camera, and a click on a grip picks nothing behind it. A
plan's (and a plan view's) Properties have Section Box under Extents too, a `Flag` row: ticked, the plan shows what the box keeps.

Sketching in 3D (`sketch3d.rs`). Edit Boundary on a floor, ceiling, roof by footprint or shaft selected in the 3D view keeps the
sketch there, as Revit does: the lines are drawn over the picture on the element's own plane, `Model::sketch_height` (a floor's
top at its level and offset, a sloped one's too; a ceiling's underside; a roof's base; a shaft's bottom), and `Sketch::plan` is
`Sheet::View3D`, so the window comes back to the 3D view until Finish or Cancel as it does to a plan (`BimApp::sketch_plan`,
while the element still has a plane). The view turns, pans and zooms as ever, and its clicks pick no elements: `sketch3d::Plane`
takes a point on screen to the plane (`Camera::on_plane`) and back. Modify picks a line where it shows (within `PICK` points of it
on screen, so a foreshortened line is as easy to hit as any), and a primary drag from a picked line's end holds that grip, not the
camera (`BimApp::sketch3d_grab`, before the camera moves), the ends there moved where it is let go; a drag from anywhere else
orbits, so there is no selection box. The Draw panel's shapes, Pick Walls and Pick Lines (the element the picture shows under the
click), Split, Trim/Extend, Offset and Vertex take the plane's point as the plan takes its own, through `sketch_click`,
`sketch_modify` and `vertex_click`, snapped by `snap::in_sketch` in the element's level with reaches from the view's scale.
Align, which lines up with what a plan shows, says so. A view edge-on to the plane picks nothing on it, and says to turn it.

Unhide. Only Reveal Hidden Elements lets what a view hides be picked, so the Modify tab's View panel offers Unhide Element and Unhide Category (EU, VU) for a selection
`BimApp::hidden_selection` finds the active view hiding: the elements hidden one by one, and the categories hidden whole. They run `graphics` with `show_elements` or `show`
(`unhide_selection`), the selection stays, and with nothing of it hidden EU opens the Elements tab as before and VU says so.

Filter. While something is selected, every tab's Select panel has Filter, and the funnel by the status bar's count opens it too
(`filter.rs`): Revit's Filter, the selection counted by category, each checked to keep or cleared to leave out, with Check All,
Check None and the total kept; OK keeps what is checked selected. Its Filter by list goes further: any parameter every selected
element has, which `selection::common` finds (the schedules' fields their categories share, Category first, Count left out; what
schedules aren't of, lines, regions, text, tags, grids, views and sheets, has its fields in `schedule::MORE`: a line's Line Style), and
the list becomes that parameter's values (`selection::groups`, as schedules show them, numbers by value, `(none)` last), to keep or
leave out the same way. The `filter_selection` command gives agents the same: the groups, the shared fields, and with `keep`, the ids kept.

Undo's list. The arrows beside Undo and Redo (`BimApp::steps_list`) list `Session::steps`, each a `Step` the core keeps with its
undo snapshot: the command's label and the elements it made and took away, by category, which `step_name` turns into Create Walls
or Delete Door (else the label). A click runs `undo` or `redo` with `count`.

Tabs. Ribbon and view tabs are pills (`tab_button_with`): the open one in the accent's soft tint, a contextual one solid green
when open and tinted green when not. A view's × is part of its tab (`tab_button_with`'s `room`). The ribbon's tab row scrolls
sideways with the wheel in a window too narrow for it.

The look. `style.rs` holds the tokens: the palettes, light and dark, with a dark app bar; the
card frame (`style::card`: 8 px corners, a hairline border and a soft shadow, standing `GAP` off the edges it's told to); IBM Plex Sans
and Mono (`assets/fonts`, SIL OFL 1.1; built into the executable, so `NOTICE` names them with their versions and sha256 and every package
carries it and both OFL.txt, as `copy_docs` in `packaging/env.sh`, `bimcraft.wxs` and `portable.ps1` do: `tests/fonts.rs` checks), with
`style::semibold` for titles and tabs (the body's face until the window has loaded it, a frame after `apply`). The semibold face is a
family of its own, and egui does not fall back across families, so the Chinese font joins every family in `style::families` (`lang::use_chinese_font`). `BimApp::show` paints the active view's ground (`backdrop`) under everything, then the app bar (Home, File, the
Quick Access Toolbar and the title, light on dark, `style::on_app_bar`), and the ribbon, the Options Bar, the status bar and each docked
palette stack as cards, so the gaps between them read as the view going on beneath; the view control bar is a small card in the view's
corner. The Options Bar (`BimApp::options_bar`) floats over the view's top left, an `Area` above it, while there's something on it (a
contextual tab's name, a tool's options): coming and going it takes no room, so the view never moves under a drag when a selection
fills it, and a click on it never reaches the view. File > Settings > Layout picks Floating (the cards) or Compact (`BimApp::compact`, kept in `settings.json` as
`layout`): the same pieces flush to the edges and square, `style::gap` and `style::card` reading it each frame (`style::follow`, given the `Look` the user kept: Compact and the Light Canvas).
Text holds 4.5:1 and a focus ring or an icon 3:1 on what it sits on, in both themes (`style::tests` computes WCAG's ratio for every pair the
window draws): `accent` is for fills, strokes and the drawing, `accent_ink` for the accent as text, an icon or a focus ring
(`style::focus_ring`, which on the app bar is the bar's lettering colour), `on_context` for the open contextual tab's lettering. A new pair of
colours goes in that test.

The title bar (`titlebar.rs`). On Windows and Linux the window draws its own (`BimApp::own_title_bar`, set by `main`, which opens
the window without the system's decorations): the app bar is the title bar. Its bare ground moves the window and a double-click
there maximizes or restores it (`titlebar::drag_area`, laid out before the bar's buttons so they keep their presses), Minimize,
Maximize (Restore Down when maximized) and Close sit flush in its right end (`caption_buttons`, 46 points each, Close red under the
pointer), and `titlebar::frame`, over everything, resizes the window from 4-point zones along its edges (`titlebar::EDGE`; corners twice as long,
winning where they meet) and draws a hairline round it; a maximized or full-screen window has neither. The zones sit over the window and win
the hit test, so they are as thin as the room each layout leaves round its controls (the app bar keeps 5 above its buttons, the cards stand off by
`GAP`, a Compact card 3), the top edge stops short of the caption buttons and the right edge starts under them (`titlebar::zones`, given
where `caption_buttons` put them this frame), and a Compact card docked right pads its scrollbar in by the same 4. The titlebar tests run in
the window's real geometry (`own` fills the window, with none of the margin `app()` keeps). Moving and resizing are the
system's (`StartDrag`, `BeginResize`), so Windows' snapping works, and Close is the system's Close, so unsaved work is asked about.
The start page has a slim bar of its own (`start_title_bar`). Settings > User Interface > Title bar asks for the system's instead
(`system_title_bar`, kept in `settings.json`, read by `main` before the window opens and switched at once with `Decorations`);
macOS keeps its own.

The least a window can be is `window::MIN_SIZE`, 900 by 560 points (`main` gives it as the window's minimum), and nothing runs under, over or
past anything else there (`tests/smallest.rs`, in the window's real geometry): the app bar's title sits between the quick access and the
caption buttons, in the window's middle if it fits there and cut short with an ellipsis if not; the ribbon's tab row scrolls, and a tab just
picked (a contextual tab comes last of all) is scrolled into view; the Options Bar wraps to a second line, and a view's navigation bar
steps under it where a drop-down (which doesn't wrap) still reaches over it (`BimApp::nav_corner`); and a stack of palettes standing over another leaves it `dock::LAST_MIN`, so its last card never runs under the status bar,
even when the window was brought down from a taller one. Nothing is fitted to a size below that: the window is asked not to go there.

The ribbon's panels (`ribbon.rs`). A panel's name, under its tools, is a button (named `Build ▾` for screen readers) whose list
(`ribbon::list`) draws the same tools again as rows: `ribbon::listing` turns `big_button` and `small_button` into `list_row`. A tab too
wide for the window makes room from the right, on the last frame's widths (`ribbon::tab`, `ribbon::fits`): a panel hands its last tools
over to its list (`ribbon::handed_over`: counted, not drawn), and when one tool left isn't room enough it folds to one button that lists
them all, and the panel before it is asked; what still doesn't fit scrolls sideways. A tool that drops a list of choices down (Project
Units, Schedules, Duplicate View, New Family...) goes through `ribbon::choices`, never `Popup::menu` on its own button: egui keeps one
popup open, so a popup of a row's own closes the panel's list that holds the row, and with it itself. In a list the choices are a
submenu of the row; a request to open them at once (UN) opens the panel's list when the tool has no button, and waits for a tab's second
frame, since its first lays out every tool (`a_tool_with_choices_of_its_own_opens_them_from_a_panels_list`).

Keyboard Shortcuts. Manage > Shortcuts (or KS) opens `shortcuts_window` (`shortcuts.rs`): every shortcut by what it does, from `shortcuts()`
(`SHORTCUTS`, the fixtures' letters, the commands' aliases), with Add, Edit and Load File. A shortcut is two letters or digits, all that `typed` holds,
and a custom one is tried before the defaults. Each name in `ACTIONS` is a tool or command the window runs (`execute_shortcut_action`), and a key never
carries Revit's name for a tool BIMCraft lacks (Sun Settings, Detail and Model Line, Split Face, Brace, Beam System, Reference Plane, the duct tools): it waits
for the tool. Load File reads JSON (`{"WA": "Wall"}`), Revit's `<Shortcut CommandName=… Shortcuts="WA#W1">` lines, or `KEY - Name` lines, up to 64 KiB;
a description is an action only by its whole name in one of the interface's languages (`resolve_action`), and a key that is not two characters, names no action or
is another action's is skipped and counted in the message. Custom shortcuts last while the window is open. Match Properties (MA) is the `match_properties`
command; with one element selected the window remembers its type for the next one drawn, with several it gives the others of the first's category its type.

Annotation dialogs and draw tools. Manage > Fill Patterns (`fillpatterns.rs`, also Edit Type of a filled region) and Manage > Line Styles
(`linestyles.rs`) edit a draft of the project's entries and send it as one transaction: what is deleted and made again under its name is changed in
place, and renames go through `renames::plan`, which runs a chain from its end and a swap through a spare name (the core refuses a rename onto a name
that is taken). A draft keeps the entries the project held when it opened and is refused over a project that has changed since (an agent's command),
so Apply never undoes or deletes what the draft never saw. A blank or repeated name is refused in the dialog. What an import or an export says
(`note`) is said in the dialog, not in the status bar, which can lie over its buttons. A PAT file is read by the `import_fill_patterns` command on a scratch
project that holds the draft's names, so the command's keep or replace rule and its caps are the dialog's too (Replace Same Name is the command's
`replace`); an export to a name without `.pat` does not write over a file of that name with `.pat`. The preview draws a type as the plan does, in the canvas colours.
The note belongs to the Patterns tab (the types do not show it), a file's patterns are told in the singular for one, and a left-out pattern is named with its reason (the
core's message without the pattern's name it begins with), each cut at a word. In a test a file dialog is answered through `dialog::BEGUN`; no system dialog opens.

A dialog that holds a long list (these two, Edit Type, and Line Styles) never runs off the window: the list, and a fill pattern's families open under it, scroll in
the height the window has left (`style::dialog_room`: the window's height less the dialog's frame, what is drawn above the list, and what is drawn under it, the notes
and the row of buttons), so OK, Cancel and Apply stay in reach down to the least window, and the note and the error are boxes that scroll (`style::boxed_text`). When a
fill pattern's families are open they share the room with the list, half each. Line pattern segments scroll in their separate creator. `tests/smallest.rs` opens each with forty of everything and a long
note at the least window and asks that every button lie inside it. The room is worked out from the heights of the rows, not measured after layout, so a label that wraps
in a language with long words can take a row or two more than it reserved: the dialog then scrolls a little instead of fitting exactly.

The window is on one project, the `uid` of the model it was built on (`BimApp::project`): a model of another `uid` in the session's place (a script's or an embedder's
New or Open; the control port turns an agent's away, `bridge::REFUSED`) starts the window over (`reopened`), so a sketch or a dialog's draft of the old project cannot be
finished into the new one, and gives the new project a recovery copy of its own (`autosave_document`). The replaced project's own recovery copy is not asked about: it stays until the new project's first snapshot is written and is deleted then (`select_destination` retires it, `cleanup_retired_autosaves` removes it), where the old code wrote the new project over it; File > New asks about unsaved changes first and then retires the copy at once, and a copy found on disk and not yet answered (a candidate in the Recover dialog) is never deleted this way. Hover and selection are drawn in the canvas's palette, not the interface's (`canvas_pal`): on a light canvas inside a dark interface the dark palette's
blues were 2.5:1 on white; `style`'s tests keep the two told apart and 3:1 on the paper in `CANVAS_LIGHT`, and keep `LIGHT`'s tints as they were. A section's picture is drawn first and once a frame, and the drawing over it
(`render::section_over`) takes its regions' ink from that picture (`Cut::draw`), so a pan or a zoom costs one picture; a hover that changes along with the view draws it a
second time.

The Draw panel's tools leave nothing behind: `set_tool` drops Insulation and Revision Cloud (the Detail Line tool's) and the lines picked for Fillet Arc and
Trim/Extend (`forget_picks`), a sketch's picks are dropped when its lines change (`track_sketch`, once a frame), except an unchanged Multiple Trim boundary, and when it ends, and a shape's typed numbers are
dropped when the shape changes. A fillet that does not fit keeps its two lines picked and spends the digits; a click that sizes the arc clears them. A number of the
Options Bar that the shape in hand takes and cannot read (`options_error`) is red and makes nothing. A sketch keeps the lines as they were before each change
(`Sketch::track`), so Ctrl+Z takes back a fillet, a trim or a grip move whole; it belongs to the plan it began in, and the window comes back to that plan until Finish or
Cancel Edit Mode (`BimApp::sketch_plan`; a plan whose level was deleted is nowhere to come back to, and holds neither the window nor the tool: a view that cannot use the tool takes Modify, as for no sketch). egui draws a Dot (a dash of a hundredth of a millimetre) about a pixel and a half long, so it shows; `a_dot_of_a_hundredth_of_a_millimetre_still_draws_about_a_pixel_on_the_canvas`
fails if that changes.

Limits of the annotation stack, left as they are: the desktop has no group type rename, delete or edit (the `group_type` command does them), and a group taken
apart with Ungroup and made again with Create Group cannot take its old name (the core refuses a name in use), so what was placed from the old type does not follow;
detail groups and repeating details are placed in plans only; Properties' Group row is read only. Pick Lines and the polygons are Draw-panel tools that make
curves, not commands, so an agent draws the curves they would make. A sketch's Trim, Offset, Split, Fillet, Join, Vertex and Convert Spline (`sketch_modify` and its neighbours
in `draw.rs`) act on the sketch the window holds with the core's `draw` and `curves` functions and are not commands, so the CLI and MCP cannot reach them. The Align tool on a
pattern asks `Model::align_pattern` where the pattern then starts and sends the answer through `modify`, saying the core's refusal when it gives one. The patterns list is not virtualized: rows off the screen paint
no swatch, but each still lays out its widgets. A sketch's Ctrl+Z takes back at most the last 100 changes (`sketch::UNDO`); an older one is forgotten. The window tells a replaced project by its `uid`, so
`open` of the very file the window already holds, put in the session's place by a script or an embedder, is not noticed: the file keeps its `uid`, and a sketch or a dialog's draft survives it
(the core's `Session` counts its replacements in a private `resets`; a public reader of that count would let the window see this one too).

The Line Styles Patterns tab opens a separate Line Pattern Creator for New Pattern and Edit. Its preview uses the core
pattern geometry (including font outlines) and can follow a curve. The creator saves into the parent draft, with its own
Cancel, so OK/Apply remains one undoable Line Styles transaction. See [views and graphics](views-and-graphics.md) for the
seven segment kinds, paper units, text fitting and centerline semantics.
