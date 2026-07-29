# llm-stream

A Rust library and CLI for streaming interactions with LLM providers (OpenAI, Anthropic, Google, Mistral, Ollama, Groq, Jina, DeepSeek, and a ChatGPT subscription account), including a ChatGPT sign-in flow for authenticating with a subscription instead of a metered API key.

## Language

**ChatGPT sign-in**:
The OAuth 2.0 + PKCE browser flow that authenticates the CLI against a ChatGPT subscription account (as opposed to an OpenAI API key).
_Avoid_: login, OAuth flow (when the subscription-specific meaning is intended)

**Credentials**:
The persisted `TokenSet` (access token, refresh token, ID token, account id) resulting from a ChatGPT sign-in, stored at `<config_dir>/auth.json`.
_Avoid_: tokens, auth file (be specific: say "credentials" for the concept, "`auth.json`" for the file)

**ChatGPT provider**:
The `--api chatgpt` provider (aliases `chat-gpt`, `codex`) that streams model answers using ChatGPT sign-in credentials, over OpenAI's private Responses API, rather than a metered API key.
_Avoid_: ChatGPT API (there is no public "ChatGPT API"; this is the undocumented Responses API used by Codex)

## Behavior

### Requirement: ChatGPT sign-in
Running `llm-stream --login` authenticates the operator against a ChatGPT subscription account via a browser-based OAuth 2.0 + PKCE flow, and persists the resulting credentials for later use.

#### Scenario: Successful sign-in
- **WHEN** the operator runs `llm-stream --login`
- **THEN** the CLI opens (or prints, for headless/SSH sessions) an `auth.openai.com` authorization URL
- **AND** it waits on a local loopback listener for the browser to complete the flow
- **AND** on success it exchanges the authorization code for credentials and stores them at `<config_dir>/auth.json`
- **AND** it prints `signed in as {email} ({plan type})`

#### Scenario: Forged or mismatched callback state
- **WHEN** the loopback callback's `state` parameter is missing or does not match the value generated at the start of `--login`
- **THEN** the sign-in fails and no credentials are stored

#### Scenario: The preferred loopback port is occupied
- **WHEN** the operator runs `llm-stream --login` while another process holds port 1455
- **THEN** the CLI prints a warning to stderr naming port 1455, the port it fell back to, and what to close
- **AND** the sign-in continues on the fallback port

### Requirement: Credential storage permissions
Stored ChatGPT credentials are only ever readable and writable by the file's owner.

#### Scenario: Fresh credential file
- **WHEN** `--login` writes `<config_dir>/auth.json` for the first time
- **THEN** the file is created with mode `0600`

#### Scenario: Overwriting a looser pre-existing file
- **WHEN** `--login` or a token refresh overwrites an `auth.json` that previously had broader permissions
- **THEN** the file's permissions are tightened back to `0600`

### Requirement: Sign-in status
Running `llm-stream --login-status` reports whether ChatGPT credentials are stored, without refreshing them.

#### Scenario: Credentials present
- **WHEN** the operator runs `llm-stream --login-status` and credentials are stored
- **THEN** the CLI prints `signed in as {email} ({plan type}) — access token valid for {N}s`, where `{N}` is the remaining time until the access token's `exp` claim (zero if already expired)

#### Scenario: No credentials stored
- **WHEN** the operator runs `llm-stream --login-status` and no credentials are stored
- **THEN** the CLI prints `not signed in — run: llm-stream --login`

### Requirement: Sign-out
Running `llm-stream --logout` removes any stored ChatGPT credentials.

#### Scenario: Credentials present
- **WHEN** the operator runs `llm-stream --logout` and credentials are stored
- **THEN** the credential file is deleted and the CLI prints `signed out`

#### Scenario: Already signed out
- **WHEN** the operator runs `llm-stream --logout` and no credentials are stored
- **THEN** the CLI still prints `signed out` and does not error

### Requirement: ChatGPT provider streaming
Running `llm-stream --api chatgpt` sends the prompt, and any conversation history, to a ChatGPT subscription account's model and streams the answer to stdout, using the credentials from ChatGPT sign-in.

#### Scenario: Single-turn prompt
- **WHEN** the operator runs `llm-stream --api chatgpt "<prompt>"` while signed in
- **THEN** the CLI streams the model's answer to stdout

#### Scenario: Continuing a conversation
- **WHEN** the operator runs `llm-stream --api chatgpt --from-last "<prompt>"` after an earlier `--api chatgpt` turn in the same conversation
- **THEN** the model's answer reflects information from that earlier turn

#### Scenario: Not signed in
- **WHEN** the operator runs `llm-stream --api chatgpt "<prompt>"` without stored ChatGPT credentials
- **THEN** the CLI errors with `not signed in — run: llm-stream --login` and streams nothing

#### Scenario: The server fails partway through the stream
- **WHEN** the server accepts the request but then reports a failure mid-stream (for example, `server_is_overloaded`), which arrives inside an otherwise successful HTTP 200 response
- **THEN** the CLI errors with the server's own sentence (for example, `Our servers are currently overloaded. Please try again later.`) and exits non-zero
- **AND** it never reports success with an empty answer

#### Scenario: Rejected model
- **WHEN** the operator runs `llm-stream --api chatgpt --model <model> "<prompt>"` and the server refuses the request (for example, a model the account's plan cannot use)
- **THEN** the CLI errors with the server's own message (for example, `The '<model>' model is not supported when using Codex with a ChatGPT account.`) and streams nothing

#### Scenario: Inert flags
- **WHEN** the operator runs `llm-stream --api chatgpt` with `--temperature`, `--top-p`, `--top-k`, `--api-key`, or `--api-env`
- **THEN** the CLI prints a warning to stderr that the flag is ignored by this provider
- **AND** the answer still streams to stdout as normal

#### Scenario: Reasoning effort
- **WHEN** the operator runs `llm-stream --api chatgpt --reasoning-effort <low|medium|high|xhigh> "<prompt>"`
- **THEN** the request asks the model for that level of effort
- **AND** the answer streams to stdout as normal, with no reasoning summary

#### Scenario: Reasoning summary
- **WHEN** the operator runs `llm-stream --api chatgpt --reasoning-summary "<prompt>"`
- **THEN** the model's reasoning summary, if it produces one, streams to stderr
- **AND** a `---` rule is printed to stderr between the summary and the answer
- **AND** the answer streams to stdout

#### Scenario: Reasoning summary the model declines to produce
- **WHEN** the operator runs `llm-stream --api chatgpt --reasoning-summary "<prompt>"` and the model produces no summary
- **THEN** no `---` rule is printed
- **AND** the answer streams to stdout unchanged

#### Scenario: Unrecognised reasoning effort in the config file
- **WHEN** the config file or the selected preset sets `reasoning_effort` to a value that is not `low`, `medium`, `high`, or `xhigh`
- **THEN** the CLI errors with `unknown reasoning effort "<value>"; expected one of: low, medium, high, xhigh` and streams nothing

### Requirement: Reasoning effort defaults
The config file's top-level `reasoning_effort`, and a preset's `reasoning_effort`, supply the value when `--reasoning-effort` is not given.

#### Scenario: Config file default
- **WHEN** the operator runs `llm-stream --api chatgpt "<prompt>"` without `--reasoning-effort`, and the config file sets `reasoning_effort`
- **THEN** the configured value is used, regardless of which provider the config file's own `api` names

#### Scenario: Explicit flag wins
- **WHEN** the operator passes `--reasoning-effort` and the config file or preset also sets one
- **THEN** the flag's value is used

### Requirement: Reasoning stream separator
Any provider whose stream can carry both a reasoning summary and answer text (`chatgpt --reasoning-summary`, and `--api deepseek`, which always streams reasoning) prints the `---` rule to stderr, and only when reasoning text actually arrived — never to stdout, and never when the stream carried no reasoning.

#### Scenario: DeepSeek's answer stream has no stray separator
- **WHEN** the operator runs `llm-stream --api deepseek "<prompt>" | cat`
- **THEN** stdout contains only the answer, with no leading `---`

### Requirement: Model discovery
Running `llm-stream --models` asks the server which models the signed-in ChatGPT account may use, one at a time, and reports the answer for every candidate slug. It is a query: it succeeds whether the account can use every candidate or none of them.

#### Scenario: Probing while signed in
- **WHEN** the operator runs `llm-stream --models` while signed in
- **THEN** the CLI prints a warning to stderr naming how many models it will probe and that each accepted probe spends subscription quota, before opening any connection
- **AND** it prints a table to stdout with one row per candidate slug
- **AND** a slug the account may use reads `OK`
- **AND** a slug the server refuses reads the server's own sentence

#### Scenario: Not signed in
- **WHEN** the operator runs `llm-stream --models` without stored ChatGPT credentials
- **THEN** the CLI errors with `not signed in — run: llm-stream --login`
- **AND** no quota warning is printed and no model is probed

#### Scenario: Redirecting the table
- **WHEN** the operator runs `llm-stream --models > models.txt`
- **THEN** `models.txt` contains only the table, and the quota warning appears on the terminal

#### Scenario: The candidate list is not an allowlist
- **WHEN** the operator runs `llm-stream --api chatgpt --model <slug> "<prompt>"` with a slug that `--models` does not list
- **THEN** the request is still sent and the server decides, exactly as if the slug had been listed

### Requirement: Provider-scoped config defaults
The config file's top-level `base_url`, `env`, `key`, `version`, and `model` defaults describe one specific provider and are only inherited when the config file's own `api` matches the provider selected via `--api`, or no `--api` flag was given.

#### Scenario: Config file describes a different provider
- **WHEN** the operator runs `llm-stream --api <provider> "<prompt>"` and the config file's top-level `api` names a different provider
- **THEN** `base_url`, `env`, `key`, `version`, and `model` are not inherited from the config file
- **AND** the provider falls back to its own built-in default endpoint and model

#### Scenario: Config file matches the selected provider
- **WHEN** the operator runs `llm-stream --api <provider> "<prompt>"` and the config file's top-level `api` matches `<provider>`, or omits `--api` entirely
- **THEN** `base_url`, `env`, `key`, `version`, and `model` are inherited from the config file as before

### Requirement: Version reporting
Running `llm-stream --version` identifies the binary and the version actually installed.

#### Scenario: Reporting the installed version
- **WHEN** the operator runs `llm-stream --version`
- **THEN** the CLI prints `llm-stream <version>`, where `<version>` is the version of the installed `llm-stream` package

### Requirement: Conversation metadata survives continuation
A conversation's `title`, `description`, and `parent` belong to the conversation, not to the invocation that set them. Continuing a conversation rewrites its cache file wholesale, so every one of them is carried over from the cache unless the command line supplies its own.

#### Scenario: Continuing a titled conversation
- **WHEN** the operator runs `llm-stream --from <id> "<prompt>"` on a conversation that has a title or a description, without passing `--title` or `--description`
- **THEN** the rewritten cache file still carries them
- **AND** `llm-stream --list` still shows them

#### Scenario: Renaming a conversation
- **WHEN** the operator runs `llm-stream --from <id> --title "<new title>" "<prompt>"`
- **THEN** the new title replaces the cached one

#### Scenario: Continuing a fork
- **WHEN** the operator runs `llm-stream --from <id> "<prompt>"` on a conversation that was created with `--fork`
- **THEN** the rewritten cache file still names the conversation it forked off, so `llm-stream --list` keeps showing its lineage
