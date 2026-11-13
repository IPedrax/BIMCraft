# Co-Work

Live co-editing of one project: a host's window holds the model, guests connect to it, and everyone sees the others' edits,
cursors, cameras and selections, and shares one AI chat. The whole design, the decisions and the build order are in
[COWORK-PLAN.md](../../COWORK-PLAN.md). This page says what is built.

## Built: the engine's side (slice 1)

`crates/core/src/cowork.rs`, with its tests in `cowork_tests.rs`. Nothing here touches the network.

- **Deltas.** `cowork::delta(before, after)` says what a command changed, for a guest's copy that has `before`: the elements
  alone (`Delta::Elements`: each one as it is now with its id and GUID, each id gone, the next id) when nothing else changed,
  else the whole model as a file holds it (`Delta::Whole`). It reads `Model::changed_since`. `cowork::apply(copy, delta)` takes
  one in and checks the result as a command's model is checked; a delta that doesn't fit (an element the copy never had,
  another element's GUID, a door left with no wall, a next id behind an element's or past `LINKED`, a model that doesn't read)
  is an error and leaves the copy as it was. A `Delta` is JSON with `deny_unknown_fields`, so a host is read as strictly as a
  command. The test that guards it runs a session of walls, doors, moves, a type, undo, redo and delete, and holds a copy fed
  only the deltas, through JSON, to the host's model file for file.
- **A guest's command on the host.** `cowork::run_for_guest(session, guest, command, params, made_in, edit)` runs it as the
  guest (`Session::author`), in the guest's phase and option, and gives the session back as it was. It refuses what would
  replace the project, and while it runs `Session::execute` refuses any command that reads or writes a file (what
  `touches_files` marks, or any `path` or `save` given), a plugin's nested commands too, so a guest never reaches the host's
  disk; a guest editing a link's `path` is refused by the same rule. With `edit` false (the host lets guests only look) an
  edit is refused with `cowork::LOOK` and what reads still answers.
- **Each person undoes their own edits.** While `Session::author` is set (a session's host sets its own name, and
  `run_for_guest` the guest's), every step records who made it (`Step::by`), and `undo` and `redo` take back the author's own
  (`Session::undo_own`) instead of the top of the history. Outside a session they are as they always were.
  - Undo picks the author's last edit still standing, wherever it is, and puts what it changed back as it was before it, as a
    new step on top named `Undo <label>`. Redo picks their last undo still standing, unless an edit of theirs came after it,
    and makes `Redo <label>`. `count` takes several, all or nothing.
  - It is refused, and nothing changes, when anything the step changed is no longer as the step left it: an element someone
    changed, removed or gave back since, or the project's settings when the step changed them and they have changed since.
    Comparing what is there now with what the step left handles undos and redos made by others in between without special
    cases. The error names the last step after it that touched those elements, with its author and label, and the elements:
    "can't undo your Wall: Bo changed wall 100234 after it (Bo's Move)".
  - It is refused too when the model it would leave fails the check (Bo put a door in Ana's wall, and her undo would take the
    wall away); then the error lists whose steps came after it.
  - Settings are compared as a whole (`Model::same_settings`, which `Model::diff` uses): two steps that both changed any
    setting conflict even when they changed different types. That is safe and sometimes too strict.

## Built: the interface's funnel

`BimApp::exec` and `exec_batch` (`crates/ui-egui/src/lib.rs`) are the one way the desktop changes the project: `run` goes
through `exec`, and the dialogs that ran several commands as one step (Visibility Set, Fill Patterns, Line Styles, Electrical
Settings, Draw's runs) go through `exec_batch`. `tests/cowork.rs` reads the interface's sources and fails on any other
`self.session.execute`, `transaction` or `script`. It is where a guest's commands will go to the host.

## Built: the transport (slice 2)

`crates/cowork` (`bimcraft-cowork`; its rules for agents are in [its notes](../../crates/cowork/CLAUDE.md)). Nothing uses it yet:
the desktop wires it in slice 3.

- **The code** (`Code`): 80 random bits, written `7KQ2-M9XD-4TPA-R3HC` in Crockford's base 32, read back in any case, with or
  without dashes, and with O, I and L taken for 0, 1 and 1. It never goes on the wire. `Code::tag` is 32 bits of HMAC-SHA256
  under it, which the host says on the LAN so a guest who typed only the code finds the host holding it.
- **The handshake** is Noise `NNpsk0_25519_ChaChaPoly_SHA256` on `ring`, with the code's key as the pre-shared key: a wrong code
  fails the host's check of the guest's first message, and each connection has keys of its own. It matches cacophony's published
  vector for that protocol byte for byte. After it, every frame is sealed with a counter as its nonce, so a changed, dropped,
  reordered or replayed frame ends the connection.
- **A host** (`Host::start`) listens where it is told (0.0.0.0 and the session port by default; anything beyond that, such as
  forwarding a port, is the user's), lets in a guest with the right code and the same version, at most eight, and hands the
  window `HostEvent`s: `Joined` (the guest's number, its name made safe, its colour by join order, its hello), `Message` and
  `Left` (with why: it left, was kicked, fell silent, flooded, broke the line). The window sends with `send` and `broadcast`, and
  `kick`s. Presence is relayed to the other guests by the guest's own thread, with `from` added, so it never waits for the
  window. Dropping the host says `ended` ("the host closed the session") to every guest and waits until that is out; `end` says
  a reason of the window's.
- **A guest** (`Guest::join`) does the handshake, says the window's hello and waits for the welcome, or returns why not: the
  code, the version, a full session, a host that didn't answer. Then `GuestEvent`s: `Message`, `Ended` (the host said so, and
  why) and `Lost` (the host went without a word).
- **Both sides ping** after 5 seconds of saying nothing and take 15 seconds of hearing nothing as a broken line. A guest that
  floods (more than 100 messages in a second), sends what isn't a message or stops taking what it is sent is dropped; an address
  that gets the code wrong five times waits 30 seconds.
- **Discovery** (`discover`): a guest sends a small "who hosts?" UDP datagram to the targets it is given (the window will give
  the LAN's broadcast address and the discovery port), and each visible host answers with its name, its project's, its version,
  its session port and its code's tag. Nothing secret goes either way, and an answer that isn't one is passed over. A host whose
  discovery port is taken still hosts, out of sight (`Host::discovery_port` is none).

## Built: hosting and joining in the desktop (slice 3)

- **A session on a `Session`** (`bimcraft_cowork::live`, tested with no window in `live_tests.rs`). `Hosting::start` makes the
  host's steps its own (`Session::author`), and `pump`, once a pass, lets guests in with the whole project (`welcome`, a
  `cowork::whole` delta with the linked models in it), runs their `execute` and `batch` as theirs (`run_for_guest`,
  `batch_for_guest`) and answers each after the delta of what it changed, sends what changed since the last pass whoever changed
  it (the window, its agents, the assistant), the whole model again to a guest that lost step (`resync`), and the people (a
  name another has gets a number). It stops sending once the window holds another project. `Joined::join` returns the copy
  of the host's project to put in the window; `exec` and `batch` run what only reads on it and send what edits to the host
  (`cowork::place`), waiting up to 10 seconds for the answer and taking in the deltas before it; `pump` takes in the rest in
  order, asks for the whole model again on a gap or a delta that doesn't fit, and says `Over` when the host ended or went.
- **The window** (`crates/ui-egui/src/cowork.rs`): Collaborate > Work Together > Co-Work, CW and Find open the Co-Work
  dialog, in both profiles. Not in a session, it hosts this project (Start Co-Work) or joins one: the sessions found on the
  network are listed (asked every 3 seconds while it is open), and the code alone picks the host on this network; from another
  network the host's address goes in too, with a port after a colon if it isn't 41730. Joining with unsaved changes is refused:
  it would put the host's project in the window. Hosting, it shows the code (Copy), this machine's address and the port, Guests
  may edit, and who is in, each with Kick (who is then kept out for the session); End Session. A guest sees whose session it
  is and who is in, and Leave. The view control bar says "Co-Work: hosting, 2 in" or "Co-Work: Ana's session", and the title
  says it too.
- **File > Settings > Co-Work** keeps the name the others see, the address and port a host listens on (0.0.0.0 and 41730 by
  default; anything beyond that, such as forwarding a port, is the user's), Visible on this network, and Guests may edit.
- **What a guest's window does.** `BimApp::exec` and `exec_batch` send a guest's commands through `Joined`; the project is
  read-only in the window otherwise (`cowork::GUEST`), so nothing changes it but what the host sends. Autosave is off (the
  host's keeps the project). An agent on a guest's control port only reads, by the same read-only. When the session ends the
  project stays, read-only, to save as a file of the guest's own; a project the user opens after is theirs to change.
- **Answering while minimised.** `BimApp::pump` answers agents, pumps the session and polls the assistant. `App::logic`
  calls it, which eframe runs while the window is hidden too (at least every 100 ms when woken), and the session's threads
  wake the window when anything comes; `show` calls it as well, for the tests that drive `show` alone.
- **The host opening another project, or closing,** ends the session: guests are told "Ana opened another project" or "the
  host closed the session".

## Built: seeing each other (slice 4)

- **A presence** (`cowork::Presence` in the core): the view on someone's screen (its kind and the GUID of its level, section,
  sheet or schedule), their pointer on a plan as a model point, the plan's zoom, their 3D camera (every field of
  `view3d::Camera`, as `cowork::Look`) and their selection as GUIDs. `Presence::check` refuses what can't be drawn (a number not
  finite or past the model's range, a camera no 3D view could have, more than `MAX_SELECTION` GUIDs), and a window keeps only
  what passes. `Look::eye` is where a camera looks from, for drawing it in another's 3D view (back along its line in a parallel
  view, which has no eye). `Scene::boxes` is the box each of some elements takes in a scene, from their triangles.
- **Who is where** (`bimcraft_cowork::live`): `Hosting` and `Joined` keep the others (`Other`: name, colour number, presence),
  the host as 0 with colour 0 and the guests in join order. `present` sends the window's own when it changed; the transport
  hands a guest's to the other guests at once, and a newcomer gets everyone's right after the people. Every delta says who made
  it (`by`), so a window lights what someone else changed (`Happened::Edited`), never its own.
- **The window** (`crates/ui-egui/src/cowork.rs`) sends where it is at most 15 times a second (`my_presence`: the view, the
  pointer on a plan, the plan's zoom, the 3D camera, the selection), and draws the others in their colours, eight distinct ones
  none of which is the selection's blue or the hover's:
  - on a plan, the pointer of each person on that plan, an arrow with their name, and what anyone has selected or just changed
    (lit for a second and a half), outlined over the drawing (`paint_plan`, from `marks`);
  - in 3D, what anyone has selected, boxed (`Scene::boxes`, worked out again only when the scene or the selections change),
    and for each person in a 3D view a dot where they look from, a dashed line to what they look at, and their name
    (`paint_3d`);
  - on the view control bar, a badge for each (their initial in their colour; its tooltip says their view and how much they
    have selected). A click follows them: their view, their plan's pan and zoom or their 3D camera, until the user moves the
    view themselves, which the window says;
  - in Properties, "Ana has this selected" in Ana's colour, over an element someone else has selected too.

## Built: the shared AI chat (slice 5)

- **The chat is the host's.** Its conversation and its tools run in the host's window, on the host's session, as they do outside
  a session. A guest's message comes as `chat_say` (`Happened::ChatSay`), with its files in the form `bimcraft_assistant::wire`
  writes (pictures and all, read strictly), and runs as `Chat::send_shared`: its edits are the guest's (the session's author is
  swapped while a reply's calls run), and its undo step is `Assistant (Bo): <message>`, Bo's to undo. A guest who may only look
  has a turn that only reads. Anyone may stop a turn (`chat_stop`); only the host starts a new chat. A message sent while a turn
  runs is refused, and the guest's window says so.
- **Everyone sees it.** After each change (the chat's revision, or whether it is busy) the host's window sends the whole of it,
  the log, the checklist and whose key it runs on, as `chat` (`Hosting::chat`), and keeps it for whoever joins next. A guest's
  palette shows that (`cowork::Ai::shown`), not a chat of its own, and sends what is typed to the host.
- **Who pays** (`live::payer`, a pure function): whoever has a key uses their own; one who has none is lent one, the host's
  while the host lends it (Settings > Co-Work and the session's dialog: Guests without an AI key may use mine, on by default),
  else the first guest in join order who lends; nobody, and the sender is told to set a key. A guest tells the host only that it
  has a key, for which provider and model, and whether it lends it (`ai_key`, `AiKey`), never the key.
- **The key stays home.** A turn on a guest's key asks that guest's window for each reply (`Lend::ask` on the assistant's
  thread, `ai_request` and `ai_reply`, through a `Mailer` the host's threads share); that window makes the request with its own
  key (`bimcraft_assistant::complete`, on a thread), at most `MAX_LENT` a minute, and counts the tokens. A guest who leaves ends
  the turn: "Bo left: ask again".
- **Asked once.** Before a guest's key pays for someone else's message, its owner is asked, once a session ("Lend your AI
  key?"); Settings > Co-Work keeps a standing answer (Ask me first, Always, Never). Their own messages on their own key need no
  ask, only the palette's notice of where a session's messages go.
- **Across keys.** A provider's reasoning is valid only where it was made, so a turn on another key or provider than the last
  starts the model afresh, with what was said before told as text in the message (`chat::fold`), and the log keeps all of it.

## Built: the rest of the gaps

- **Undo lists.** In a session the Undo and Redo lists show what that person's undo takes back (`Session::own_steps`: their
  edits still standing, and to redo the edits their undos took back, named as those edits). The host's window reads its own
  session; a guest's has its lists from the host (`history`, sent when they change, read strictly into `live::Named`).
- **A refused undo** lights what the other person changed after the step, in their colour (`BimApp::light_refused`, which finds
  the ids in the core's error, "…: Bo changed wall 100004 after it").
- **The host is asked before it closes** while guests are in ("End the Co-Work session?", `quit_window`); a host with nobody in
  closes as any window does.
- **Pointers on sections and sheets.** A presence's `cursor` is in the drawing of the view it is over: a model point on a plan, a
  point of a section's drawing or of a sheet's paper. Each view draws the others' pointers and selections over its drawing
  (`paint_plan`).
- **Run Script for a guest** goes to the host (`script`, `cowork::script_for_guest`): its dry run there first, then the script as
  one undo step, the guest's. The window's scripts go through `BimApp::exec_script`, which the funnel test counts.
- **A guest's imports.** Import CAD, Import Mesh, Point Cloud, Insert from File and Import Fill Patterns of a file on a guest's
  machine send the file (at most `MAX_IMPORT`, 32 MB) with the command (`import`); the host writes it into a temporary folder of
  its own, runs the command on it as the guest's step and removes the folder (`cowork::import_for_guest`). Only the file's own
  name is kept, of plain characters. A link would point at that folder, so linking stays the host's, and a command that needs files
  beside the one sent (an OBJ's MTL, a glTF's buffers) finds them missing.

## Built: asking to join, and handing the session over (slice 6)

- **Ask to Join** (the Join dialog, with an address typed or a session picked) knocks without the code (`knock.rs`). The handshake
  is the session's own Noise `NNpsk0` under a key everyone knows (`knock::psk`), so it is encrypted but proves nothing alone. Both
  windows then show six digits made of the handshake's hash and a nonce from each side, the guest's committed to (a hash of it sent
  first) before it sees the host's; so someone in between, who runs a handshake with each, can't search for a pair whose digits
  agree (one in a million, and a turned-away knock counts toward the address's lockout). The host's window asks "Someone asks to
  join" with the digits (`knock_window`); Let In tells the guest the code, and it joins with it as anyone does. At most
  `MAX_KNOCKS` wait at once, one an address, each for `KNOCK_WAIT`. Settings > Co-Work and the session's dialog have Others may ask
  to join without the code (on by default; off, a knock is a wrong code).
- **Make Host** (the session's dialog, beside Kick) and **Hand Over and Close** (the question a host with guests in is asked when it
  closes) hand the session to a guest (`Hosting::hand_over`): what changed goes out first, so its copy is the host's model; its
  window hosts that copy as Settings > Co-Work says (`take_over`) and answers with its port and code (`hosting`); the old host sends
  the others there (`moved`, with the address it sees the new host at) and its own session ends (`handed_over`). The others' windows
  join the new host by themselves (`move_to_new_host`). The undo history and the AI chat stay with the old host: the new host starts
  both afresh. The new host's project is its copy, with no file until it saves one.

## Not built yet

Nothing of the plan. Not checked by any test: a session between two machines (and a handover to a machine the others can't reach),
a minimised host on Wayland, a live provider call on a lent key.
