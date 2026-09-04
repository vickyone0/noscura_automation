# noscura_e2e

Rust/[playwright-rs](https://crates.io/crates/playwright-rs) end-to-end smoke tests for the Noscura admin app, covering login, new patient registration, appointment booking, lab booking, pharmacy booking, inpatient admission, and consultation flows.

## Setup

```bash
cd rust
cargo build
```

Playwright's browser binaries must be installed once (playwright-rs shells out to the Playwright driver):

```bash
npx -y playwright install --with-deps chromium
```

## Running tests

All tests are gated behind `RUN_NOSCURA_LOGIN_SMOKE=1` plus valid credentials, and must run single-threaded since they drive a real browser against a shared admin-test environment:

```bash
RUN_NOSCURA_LOGIN_SMOKE=1 LOGIN_EMAIL=<email> LOGIN_PASSWORD=<password> \
  cargo test -- --test-threads=1
```

Run a single suite the same way, e.g.:

```bash
RUN_NOSCURA_LOGIN_SMOKE=1 LOGIN_EMAIL=<email> LOGIN_PASSWORD=<password> \
  cargo test --test consultation -- --test-threads=1
```

Without `RUN_NOSCURA_LOGIN_SMOKE=1` (and credentials), tests report as skipped rather than failing.

### Environment variables

| Variable | Purpose | Default |
|---|---|---|
| `RUN_NOSCURA_LOGIN_SMOKE` | Set to `1` to enable the smoke tests | unset (skipped) |
| `LOGIN_EMAIL` | Admin login email | none |
| `LOGIN_PASSWORD` | Admin login password | none |
| `LOGIN_URL` | Login page URL | `https://admin-test.noscura.net/login` |
| `NEW_PATIENT_URL` | New patient form URL | `https://admin-test.noscura.net/newHomeAdmin?selectedOption=1` |
| `HEADED` | Set to `1` to run with a visible browser window instead of headless | unset (headless) |
| `SLOW_MO_MS` | Delay (ms) padding each action; only applied when set or when `HEADED=1` (defaults to `150`) | `0` |

## Layout

- `src/support/` — shared page-object-style helpers (auth, flutter widget helpers, config, and one module per flow: new patient, appointments, lab, pharmacy, inpatient, consultation).
- `tests/` — one integration test file per flow, each running against the shared admin-test environment.

## Notes

- Tests share a fixed patient named **Brad Pitt** (`new_patient::EXISTING_PATIENT_NAME`) for flows that need an existing patient. The inpatient admission test requires that patient to be discharged before each rerun, since admission fails on a patient that's already admitted.
- On the Final Bill form, **Print Receipt** must be left as **No** — setting it to **Yes** triggers a 403 on PDF upload in the test environment.
