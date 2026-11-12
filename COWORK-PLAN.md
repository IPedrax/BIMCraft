# Co-Work: live co-editing of one project

Planned on 2026-11-12 against `origin/main` at 733dd127, and revised the same day with the user's answers. Slices 1 and 2 are
built (the engine's side, the interface's funnel and the transport); what they do is in [docs/areas/cowork.md](docs/areas/cowork.md).

Sizes follow DISCUSSIONS-PLAN.md: S is a day or two, M about a week, L more.

## What was asked

- The host opens a project and starts Co-Work. The window opens a port that other BIMCraft users connect to.
- Guests find hosts on the same LAN without typing an address, and join with a code. From outside the LAN they join with the
  host's IP and port. The window listens on 0.0.0.0 by default; anything beyond that (forwarding, firewalls, VPNs) is the
  user's to set up, and the app doesn't try.
- It works like Google's editors: you see the others live. Their mouse on a 2D view, their camera moving in 3D, what they
  select, and their edits as they make them.
- Everyone in the session shares one AI chat. By default it runs on the host's API key, and the host can switch guests to their
  own keys instead. When the key in use isn't there, it runs on the key of the first person to join who has one, then the next,
  in join order. A guest is asked once before the session spends on their key.
- Undo takes back your own edits only. It fails when someone else edited the same thing after you, and says who.
- A minimised host keeps answering. A host that closes tells every guest it disconnected.

## Decisions

Confirmed by the user on 2026-11-12:

- **The AI key.** The host chooses whether guests' messages run on the host's key (the default) or on their own. A guest is
  asked once per session before their key pays for anyone else's message. Slice 5.
- **Undo** takes back your own last edit wherever it sits in the history, and fails, naming who, when someone changed the same
  element or the same project setting after it. Slices 1 and 3.
- **A minimised host keeps answering; a host that closes says so.** Slice 3.
- **Outside the LAN is the user's business.** The window listens on 0.0.0.0 at its port, and the address and port can be
  changed in Settings. No router advice, port mapping or VPN help in the app. Slice 3.

Still recommendations, which the rest of the plan assumes:

1. **The host's window holds the one true model.** Guests keep a copy to draw from and send their edits to the host as
   commands. The host runs them through `Session::execute`, the way it runs an agent's, and sends everyone what changed. The
   alternative is to merge edits made on each copy (a CRDT), and that fights the engine: every command is checked against the
   whole model by `Model::check` and rolled back when it fails, which only works when one place decides. Slices 1 to 3.
2. **The join code has 16 characters and doubles as the encryption key.** `7KQ2-M9XD-4TPA-R3HC` is 80 random bits, used as the
   pre-shared key of a Noise handshake (`Noise_NNpsk0_25519_ChaChaPoly_SHA256`) written on `ring`, which the workspace already
   depends on (the assistant's `seal.rs`, the updater). Everything on the wire is encrypted, and a wrong code fails before the
   host reads a byte of what the guest sent. The 80 bits are what make that safe: with a 6-digit code, someone in the middle
   could try every code offline against one captured message, so a short code would need a PAKE (`spake2`, a new dependency).
   The alternative to writing the Noise pattern ourselves is the `snow` crate, which brings several new crates
   (`curve25519-dalek` among them). Ours is held instead to cacophony's published test vector for this protocol, byte for byte,
   with fixed keys through `ring`'s test randomness, so it needs no new crate at all.
   About "the port code": a port number alone is no secret, so the code takes its place. On the LAN the code alone is enough,
   because discovery finds the host that holds it. From outside, the guest types the host's address and port with the code.
   Slice 2.
3. **An API key never leaves its owner's machine.** The conversation lives on the host. When a turn runs on a guest's key, the
   host sends that guest the conversation, the guest's machine makes the request to the provider with its own key, and the
   reply comes back. Sending keys to the host would be less code, and it would hand a guest's credential to another person's
   machine. Slice 5.
4. **Fixed default ports**: one TCP port for sessions and one UDP port for discovery, both changeable in Settings. Proposed 41730
   and 41731: check them against the IANA registry before shipping, and stay clear of 47808 to 47823, which BACnet uses. A fixed
   default means a user who forwards a port forwards the same one every time.

## How it works

### Shape

```
  guest A                         host window                         guest B
  copy of the model  <--TCP-->   Session: the model, undo, checks   <--TCP-->  copy of the model
  draws, sends commands          runs every edit, sends deltas                 draws, sends commands
                                 relays presence and the chat
                                 the shared conversation (tools run here)
```

It's a star: guests connect to the host and never to each other. At most 8 guests, as the control port's `MAX_CONNECTIONS`.

### Finding a host and joining

- **Start.** Collaborate > Start Co-Work (shortcut CW). The window listens on the address and port Settings > Co-Work keeps
  (0.0.0.0 and 41730 by default), makes a code, and shows the code with a Copy button, the port, and the machine's LAN address.
  The LAN address comes from the system's routing (a UDP socket connected to a documentation address, then `local_addr`;
  nothing is sent). The dialog says nothing about routers, and BIMCraft never asks a server for a public address, since that
  would phone home.
- **Discovery.** While hosting, the host answers on UDP 41731. The Join dialog broadcasts a small "who hosts?" datagram once a
  second and lists the replies with the sender's address: the host's name, the project's name, the version, and a tag (32 bits
  of HMAC-SHA256 of the code under a fixed label). Typing just the code picks the host whose tag matches; clicking a host asks
  for its code. The tag says nothing about the code. Hosts answer only while Settings > Co-Work > Visible on this network is
  ticked (on by default). Guests bind no fixed port, so two windows on one machine can both look.
- **Join.** Collaborate > Join Co-Work takes a code alone (on the LAN), or an address, a port and a code; an IPv6 address goes in
  brackets. Broadcast discovery stays inside one subnet, so across VLANs or the internet the address is typed.
- **Same version only.** The first message names the version, and a mismatch is refused with both versions named. Deltas
  carry elements in memory form, which only the same build reads the same way. No `FORMAT` change: nothing in a file changes.

### The wire

After the handshake, each frame is a 4-byte length (capped) and the ciphertext. Each plaintext is one JSON object with an `op`,
as the bridge's lines are. Snapshots are deflated (`flate2`, already in the workspace) and carried as base64.

| From | `op` | What it carries |
|---|---|---|
| guest | `hello` | version, display name, whether this guest has an AI key and with which provider and model |
| host | `welcome` | the guest's id and colour, the people, the whole model, the delta number, the conversation so far, the undo labels, the AI mode |
| guest | `execute`, `batch` | a command with its params, the guest's phase and option, a reference; `batch` is a label and a list, one undo step |
| host | `done` | the reference, and `ok` or `error` as the bridge's replies say |
| host | `delta` | number, author, the change (elements or the whole model), the undo and redo labels with their authors |
| guest | `resync` | asks for the whole model again (a gap, or a delta that didn't apply) |
| both | `presence` | view, cursor, camera, selection (the host adds who it's from when it relays) |
| host | `people` | who's here, in join order, with colours; sent on every join, leave and kick |
| guest | `chat_say`, `chat_stop` | a message for the shared assistant; stop the turn |
| host | `chat` | a line of the conversation and who it's by |
| guest | `ai_key` | this guest's key changed: there now, gone, locked; and whether it may be lent |
| host | `ai_request` | a reference, provider, model and the conversation, for the guest whose key pays for the turn |
| guest | `ai_reply` | the reference, and the provider's reply or an error |
| both | `ping` | after 5 seconds of saying nothing, so silence means a broken line, either way |
| guest | `bye` | leaving |
| host | `ended` | the session is over, and why: the host closed the window, ended the session, opened another project, or kicked this guest |

### A guest's command

`BimApp::run` is already the funnel (176 calls go through it). On a guest it decides by the command (`engine::find`), in this
order:

| Command | Where it runs | Why |
|---|---|---|
| one that replaces the project (`REPLACES`: new, open, recover, highrise, tower) | refused | leave the session first |
| one that touches a file (`touches_files` says so) | on the guest's copy, under `Session::reading` | an export writes the guest's own disk; an import that would change the project is refused in version 1, since the file is on the guest's machine (slice 6 uploads it) |
| `undo`, `redo` | sent to the host as "undo my own last edit" | see Undo |
| an undo step (`undoable`), a plugin command | sent to the host | it changes the shared model |
| anything else (inspect, schedules, render, measure, the checks, Save as a copy) | on the guest's copy, under `Session::reading` | instant, and it changes nothing |

The guest's copy has `Session::read_only` set, with a reason of its own ("a guest's copy: the host makes every change"). Anything
that slips past the funnel and calls the session directly is refused, so a guest's copy can never drift from the host's in
silence. Deltas go in through a method that bypasses that guard. About two dozen direct `self.session.execute` and
`transaction` calls in ten UI files move to `run` and a new `run_batch(label, commands)`, and a test reads the UI's sources so
no new one comes back.

The guest waits for `done` (up to 10 seconds, with "Waiting for Ana..." in the status box) and applies the deltas that arrive
first. The host writes the delta before the `done`, so when `run` returns, the element the command made is already in the
copy, and the call sites that select it work unchanged. On a LAN the wait is milliseconds; across the internet it's one round
trip per command. That is a click, not a drag, because drags preview locally and commit once.

On the host, a guest's command runs the way an agent's does: with the guest's phase and option set for it, the guest as the
step's author, and under the host's choice of Guests may edit or Guests only look (the same `bridge::Access`). It's refused when
it would replace the project or touch a file. That is the plugins' rule (`touches_files`), and it matters more here: a guest
must never make the host's machine read or write its disk. While a file is being read or an update installed, guests wait, as
agents do.

### Keeping the copies in step

The core already knows what a command changed: `Model::changed_since(&before)` gives either the ids of the elements that
changed or `Changed::All` (a type, a material, a setting). A new pure module in the core turns that into a delta and applies it:

- `Changed::Ids`: each changed element as it is now (id, GUID, the element through serde; `Element` is already `Serialize`,
  `Deserialize` and `PartialEq`), each removed id, and `next_id`.
- `Changed::All`: the whole model (`Model::to_json`, deflated).

Measured with the release CLI: the sample house is 179 KB as a file and 27 KB deflated; the 10,000-element tower is 3.7 MB and
362 KB deflated. A join costs that once, and an ordinary edit costs a few KB.

The host compares the model's `Arc` with the last one it sent, every pass, and also right before each `done`. That catches
every source of change without touching the call sites: the host's own clicks, its agents, the assistant, undo and redo.

The guest applies deltas in order and checks what changed (`Model::check_changed`). A gap or a delta that doesn't apply asks
for the whole model again; a whole model that fails `Model::read` or `Model::check` ends the session with the reason. The undo
and redo labels ride along so everyone's Undo list shows the shared history, with who made each step.

### Undo

Each person undoes their own edits, wherever they sit in the history. Outside a session, undo and redo stay exactly as they are.

- **Who made a step.** `Step` gains `by` and a stable number of its own. The host sets `Session::author` around each guest's
  command; the host's own steps (and its agents' and the assistant's) have none, which reads as the host. A step the assistant
  makes counts as the person whose message started the turn.
- **What a step changed.** The undo stack already keeps the model before every step, so the step's changes are the difference
  between that model and the next one: the elements it added, changed or removed, and whether it changed the project's
  settings. `Model::diff(before)` says both, comparing element by element where `changed_since` can only say "everything".
- **Undo my last edit.** The host finds that person's last step that isn't already undone. When anything it changed is no
  longer as it left it (an element someone changed since, or the settings), the undo fails and nothing changes. Comparing what
  is there with what the step left means others' undos and redos in between need no special case. The message names the last
  step after it that touched those elements, its author and the elements, and the desktop lights them up in that person's
  colour: "can't undo your Wall: Bo changed wall 100234 after it (Bo's Move)". Otherwise the elements and settings the step
  changed go back to what they were before it, as a new step named "Undo Wall" by the same person, checked like any command
  (`Model::check_changed`).
- **What the check catches.** Some edits depend on yours without touching the same element: you made a wall, Bo put a door in
  it. Undoing your wall would leave the door with no host, so the check refuses it, and the message adds whose step depends on
  it.
- **Redo** takes back that person's last undo the same way, and fails the same way. A new edit of theirs clears their redo, as
  it does now.
- **Limits.** A step that has dropped off the 200-step history (`UNDO_LIMIT`) can't be undone: "nothing of yours left to undo".
  `count` undoes that many of your steps and stops at the first that fails.

### Presence

Each window sends what it shows when it changes, at most 15 times a second, and the host relays it from its network threads,
without waiting for a frame:

- `view`: what's on screen (plan, ceiling, section, sheet, 3D, schedule) and the GUID of its level, section, sheet or schedule.
- `cursor`: the mouse as a model point in millimetres, on a plan, a section or a sheet.
- `camera`: the 3D view's `azimuth`, `elevation`, `scale`, `target` and `lens` (`view3d::Camera`; `Camera::eye` already exists).
- `selection`: the selected elements' GUIDs, since a reference holds a GUID, never an id.

What each window draws:

- **A cursor** where another person points, on the same view: an arrow and a name tag in their colour.
- **A camera** in the 3D view for each person in 3D: a small frustum at their eye, looking at their target, with their name.
- **Their selection**: their selected elements outlined in their colour, in plans and in 3D. The plan's `paint` takes one list
  in the selection's colour today; it gains a list per colour.
- **Their edits**: the elements a delta changed light up in the author's colour for a second and a half.
- **A soft lock**: the Properties palette says "Ana has this selected" over an element someone else is working on. There are no
  hard locks; commands run one at a time on the host, and the last one wins.
- **The people bar**: a round badge per person in the title bar (initials, colour, a mark for the host), in both profiles. Its
  tooltip says the person's view and how much they have selected. A click follows them: your window opens their view and
  copies their camera, or their plan's pan and zoom, until you move it yourself.

Colours come from eight distinct ones, given in join order with the host first, none of them the selection's or the hover's.

### The shared AI chat

- **One conversation, on the host.** The host holds it, and its tools run on the host's session through `bridge::run`, as they
  do now. Every line is sent to everyone with who it's by, and a guest who joins gets the conversation so far. Anyone may send;
  a message sent while a turn is under way is refused with "the assistant is still answering Ana". Anyone may stop a turn, and
  only the host starts a new chat.
- **Whose key pays for a turn.** The host chooses, in the Start dialog and in Settings > Co-Work: Guests use my AI key (the
  default) or Guests use their own AI key.
  - *Guests use my key*: every turn runs on the host's key while it is set and unlocked.
  - *Guests use their own key*: a turn runs on the key of whoever sent the message; the host's own turns run on the host's.
  - *No key where one is needed*: the turn runs on the first guest in join order, still here, whose key is set and who has
    agreed to lend it; when that guest leaves, the next one. With nobody to lend, the sender is told to set a key in File >
    Settings > Assistant.
  - A request in flight when its key's owner leaves fails with "Bo left: ask again".
  - The palette says, for each turn, whose key and which model: "Claude Sonnet 5.5, on Bo's key".
- **Asked once.** The first time in a session that a guest's key would pay for someone else's message, that guest is asked:
  "Let this session's chat use your AI key? What others ask is billed to your account." The answer holds for the session, and
  File > Settings > Assistant > Lend my key in Co-Work keeps a standing one (Ask, Always, Never). A guest's own messages on their
  own key need no ask, only the usual notice of what is sent where.
- **The key stays home.** When a turn runs on a guest's key, each round of it is a remote completion: the host sends
  `ai_request` with the conversation, and the guest's machine calls `bimcraft_assistant::complete` with its own key, provider
  and model, on a thread, and sends back `ai_reply`. The lender checks the conversation's size (`MAX_HISTORY`) and caps requests
  per minute, because a host could otherwise run up its bill. `Msg` gains a JSON form with a round-trip test.
- **One conversation across keys.** Each provider sends its own earlier replies back exactly as they came (`reply.raw` in
  `anthropic.rs`, `openai.rs` and `gemini.rs`), because thinking blocks, encrypted reasoning and thought signatures are only
  valid where they were made. So every assistant turn remembers whose key and which provider made it, and a turn made on another
  key or provider goes back as plain text: the model's words and one line per tool call with its result, folded into a single
  assistant message with no tool ids. A turn always runs on one key from start to end, so its own calls and results keep their
  native form. The model loses the hidden reasoning of other people's turns, and nothing else.
- **Told first.** Each person's palette says, before their first message, that the messages and what the assistant reads of the
  project go to that provider on whose key.
- **Undo.** A turn's step is named `Assistant (Ana): <message>`, and it is Ana's to undo.

### When the host minimises, closes or vanishes

- **Minimised.** Today the window answers agents in `ui` (`answer_agents`, `poll_assistant`), which the system may stop calling
  for a hidden window. eframe 0.36 has a hook for this: `App::logic` runs before every `ui` pass and also while the window is
  hidden whenever `request_repaint` is called, with hidden repaints throttled to 100 ms
  (`INVISIBLE_WINDOW_REPAINT_INTERVAL`). Answering guests, sending deltas, answering agents and polling the assistant all move
  to `logic`, and the network threads call `request_repaint` when something arrives. A minimised host answers within about
  100 ms. Agents on the control port gain the same.
- **Closed.** When the host's window closes (after the unsaved-changes question, if there is one), the Co-Work host is dropped,
  and its `Drop` sends `ended` ("Ana closed BIMCraft") to every guest before it hangs up, as the control port's `Drop` hangs up
  its agents. Ending the session, opening another project and kicking send `ended` with their own reason.
- **Vanished.** A host that crashes or loses its network sends nothing. The host sends `ping` every 5 seconds, and a guest that
  hears nothing for 15 seconds says "Lost the connection to Ana". The two messages differ, so a guest knows whether the host
  left or the line broke.
- **What a guest keeps.** Either way, the guest keeps the copy, read-only, to save as a copy or close. A guest that leaves sends
  `bye`, and the host shows "Bo left".

### Hostile input

Guests have the code, but everything they send is still hostile, and so is everything a host sends:

- A frame is capped before it's read; a deflated snapshot is capped once inflated at the size `open` takes (`MAX_FILE` in
  `commands.rs`, 256 MB).
- Floats are checked finite and within `model::MAX_COORD`; a selection holds at most 10,000 GUIDs; a name is at most 40
  characters with no control characters; a chat message is at most the assistant's `MAX_TEXT` (8,000 characters).
- A guest may send at most 100 messages a second of any kind (presence, at 15 a second, included); past it, it is dropped. The
  host's window handles at most `PER_FRAME` requests a pass, as for agents.
- After 5 failed handshakes, an address waits 30 seconds. A connection that sends nothing whole within 10 seconds of opening
  is dropped, as the control port's `FIRST_LINE` does.
- A guest's command goes through `Session::execute`, which already survives hostile params (`catch_unwind`, rollback, the
  hostile sweep).
- A delta that doesn't parse (`deny_unknown_fields`) or doesn't pass the checks never reaches the guest's model; it asks for a
  resync instead.

## What it reuses

| Need | Already there |
|---|---|
| a remote that edits the window's project, with replies and caps | the control port: `bridge.rs`, `control.rs` (`answer_agents`, `MAX_CONNECTIONS`, `PER_FRAME`, `FIRST_LINE`, threads, `read_line` limits, the hang-up in `Drop`) |
| answering while minimised | eframe's `App::logic` |
| Guests may edit or only look | `bridge::Access` and `Session::reading` |
| what a command changed | `Model::changed_since`, `Changed::Ids` and `Changed::All`, `Model::same_settings` |
| what a step changed, for undo | the undo stack's models (`Session::undo`: each step with the model before it) |
| a guard so a copy never changes by mistake | `Session::read_only` |
| a guest never touching the host's disk | `touches_files`, made for plugins |
| several commands as one undo step | `Session::transaction`, `merge_steps` |
| elements as JSON, compared | `Element`: `Serialize`, `Deserialize`, `PartialEq` |
| encryption, HMAC, random bytes | `ring` (X25519, ChaCha20-Poly1305, HMAC-SHA256), `getrandom` |
| deflate | `flate2` |
| the selection and the active view, read for someone else | `BimApp::desktop_context` |
| a 3D camera's eye | `view3d::Camera::eye` |
| the AI loop, its tools, its caps, the network behind a trait | `crates/assistant`: `Chat`, `tools::run`, `net::Net`, `testing::Fake` |

No new dependency, at run time or in the tests.

## Slices

Each slice ships alone, starts with its failing tests, and passes the three checks in CLAUDE.md.

### 1. Copies that stay in step, and undo of your own edits, with no network (M to L): built

- `crates/core/src/cowork.rs`: the delta (`delta(before, after)`, `apply(model, delta)`), and the rule a guest's command runs
  under on the host (refuse what replaces the project or touches a file, then `Session::execute` under the host's `Access`).
- In the engine: `Session::author`, `Step::by` and a step number, `Model::diff`, and undo and redo of one person's own steps
  (`Session::undo_own`, `Session::redo_own`).
- The UI's direct session calls move to `run` and `run_batch`.
- Tests first:
  - After each step of a scripted session on the sample house (walls, doors, modify, delete, a type edit that gives
    `Changed::All`, undo, redo), a copy that only got the deltas equals the host's model (`to_json` equal).
  - Hostile deltas (unknown keys, an id that isn't there, NaN, a huge `next_id`) never panic and never change the copy.
  - A guest's file command is refused on the host.
  - Ana moves a wall, Bo moves a door elsewhere, and Ana's undo takes back only her wall. Bo modifies Ana's wall after it, and
    Ana's undo fails, names Bo and his step, and changes nothing. Ana makes a wall, Bo puts a door in it, and Ana's undo is
    refused by the check and names Bo's step. Redo after a successful undo restores the wall; a new edit of Ana's clears her
    redo. A type change of Ana's conflicts with a later type change of Bo's.
  - The grep test on the UI's sources.

### 2. The transport, `crates/cowork` (M): built

- New crate, depended on only by the desktop (`crates/ui-egui`, `apps/bimcraft`): the TCP listener (0.0.0.0 by default) and
  connector, the Noise handshake on `ring`, frames, join codes, the `op` messages, the host's relay of presence, `ping`, caps
  and rate limits, UDP discovery. It carries JSON and knows nothing of egui.
- Tests first, all on this machine's address:
  - The code's format round-trips and refuses typos; a wrong code fails the handshake and says so; a tampered or replayed frame
    ends the connection; the handshake matches cacophony's published vector for the protocol.
  - One host and two guests over loopback join, send a command, receive the delta before the `done`, relay presence, leave and
    are kicked; a ninth guest is turned away; 5 wrong codes lock an address out.
  - Dropping the host sends `ended` to both guests; a host whose socket dies with no `ended` is reported lost after the
    heartbeat timeout (shortened in the test, as `Control::start_within` shortens `FIRST_LINE`).
  - Discovery finds a host by its tag and ignores junk datagrams.

### 3. Hosting and joining in the desktop (M)

- `crates/ui-egui/src/cowork.rs`: Start, Join (with the list of hosts found), Leave and End, kick, Guests may edit; the guest's
  funnel; the host's broadcast; the guest's copy kept read-only; Settings > Co-Work (display name, listen address and port,
  Visible on this network, Guests use my AI key or their own); the state on the view control bar next to the agents'; the
  title says "House, Ana's Co-Work". In the Rev profile a Collaborate tab, in BIMCraft's the Tools palette and Find; shortcut
  CW, listed by KS.
- `BimApp` implements `App::logic`, and answering guests and agents, sending deltas and polling the assistant move there.
- Ctrl+Z and Ctrl+Y on any window of a session are "undo my own" and "redo my own"; a failed undo says who, and lights up the
  elements in common in their colour.
- On a guest, the control port answers agents read-only, with a message pointing them at the host's window, and autosave is
  off (the host's covers the project).
- The host opening another project, or quitting, asks first, then ends the session; guests are told why and keep their copy
  read-only, to save as a copy or close.
- Tests first (egui_kittest, two `BimApp`s in one process over loopback):
  - A wall drawn on the guest lands on the host; the host's edit shows on the guest; a direct session edit on the guest is
    refused.
  - A guest's command is answered by `logic` alone, with no `ui` pass, as a minimised window runs.
  - The guest's Ctrl+Z takes back its own wall and leaves the host's; a conflicting one shows the host's name.
  - The host's window closing leaves the guest showing that the host closed it.
  - CW opens the dialog in both profiles; the strings go through `t` and `msg!`.
  - Then two real windows side by side, as in [looking at results](docs/areas/looking-at-results.md), with the host minimised
    while the guest edits.

### 4. Presence (M)

- Cursors, 3D cameras, peers' selections, edit flashes, the people bar, follow, the soft lock in Properties. What a window
  shows of someone else is computed in the core where a front end would need it (which view a presence is on, where a camera's
  frustum goes); the drawing stays in the UI.
- Tests first: a presence with a cursor on the plan on screen draws the peer's colour at that point (pixels); one on another
  view draws nothing; a peer's selection outlines in their colour; follow switches the view and stops on your own pan; hostile
  presence (NaN, a GUID that isn't there, 100,000 GUIDs) is dropped.

### 5. The shared AI chat (M)

- In `crates/assistant`: `Msg`'s JSON form; each assistant turn tagged with its key's owner and provider; the plain-text form of
  a turn made on another key; a remote completion for `Chat`; who pays for a turn, as a pure function of the mode, the sender,
  the people in join order, their keys and their answers to the ask.
- In the desktop: the shared palette, the host's mode, the ask and its standing answer, "on Bo's key", the author in the undo
  step's name.
- Tests first:
  - Who pays: in each mode, with and without the host's key, the sender's key, and lenders who agreed, refused or left.
  - A conversation with a turn made on Anthropic and the next on OpenAI builds a valid OpenAI request holding none of the
    Anthropic reply's raw form, and the same for each pair of providers (recorded shapes, as `tests.rs` does).
  - The key's bytes appear in no frame the host sends (searched, as the assistant's tests search requests).
  - A lender refuses a conversation past `MAX_HISTORY` and requests past its cap.
  - A turn through a remote completion (`testing::Fake` on the lender) runs its tools on the host's session and merges into
    one step, Ana's to undo.

### 6. Later, each on its own

- A guest's import: the file's bytes, capped, sent to the host, which reads them from a temporary folder (S).
- Ask to join: a guest without the code knocks and the host lets them in; needs a PAKE or a fingerprint the two compare (M).
- Handing the host's role to a guest before the host leaves (M).

## Rules and docs that change

- CLAUDE.md's network rule: one sentence that Co-Work (`crates/cowork`) talks only to the peers the user chose, never to a
  server, keeps discovery on the LAN, and is tested on this machine's address. The detail goes in a new
  `docs/areas/cowork.md` and `crates/cowork/CLAUDE.md`, and the module map gains the three new modules. CLAUDE.md stays under
  3,000 words (`agent_docs`).
- `crates/update`'s `network_offenders` test: `bimcraft-cowork` joins the crates only the desktop may depend on.
- `docs/areas/assistant.md`: the shared conversation, the modes, the ask, turns across keys.
- ROADMAP.md, 1.x: Co-Work goes in as live co-editing on a host's window, before the central server with element ownership,
  which it doesn't replace (no server, no offline work, no ownership).
- Translations: every new string through `lang::t` or `msg!`, then `cargo i18n update`.

## Left out

A relay server, port mapping (UPnP, NAT-PMP), firewall rules in the installers, and any help with routers or VPNs: the window
listens on its address and port, and the rest is the user's. Also left out: joining without the code, guests' imports, hard
locks or borrowing elements, voice and video, guests in a browser, the host's plugin commands in a guest's ribbon, and offline
work merged later. Keeping a guest from saving a copy would be theatre, because the guest's machine holds the whole model in
memory anyway.

## Risks to check early

- **Minimised on Wayland.** eframe calls `logic` for a hidden window when winit can tell it's minimised (Windows, macOS, X11).
  On Wayland winit can't tell (`is_minimized` gives none), and a redraw waits for the compositor's frame callback, which a
  compositor may withhold from a minimised window. Check on KDE and GNOME under Wayland at the start of slice 3. If guests
  stall there, the fallback is to let the host's network thread run guests' commands on a `Session` behind a lock that the
  window takes too, which is a bigger change, since `BimApp` owns the session today.
- **Delta exactness.** Every field a command can change must travel. Slice 1's equality test after every kind of command is the
  guard, and a new command kind gets added to the scripted session there. If anything is cached outside `Model` (a scene, a
  drawing), the guest rebuilds it from the model it has, as it does after any command.
- **Undo across settings.** Settings are compared as a whole: two steps that both changed any setting conflict, even a type and
  a material. Per field wouldn't help much, because nearly every settings change also changes the library's entries. It is
  safe and sometimes too strict; a finer diff (per type, per material) can come later if it bites.
- **Link paths.** Settled in slice 1: while a guest's command runs, any command given a `path` or `save` is refused, so a guest
  can't point a link at a file on the host's machine.
