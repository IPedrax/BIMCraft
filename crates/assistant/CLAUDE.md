# bimcraft-assistant

The AI chat of the desktop: a conversation with a model (Anthropic, OpenAI or Google, the user's choice and the user's key) that manages
the project open in the window through the commands, and the second code that reaches the network (the first is
[`bimcraft-update`](../update/CLAUDE.md)). `apps/bimcraft` attaches `net::Ureq` and `crates/ui-egui` draws the palette
(`assistant.rs` there); the core, the CLI and the MCP server do not depend on this crate, and a test in `crates/update` reads every
manifest for that and for `ureq`. How it works for the user, and what it sends where: [the assistant](../../docs/areas/assistant.md).

What comes from the network is hostile, and what goes out is the user's:

- A reply is read up to `MAX_REPLY` and parsed leniently; one that is not what the provider documents is an `Err` for the chat to show,
  never a panic. A tool call's arguments are checked before anything runs (`tools::run`).
- Requests go only to the three hosts (`net::allowed`), over HTTPS, with no redirect followed; the key travels in a header and is in
  no address, no message and no `Debug` (`Request`'s prints no headers). A key or model id with odd characters never reaches a header
  or an address (`key_ok`, `model_ok`).
- What goes out is capped: a tool result (`MAX_RESULT`), the conversation (`MAX_HISTORY`), the replies one message may take
  (`MAX_ROUNDS`) and the calls honoured in one reply (`MAX_CALLS`).
- The tools run on the window's session through `bridge::run`, as a connected agent's do, and refuse what reads or writes a file, what
  replaces the project and what makes a picture: a note inside the project must not steer the model into the user's disk.

- The saved key (`seal.rs`) has two levels. `bc1`, the default, is sealed with AES-256-GCM under a key made from `BIMCRAFT_KEY_SECRET`, which
  the release workflow builds in from a repository secret (a public development secret otherwise): it is no secret from anyone who has the
  program, and the docs and the page say so. `bc2`, which the user may add, is under a passphrase alone (600,000 rounds of PBKDF2), so it
  opens in any build; the two bind different associated data, so neither is taken for the other. A change to a format keeps `well_formed`
  strict (the file is hostile) and its known-answer test, made with Python's `cryptography`.
- A provider is a pure pair: `request(model, key, history) -> net::Request` and `parse(status, body) -> Result<Reply, String>`. A new
  provider or a changed request shape is a test on a recorded reply first (`tests.rs`: text, one call, two calls, malformed, refusal).
  The conversation (`Msg`) is in no provider's form; the model's own turn goes back whole as `Reply::raw`, because thinking blocks,
  reasoning items and thought signatures are valid only in the conversation that made them.
- The turn runs in `Chat::poll`, once a frame: the request is on a thread, its reply is taken in the frame, the tools run on the
  session there, one transaction named `LABEL` for each reply's calls, and the turn's end merges them into one undo step
  (`Session::merge_steps`). The network stays behind `Net` (`testing::Fake`), so no test reaches a provider.
- `attach.rs`: the files sent with a message. A picked file is copied into a `TempDir` that is the chat's (gone on `clear`, `reset` and
  drop), read back as text (UTF-8, `MAX_FILE_TEXT`) or a picture (`MAX_FILE_IMAGE`, base64 by `attach::base64`), and goes out through
  `user_parts`, which each provider writes in its own form. A file's name is cleaned (`clean`) because it is inside `<file name="…">`.
  File bytes are capped apart from the words (`MAX_FILES_TOTAL`, `Msg::file_bytes`). The model has no file tool, and the rule above that
  tools never read a file stands.
- `saved.rs`: a chat kept for each project, in a `chats` folder the window picks (`write`, `read`, `remove`, `Chat::to_saved`,
  `Chat::restore`). The file is hostile: `restore` parses all of it or none (`parse`), within the live chat's caps, and a new
  `Line`, `Msg` or `Reply` field is written and read there with a test. Pictures are not kept (`msg_json`), the key never is.
- `Reply` carries `thinking` (the provider's summary, shown apart from the answer; `raw` is still what goes back) and `usage` (`Usage`,
  what the provider counted: the chat hands it over with `take_usage` and the window keeps it by project). A new provider reads both.
- `todo_write` (`tools::TODO`) is answered by `tools::run` but its list is kept by the chat (`Chat::todos`), not logged as a call and
  not an undo step. The schema has an array of objects, which Gemini takes as it is.
- Look at `tools::all()`'s schemas when adding a tool: plain types only (Gemini takes a subset of OpenAPI; `gemini::schema` writes a
  free-form object as text that the tool reads back).
- A Co-Work session shares the host's chat ([Co-Work](../../docs/areas/cowork.md)): `Chat::send_shared` runs a message as another
  person's (their name on the undo step, the session's author swapped while the calls run) on a key that is `Pays::Key` here or
  `Pays::Remote` in another window, which counts its own tokens. `wire.rs` is the chat as it goes between windows: the saved forms
  with pictures kept, read as strictly. Another payer or provider than the last starts the model afresh, told the earlier turns
  as text (`fold`). Tested in `shared_tests.rs`.
- `cargo test -p bimcraft-assistant` runs in about a second.
