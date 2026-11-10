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

- A provider is a pure pair: `request(model, key, history) -> net::Request` and `parse(status, body) -> Result<Reply, String>`. A new
  provider or a changed request shape is a test on a recorded reply first (`tests.rs`: text, one call, two calls, malformed, refusal).
  The conversation (`Msg`) is in no provider's form; the model's own turn goes back whole as `Reply::raw`, because thinking blocks,
  reasoning items and thought signatures are valid only in the conversation that made them.
- The turn runs in `Chat::poll`, once a frame: the request is on a thread, its reply is taken in the frame, the tools run on the
  session there, one transaction named `LABEL` for each reply's calls, and the turn's end merges them into one undo step
  (`Session::merge_steps`). The network stays behind `Net` (`testing::Fake`), so no test reaches a provider.
- Look at `tools::all()`'s schemas when adding a tool: plain types only (Gemini takes a subset of OpenAPI; `gemini::schema` writes a
  free-form object as text that the tool reads back).
- `cargo test -p bimcraft-assistant` runs in about a second.
