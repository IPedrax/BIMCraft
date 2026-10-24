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
it English). Its words are `lang/zh.rs`, English keys with the Chinese, and the core's words (schedule headings) stay English there.
egui's own fonts have no hanzi, so the window in Chinese needs a font the system has: `lang::use_chinese_font` puts one behind egui's
(`lang/font.rs` tries each system's known places, then the Linux font folders, taking Noto Sans CJK's Simplified face) the first frame the
language is Chinese. Where there is none, `set_lang` leaves the language as it was and the status says why, and a settings file that says
Chinese on such a machine starts the window in English. The app holds the finder (`font_finder`) so a test can say there is none.

What a view hides or reveals for now (Temporary Hide/Isolate, Reveal Hidden Elements; `footer.rs`) is the window's, per `Sheet`, never the
project's: the plan, the section and the 3D view draw `BimApp::view_model`, a copy of the model whose view graphics went through the `graphics`
command on a scratch `Session`, made once for each model and each setting. A tool that needs the view's real model (select all, commands)
still reads `session.model`. The Door and Window tools preview with `opening_of`, which the `door` and `window` commands insert too, so a
sketch is the element the click makes (`render::sketch`). A selected door or window shows two temporary dimensions to the ends of its wall.
A selected column or component, and the Column and Component tools' sketches, show up to four: how far it stands from the nearest wall centre
line or grid east, north, west and south (`Model::clearances`, which finds them in the core; `standing_dimensions` draws them); a typed one runs
`move` by the difference along that way.

A point picked in a plan lands where `snap::snap` says, the core's so every front end agrees: on the end, middle or centre of
what's drawn (`Scene::of`), where two straight lines (walls, grids; not arcs) cross, else a scan's point where the cut plane cuts it
([point clouds](point-clouds.md)), the foot of the perpendicular from the last point
to one, a guide through that point at
a multiple of `Snaps::angle` from east or from the line before it (where the guide crosses a wall or grid it lands there, and a
grid point lying on it beats a rounded length), a wall's centre line, else the nearest 10 mm. The desktop's `BimApp::snap` gives
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
mark, and the next start takes the next choice on `LADDER` (wgpu's own, Vulkan, DirectX 12, OpenGL: Windows only), keeping the first that
survives. An error from `run_native` leaves the mark too, is logged, and on Windows shown in a message box (`startup::tell`: its words are
rows in `WORDS`). `WGPU_BACKEND` set by the user switches all of it off. A new way for the start to fail goes through `startup::note`, not `eprintln!` alone.

Tags. A `Tag` names an element at a `host` by a `label`: the name of one of the fields its category's schedule has (`schedule::field_text` reads it
as the schedule shows it, in the project's units), or, with none, the category's usual one (`Tag::usual`: a door's or window's `Mark`, a room's `Name`, else
the `Type`), `?` while that has no value. The host is anything `Model::anchor` finds a point for (a door or window's centre, the middle of a wall's or a run's
centre line, a column's, a component's or a room's point, the centre of a floor's, ceiling's or roof's outline, a stair's or ramp's start): a new kind of host is an arm there,
and `Model::check` refuses a tag whose host has none, or whose `label` isn't a field of it, saying which fields there are. A tag sits `offset` mm from that point (`Model::tag_offset`),
is drawn in plans only (a door's in a box, a window's in a hexagon, any other's as plain text, with a leader line when it has one), follows its host, and goes
with it. The Tag tool picks whatever `anchor` finds a point for, and the Options Bar's Label field is the `label` the click passes on; Tag All (`tag` without `ids`)
still takes doors and windows unless given a `category`.

Unhide. Only Reveal Hidden Elements lets what a view hides be picked, so the Modify tab's View panel offers Unhide Element and Unhide Category (EU, VU) for a selection
`BimApp::hidden_selection` finds the active view hiding: the elements hidden one by one, and the categories hidden whole. They run `graphics` with `show_elements` or `show`
(`unhide_selection`), the selection stays, and with nothing of it hidden EU opens the Elements tab as before and VU says so.

Undo's list. The arrows beside Undo and Redo (`BimApp::steps_list`) list `Session::steps`, each a `Step` the core keeps with its
undo snapshot: the command's label and the elements it made and took away, by category, which `step_name` turns into Create Walls
or Delete Door (else the label). A click runs `undo` or `redo` with `count`.

Tabs. The open ribbon tab and the open view tab mould out of what they open: `rule_under_tabs` draws the rule under a row of tabs
but for the open one, whose sides run down into the surface below. A view's × is part of its tab (`tab_button_with`'s `room`).
