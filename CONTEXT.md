# llm-stream

A Rust library and CLI for streaming interactions with LLM providers (OpenAI, Anthropic, Google, Mistral, Ollama, Groq, Jina, DeepSeek), plus a ChatGPT-subscription sign-in flow for authenticating as a ChatGPT account rather than an API key.

## Language

**ChatGPT sign-in**:
The OAuth 2.0 + PKCE browser flow that authenticates the CLI against a ChatGPT subscription account (as opposed to an OpenAI API key).
_Avoid_: login, OAuth flow (when the subscription-specific meaning is intended)

**Credentials**:
The persisted `TokenSet` (access token, refresh token, ID token, account id) resulting from a ChatGPT sign-in, stored at `<config_dir>/auth.json`.
_Avoid_: tokens, auth file (be specific: say "credentials" for the concept, "`auth.json`" for the file)

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
