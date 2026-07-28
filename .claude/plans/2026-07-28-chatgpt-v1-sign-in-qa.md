# Manual QA Testing Plan: ChatGPT Sign-In (V1)

**Source plan:** `.claude/plans/2026-07-28-chatgpt-v1-sign-in.md`
**Generated:** 2026-07-28

## Overview

This plan adds `--login`, `--login-status`, and `--logout` to the `llm-stream` CLI: a browser-based OAuth 2.0 + PKCE sign-in against a ChatGPT subscription account, credential storage at `<config_dir>/auth.json` (mode `0600`), and status/sign-out reporting. This QA plan validates the CLI-observable behavior of all three flags, the credential store's error handling and permissions, and that the existing CLI surface is unaffected.

The real browser-based sign-in against `auth.openai.com` cannot be scripted — it needs a human, a browser, and a real ChatGPT account — so that portion is deferred to the user (Scenario 5).

## Prerequisites

- Built binary: `cargo build --bin llm-stream` (or run everything through `cargo run --bin llm-stream --`)
- An isolated config directory so QA never touches your real `~/.config/llm-stream`. Every command below passes `--config-dir <dir>` for this reason.
- To set up a clean state, pick a fresh directory, e.g.:
  ```bash
  mkdir -p /tmp/llm-stream-qa
  ```

## How to Run

- **Solo:** Execute each step manually. Compare output to the "Expected output" block. Mark pass/fail.
- **With an agent:** Ask the agent to "walk me through the QA plan one step at a time." The agent should show each command, wait for you to run it (or run it on your behalf with your approval), display the output, compare it to the expected output, and only advance after you confirm.

## Scenarios

### Scenario 1: Signed-out status and idempotent logout

**Purpose:** Validate the signed-out messaging and that `--logout` never errors, even with nothing stored.

**Steps:**

1. Run:
   ```bash
   cargo run --bin llm-stream -- --config-dir /tmp/llm-stream-qa --login-status
   ```
   **Expected output:**
   ```
   not signed in — run: llm-stream --login
   ```

2. Run:
   ```bash
   cargo run --bin llm-stream -- --config-dir /tmp/llm-stream-qa --logout
   ```
   **Expected output:**
   ```
   signed out
   ```

**Pass criteria:** Both commands exit 0 and print exactly the lines above.
**Common failure modes:** A stack trace or `Error:` line instead of a clean message; a non-zero exit code for a no-op logout.

### Scenario 2: Status reporting with stored credentials

**Purpose:** Validate that `--login-status` reads a stored credential file and reports identity/expiry without ever printing token material.

**Steps:**

1. Seed a synthetic credential file (stands in for a real `--login` — see Scenario 5 for the real flow):
   ```bash
   python3 - <<'EOF'
   import base64, json
   def b64url(d):
       return base64.urlsafe_b64encode(d).rstrip(b'=').decode()
   access = {"exp": 4102444800}
   id_ = {"email": "qa@example.com",
          "https://api.openai.com/auth": {"chatgpt_plan_type": "plus", "chatgpt_account_id": "acct_qa"}}
   auth = {
       "access_token": f"h.{b64url(json.dumps(access).encode())}.s",
       "refresh_token": "refresh-qa",
       "id_token": f"h.{b64url(json.dumps(id_).encode())}.s",
       "account_id": "acct_qa",
   }
   with open("/tmp/llm-stream-qa/auth.json", "w") as f:
       json.dump(auth, f, indent=2)
   EOF
   chmod 600 /tmp/llm-stream-qa/auth.json
   ```

2. Run:
   ```bash
   cargo run --bin llm-stream -- --config-dir /tmp/llm-stream-qa --login-status
   ```
   **Expected output:**
   ```
   signed in as qa@example.com (plus) — access token valid for <N>s
   ```
   where `<N>` is a large positive integer (the seeded token expires in the year 2100) and the line contains no substring of `access_token`, `refresh_token`, `id_token`, or the raw JWT strings.

**Pass criteria:** Output matches the pattern above; no token material appears anywhere in stdout.
**Common failure modes:** Printing the raw `TokenSet`/JWT instead of the decoded email and plan; wrong or missing expiry countdown.

### Scenario 3: Logout removes credentials

**Purpose:** Validate that `--logout` deletes the credential file and that status reflects it immediately after.

**Steps:**

1. With the file from Scenario 2 still in place (or re-seed it), loosen its permissions to confirm logout doesn't care about mode:
   ```bash
   chmod 644 /tmp/llm-stream-qa/auth.json
   cargo run --bin llm-stream -- --config-dir /tmp/llm-stream-qa --logout
   ```
   **Expected output:**
   ```
   signed out
   ```

2. Confirm the file is gone:
   ```bash
   ls /tmp/llm-stream-qa/auth.json
   ```
   **Expected output:** `ls: /tmp/llm-stream-qa/auth.json: No such file or directory` (exit 1)

3. Run logout again (idempotency) then status:
   ```bash
   cargo run --bin llm-stream -- --config-dir /tmp/llm-stream-qa --logout
   cargo run --bin llm-stream -- --config-dir /tmp/llm-stream-qa --login-status
   ```
   **Expected output:**
   ```
   signed out
   not signed in — run: llm-stream --login
   ```

**Pass criteria:** File is removed after the first logout; the second logout still prints `signed out` with no error; status afterward shows signed-out.
**Common failure modes:** Second logout errors (`NotFound` not treated as success); file left behind.

## Edge Cases

### Edge case: corrupt credential file is a loud error, not a silent sign-out

**Setup:**
```bash
echo "not json" > /tmp/llm-stream-qa/auth.json
```

**Command:**
```bash
cargo run --bin llm-stream -- --config-dir /tmp/llm-stream-qa --login-status
```

**Expected behavior:** The command exits non-zero and prints a JSON parse error (`Error: json error` / `caused by: ... expected ident ...`) — it must **not** silently report "not signed in", since that would send the operator into a confusing re-login loop for what is actually a corrupted file.

**Cleanup:** `rm -f /tmp/llm-stream-qa/auth.json`

### Edge case: pre-existing CLI flags are unaffected

**Command:**
```bash
cargo run --bin llm-stream -- --config-dir /tmp/llm-stream-qa --dir
cargo run --bin llm-stream -- --config-dir /tmp/llm-stream-qa --presets
```

**Expected behavior:** `--dir` prints the config dir path; `--presets` runs and exits 0 with no error — both behave exactly as before this plan, since the new flags are dispatched before any other argument is touched but do not change any other code path.

## Deferred: Real browser sign-in (cannot be scripted)

### Scenario 5: Live sign-in against `auth.openai.com`

**Purpose:** Confirm the OAuth flow actually works end-to-end against OpenAI's real authorization server — this is the one thing no mock or synthetic file can validate, since it depends on OpenAI accepting this app's `client_id`, redirect URI, and scopes.

**Steps (run these yourself, with a real ChatGPT account and a browser):**

1. Run the quality pipeline once more before signing in for real, to catch anything environment-specific:
   ```bash
   cargo build && cargo test -p llm-stream
   ```

2. Sign in for real, against your **actual** config dir (no `--config-dir` override, so the credentials land where the rest of the CLI will look for them later):
   ```bash
   cargo run --bin llm-stream -- --login
   ```
   **What to look for, in order:**
   - The authorize URL printed to stderr
   - A browser window opening at `auth.openai.com`
   - The ChatGPT sign-in and consent screen
   - A redirect to a page reading **"Signed in — You can close this tab and return to your terminal."**
   - On stdout: `signed in as you@example.com (plus)` (with your real email/plan)

3. Confirm file permissions:
   ```bash
   ls -l ~/.config/llm-stream/auth.json
   ```
   **Expected:** `-rw-------`. Anything else means Task 8's permission handling has a real-world bug — worth reporting back.

4. Confirm no token material reaches the terminal:
   ```bash
   cargo run --bin llm-stream -- --login-status
   ```
   **Expected:** `signed in as you@example.com (plus) — access token valid for <N>s`, and nothing else.

5. Confirm sign-out:
   ```bash
   cargo run --bin llm-stream -- --logout
   cargo run --bin llm-stream -- --login-status
   cargo run --bin llm-stream -- --logout
   ```
   **Expected:** `signed out`, then `not signed in — run: llm-stream --login`, then `signed out` again with no error.

6. Note which port the loopback listener actually used (watch for a bind error, or check `redirect_uri` in the printed authorize URL from step 2 — it will read `http://localhost:1455/...` or a different port). This settles the open question in the design doc about whether OpenAI pins the redirect URI to port 1455:
   - If sign-in succeeded on port 1455: record that 1455 works and the ephemeral fallback in `auth/listener.rs` is untested but harmless.
   - If it succeeded on a different (ephemeral) port: record that OpenAI does **not** pin the redirect port.
   - If it failed with an `invalid redirect_uri` error: the port **is** pinned — `auth/listener.rs`'s `.or_else(...)` fallback needs to be removed, and this should go back through the plan process as a fix.

7. Sign back in (`cargo run --bin llm-stream -- --login`) so later slices of this feature have real credentials to build against.

**Pass criteria:** Every step above matches its expected output/behavior.
**Common failure modes:** `invalid redirect_uri` from the server (port pinning — see step 6); browser never opens (expected on headless/SSH — the printed URL is the fallback, paste it into any browser); a 403/consent error from OpenAI (account/app-registration issue, not a bug in this code).

## Rollback / Cleanup

```bash
rm -f /tmp/llm-stream-qa/auth.json
rmdir /tmp/llm-stream-qa
```

This never touches `~/.config/llm-stream` — every scripted scenario above used `--config-dir` to stay isolated. Only the deferred Scenario 5 touches your real config dir, by design (its whole point is to leave you signed in for later work).

## QA Run Results — 2026-07-28

| Scenario | Result | Notes |
| -------- | ------ | ----- |
| 1: Signed-out status and idempotent logout | PASS | Exact output match for both commands. |
| 2: Status reporting with stored credentials | PASS | `signed in as qa@example.com (plus) — access token valid for 2317191686s`; no token material printed. |
| 3: Logout removes credentials | PASS | File removed after first logout; second logout still prints `signed out`; status afterward shows signed-out. |
| Edge case: corrupt credential file | PASS | Exits non-zero with a JSON parse error, not a silent "not signed in". |
| Edge case: pre-existing flags unaffected | PASS | `--dir` and `--presets` behave as before. |
| 5: Live sign-in against `auth.openai.com` | DEFERRED | Needs a real ChatGPT account and a real browser — cannot be run in this session. See "What to look for" in Scenario 5 above, including settling the port-1455-pinning open question and updating the design doc's OAuth section accordingly. |

**Known pre-existing issue (not introduced by this plan):** `prelude::tests::test_preset_system_over_config_system` fails on `main` before this plan's changes (verified via `git stash`) and still fails after — unrelated to ChatGPT sign-in, out of this slice's scope.
