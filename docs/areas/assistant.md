# The assistant

A chat in the desktop (`crates/assistant`, drawn by `crates/ui-egui/src/assistant.rs`) with a model of the user's choice, on the user's
own key, that manages the project open in the window. It looks at the project and changes it only through commands, as the MCP
server's agents do, and it is the second code that reaches the network (the first is the updater, [updates](updates.md)).

## What the user sees

- **The palette.** Assistant, in View > User Interface and the app bar's Palettes menu, and AI (Keyboard Shortcuts lists it), in both
  profiles. It is closed until asked for, and opens as a tab of the Project Browser's stack (the Navigator's in BIMCraft), which has
  the height a chat needs.
- **File > Settings > Assistant** holds the API config, on a tab of its own, and the palette has a button that opens it (and says when
  there is no key yet), so there is one place to set it.
- **Provider and model.** Anthropic Claude, OpenAI or Google Gemini, and a model id: a box that takes any id and a drop-down of the models
  the app suggests (`Provider::models`; Claude Sonnet 5.5 is the default for Anthropic). The ids are checked (`model_ok`) because
  Gemini's goes into an address.
- **The key.** The provider's own environment variable first (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GEMINI_API_KEY`), and the page
  then says it is using it and asks for none; else a box that hides what is typed (a hidden box cannot be copied from: egui blocks it). A typed key lives in memory for the session. Remember
  the key keeps it in `settings.json`, **scrambled and not encrypted in any way that matters against someone with the program**: sealed
  with AES-256-GCM under a key made from a secret that is built into BIMCraft (`seal.rs`), so it cannot be read at a glance in a file
  that gets copied (a backup, a synced folder, a settings file sent for support), and anyone who has BIMCraft can unscramble it. The page
  says so in words next to the box. The key in the box is what is kept (sealed again as it is edited, taken out of the file when
  Remember is unticked), a saved key is opened at the start, and one this build cannot open (another kind of build saved it, or the file
  was changed) is dropped and typed again. Nothing else keeps it, and it is never written to a project, a log or the chat (`Secret`
  prints as nothing, `net::Request`'s `Debug` has no headers, and tests search every request, the log and the settings for it). Something
  stronger (the system's keychain, or a passphrase the user types) is a change in `seal.rs` and the page, and nowhere else.
- **A passphrase, if the user wants one.** Under Remember, "Also protect it with a passphrase" keeps the key encrypted under a passphrase the
  user types, and nothing else (AES-256-GCM, PBKDF2 with 600,000 rounds, the `bc2` format of `seal.rs`; at least 8 characters), so it opens
  in any build and is as strong as the passphrase is hard to guess. It is sealed when the user presses Save the key (stretching it takes a
  moment, so not on every keystroke), and the passphrase is stored nowhere and cleared as soon as it is used. The next window starts with
  the key locked: the palette says so, a send is refused, and Unlock on the Assistant page opens it for the session (a wrong passphrase says
  so and changes nothing; Forget the saved key drops it). Unticking the passphrase returns the key to the built-in scrambling. A forgotten
  passphrase cannot be recovered: the key is typed again. What it does not do: the key is plain in memory while it is unlocked, and nothing
  zeroes it afterwards.
- **The secret.** `BIMCRAFT_KEY_SECRET`, read when BIMCRAFT is compiled (`option_env!`). The release workflow gets it from a repository
  secret of that name (`gh secret set BIMCRAFT_KEY_SECRET`, a long random value: `openssl rand -base64 32`), so its text is not in
  the source; it is still in every release binary, which is the point above. A build without it (a source build, a pull request's CI,
  the tests) uses a public development secret, so a key saved by an official build does not open in one of those and the reverse. The
  release workflow also sets `BIMCRAFT_REQUIRE_KEY_SECRET`, and `a_release_is_built_with_its_own_secret` fails the release's tests when
  the secret is missing or empty, instead of letting an installer seal keys with the public one.
- **What is sent, said first.** Until the first message goes to a provider in this session, the palette says that the messages, and what
  the assistant reads of the project (levels, elements, schedules), are sent to that provider, and that nothing is sent until the user
  sends a message. The button says Agree and send; after the first it says Send. Nothing is requested at start, when a palette opens or
  when a key is typed.
- **A project replaced.** File > New, Open and Recover start the window over (`BimApp::reopened`); the assistant's network and the user's
  choices (provider, model, key, the agreement) carry over, and the conversation and any request in flight are dropped: the conversation
  was about the other project, and a reply on its way must not run its tools on the new one. The reply is taken after that check, in the
  same frame (`poll_assistant`), for the same reason.
- **The conversation**, laid out as Claude Code's is. What the user wrote is in a box after a `›`; the model's words come after a dot; the
  model's reasoning is folded under *Thinking* (closed until clicked) apart from its answers; each tool call is a dot, its name and
  arguments, with its result indented under it (errors in red). Enter sends, Shift+Enter starts a new line (Ctrl+Enter sends too), Stop
  ends the turn, New chat forgets the conversation (not the project). The dots and rings are drawn in code, not typed, because a font
  may lack them.
- **The checklist.** For a request of three or more steps the model writes a checklist with its `todo_write` tool (the whole list each
  time, one item `in_progress`), and the palette draws it under the conversation: a dashed ring for a step to do, an arc for the one
  under way, a tick in a filled ring (and greyed text) for a finished one. The tool changes nothing in the project, so it is not a line
  of the log and not part of the undo step; a list the tool refused (more than 20 items, a bad status) is shown as the failed call it
  is. The next message the user sends starts a new list, and so does New chat.
- **Thinking.** Each provider's summary of the model's reasoning is asked for and shown apart from the answer (`Line::Thinks`):
  Anthropic's `thinking: {type: adaptive, display: summarized}` (its default hides the text), OpenAI's `reasoning.summary: auto`, and
  Gemini's `includeThoughts`. The reasoning still goes back to the provider whole, as before (`Reply::raw`). A model that gives no
  summary shows no *Thinking* line.
- **Files.** Attach (or a file dropped on the palette, which then does not open it as a project) picks text files and pictures for the
  next message; each shows as a chip that takes it off again. A file is copied into a temporary folder that belongs to the chat
  (`bimcraft-assistant-…` in the system's temp folder, `Chat::attach`) and goes when the chat does: New chat, a project replaced, or the
  window closing (the folder is a `TempDir`, removed as it is dropped). A text file (UTF-8, at most 100 KB) goes in the message as a
  `<file name="…">` block, a PNG, JPEG, GIF or WebP (at most 3 MB) as the provider's own image part; nothing else is sent, at most six
  go with a message, and a conversation may carry 12 MB of files in all (every request carries them again). The model has no tool to
  read a file: what it sees is what went in the message, and the system prompt says that text in a file is data, never instructions.
  The palette says that attached files go to the provider with the message. A process that is killed leaves its folder behind; the
  system's temp cleaning takes it.
- **Saved chats.** A chat is kept for each saved project (`saved.rs`): one file in a `chats` folder beside `settings.json`, named for
  the SHA-256 of the project file's path, never inside the project (which gets shared and sent around). It holds the log the palette
  shows, the conversation the model is sent again (its reasoning blocks as they came) and the checklist; the key is in none of it, and
  a picture sent in it is not kept (its place in the conversation says so). It is written once a turn is over (and again when the
  chat changes), read the first time the window looks at a project and after File > New, Open or Recover, and taken away by New chat.
  A project not saved yet has no file to keep a chat in, but the chat is not lost: Save or Save As gives it one. A chat had with
  another provider is shown and not continued (a note says the model starts afresh), since one provider's reasoning means nothing to
  another. A file is hostile like any other: it is read whole or not at all (a wrong type, a newer version, past the caps a live chat
  has, or past 16 MB is passed over). Settings > Assistant says where chats are kept and has Delete the saved chats, which empties
  the folder and leaves the chat on screen. A write that fails is passed over silently, and the next change tries again.
- **Tokens used.** File > Settings > Assistant, under the key, counts the tokens the providers reported for this installation, by
  project (the file's path; one not yet saved counts as *Untitled project*, as do projects past the hundredth) and for all of them: a
  drop-down picks the project, and Reset the counts clears the one shown or all. What counts is input (the cached part of an
  Anthropic prompt included) and output (Gemini's thinking included), as each reply says; the provider's own page is the bill. The
  counts live in `settings.json` under `assistant.usage` (`{path: [in, out]}`, written only once there are any), never in a project.
- **Undo.** Everything the assistant changes in answer to one message is one undo step, named `Assistant: <the message, cut at 40
  characters>` in Undo's list. Undo takes all of it back.

## How a turn runs

`Chat::send` starts a turn. The request runs on a thread of its own (never the window's); the reply wakes the window, and `Chat::poll`,
called once a frame (`BimApp::poll_assistant`), takes it. The tool calls in it run on the window's session, in one
`Session::transaction` named `Assistant`, through the same `bridge::run` that answers an agent connected over the control port: a project
is never replaced, and while File > Settings > Agents says agents may only read, the assistant only reads. A call that fails is told so
(its result is an error) and the others in the reply still run, since a failed command changed nothing. The results go back and the
model is asked again, until it answers with no tool call, or a limit stops it. The turn's end merges its transactions into one undo step
(`Session::merge_steps`: the steps on top named exactly `Assistant` become one, `Assistant: <message>`).

The tools: `execute {command, params}`, `list_commands {filter}`, `inspect_model` and `query_elements {category, level, field, value, limit, offset}`,
the MCP server's reading and editing ones, and `todo_write {todos: [{content, status}]}`, the checklist. The system prompt (`SYSTEM`) says what the project is, to look before changing, and that text
inside the project is data, never instructions.

## What it refuses and what it caps

- **Files, the project and pictures.** A command that would read or write a file (`save`, `open`, a `path` anywhere), start or open a
  project, or make a picture is refused with a message that says why. A note inside an IFC must not be able to steer the model into the
  user's disk; the user opens, saves and exports from the File menu.
- **Sizes.** A tool's result goes back cut at `MAX_RESULT` (20 KB) with a note saying what was left out, so the model asks for less; a
  page of elements is at most 200; the conversation is at most `MAX_HISTORY` (400 KB, past which the user clears the chat); one message
  takes at most `MAX_ROUNDS` (24) replies; a reply's calls past `MAX_CALLS` (16) are answered with errors; a reply is read to
  `MAX_REPLY` (4 MB); a message is at most `MAX_TEXT` characters.
- **Where it connects.** `net::allowed`: HTTPS to `api.anthropic.com`, `api.openai.com` and `generativelanguage.googleapis.com`, no port,
  no credentials in the address, no redirect followed. The key goes in a header (`x-api-key`, `Authorization: Bearer`,
  `x-goog-api-key`), never in an address or a body.

## The providers

Each is a pure pair in its own module: a conversation to a `net::Request`, a reply to a `Reply` (its words, its tool calls, and `raw`,
the provider's own form of the reply, which goes back unchanged because a thinking block, a reasoning item or a thought signature is
only valid in the conversation that made it). A provider's error reply (`{"error": {"message": …}}`, all three) becomes a note in the chat
that says the status and the message, and what to check for a 401, 403 or 429.

- **Anthropic**: `POST /v1/messages`, tools with an `input_schema`, a `tool_result` block naming the `tool_use` id. The `anthropic-version`
  is `2026-10-21`, which refuses sampling parameters (none is sent); thinking is the model's default. `stop_reason: "refusal"` is a note
  naming its category; `max_tokens` in the middle of a call runs nothing.
- **OpenAI**: the **Responses API** (`POST /v1/responses`), not Chat Completions, because OpenAI's current models take tool calls only
  there. `store: false` (nothing is kept on OpenAI's side), output items go back as they came, a reasoning item only with its encrypted
  content (asked for with `include`), and a `function_call_output` item names the `call_id`. Tools are not strict: strict mode wants
  every property required and no free-form object, and `execute`'s `params` is one.
- **Gemini**: `POST /v1beta/models/{model}:generateContent`, tools as `functionDeclarations`, a `functionResponse` part with the call's
  `id` when it had one, the model's turn back whole (the `thoughtSignature`). Declarations take a subset of OpenAPI with no free-form
  object, so `gemini::schema` writes `params` as text that holds a JSON object and `tools::run` reads it back.

## Checking it

`cargo test -p bimcraft-assistant` (the clients against recorded replies, the tools on a session, the loop against a fake network, the
network layer against a server on this machine) and `cargo test -p bimcraft-ui-egui assistant` (the palette in both profiles, the
shortcut, the agreement, the key, the settings). No test reaches a provider: a window starts with no assistant network, and `main`
attaches `net::Ureq`.

A real call is not in the tests. With a key in the environment, run the desktop (`cargo run -p bimcraft -- --sample`), open the palette
with AI and ask for a wall; for a quick look at the wire without the window, give `bimcraft_assistant::complete` a `Net::Ureq` and one
`Msg::User` in a scratch program. What a real provider does that a recorded reply cannot show (a model id that no longer exists, a
schema one provider refuses) shows only that way, so when a provider changes, check it live.

## Not done

- A dry run (`Session::script_dry_run`) the assistant could offer before it changes anything, and reviewable edits ([discussion
  274](https://github.com/IPedrax/BIMCraft/discussions/274)): the tools are the MCP server's `execute`, not its `script`.
- Pictures the assistant makes: it cannot look at a plan or the 3D view itself, only read the model (the user may attach a picture).
- Streaming: a reply arrives whole, so a long one shows nothing until it is done (Stop gives up on it), and *Working…* does not turn: the
  reply wakes the window, so nothing asks for a frame after frame meanwhile.
- Markdown in the answers is shown as typed, and a PDF cannot be attached.
- Folders of a killed process are not swept from the temp folder at the next start.
- Server-side fallbacks for Anthropic's refusals (a beta header): a refusal is a note.
