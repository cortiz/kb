# Quickstart & Validation Guide: Shell Integration

**Feature**: Shell Integration  
**Feature Branch**: `010-shell-integration`  
**Milestone**: M8  
**Status**: Completed  

---

## 1. Prerequisites

- Built `kb` binary: `cargo build --workspace`
- A sample knowledge base in temporary directory or `~/kb`.

---

## 2. Interactive Testing Scenarios

### Scenario 1: Shell Widget Installation & Verification
1. Open zsh, bash, or fish.
2. Run `eval "$(cargo run -q -- shell init zsh)"` (or `bash` / `fish | source`).
3. Press `Alt-K`:
   - Verify compact picker opens below prompt.
   - Search for a command entry.
   - Press `Enter`: verify the command appears on the command line, cursor at the end.
   - Verify the command was **NOT** executed automatically.

### Scenario 2: Secret Protection
1. Create a fixture entry with a secret variable:
   ```markdown
   ---
   id: cmd/secret-test
   title: Secret Test
   type: cmd
   vars:
     token:
       kind: secret
       env: TEST_TOKEN
   ---
   curl -H "Authorization: Bearer {{token}}" https://api.example.com
   ```
2. Trigger the widget, select `cmd/secret-test`, press `Enter`.
3. Verify the command placed on the prompt line reads:
   `curl -H "Authorization: Bearer $TEST_TOKEN" https://api.example.com`
   and does not contain any plaintext secret value.

### Scenario 3: Automated Pseudo-Terminal Tests
Run the integration test suite:
```bash
cargo test --test shell_integration_pty
```
Verifies pseudo-terminal terminal state, clean line clearing, and cursor restoration.
