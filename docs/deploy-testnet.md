# Deploying Lumenbay to Stellar testnet

A step-by-step runbook. Every command was used for the current testnet deployment.

## Current testnet deployment

| | |
|---|---|
| Network | Stellar testnet (`Test SDF Network ; September 2015`) |
| Escrow contract | `CDSLTYUH6YDS5WIW3QB2CRBKPP2YSKURD36ZEK7LKSZBHIGVTKUNFMYG` |
| Wasm hash (running) | `2496636ac795e16b3d1769a2ad63af2552aa05e6b0fdd7b63d35b613ed8ab184` (commit `0e5ceac`) |
| Pending upgrade | `942b3bee71b620b5556c1d6b7b780b41ba09a98ee0d9a300a2c7aa9208dd176e`: the slim build plus the fee fix, built from commit `41744f6` on branch `fix/fee-volume-discount`. Proposed at ledger 5856919, executable from ledger **5891479** (about 2026-11-24 00:45 UTC). **Merge that branch before executing.** See [Upgrading the contract](#upgrading-the-contract). |
| Volume tiers (stored) | Legacy 0.01 / 0.1 / 1 XLM. Correct them with `set_volume_tiers` right after the upgrade (step 6 below). |
| Admin & fee collector | `GAZ2UWDIVCFK5TSNAO4TH6L273ZAA45LZVNGCF3BU3G3YKKF5FAQH4H7` |
| Fee | 100 bps (1%), no minimum, no cap |
| Native XLM token (SAC) | `CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC` |
| RPC | `https://soroban-testnet.stellar.org` |

Testnet is reset periodically by the Stellar Development Foundation. After a reset, follow this guide again and update the table above and the frontend's environment variables.

## Prerequisites

```bash
rustup target add wasm32v1-none      # Soroban build target
cargo install --locked stellar-cli   # or: brew install stellar-cli
stellar --version
```

## 1. Test the contract

```bash
cargo test
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

## 2. Build the Wasm

```bash
stellar contract build
```

Output: `target/wasm32v1-none/release/marketx.wasm`. The printed **Wasm hash** identifies this exact build.

## 3. Create a funded deployer identity

```bash
stellar keys generate lumenbay-deployer --network testnet --fund
stellar keys address lumenbay-deployer
```

`--fund` asks Friendbot for free test XLM. The secret key is stored locally in
`~/.config/stellar/identity/lumenbay-deployer.toml`. Never commit or share it.
This identity becomes the contract admin.

## 4. Deploy

```bash
stellar contract deploy \
  --wasm target/wasm32v1-none/release/marketx.wasm \
  --source-account lumenbay-deployer \
  --network testnet \
  --alias lumenbay-escrow
```

The printed `C…` address is the contract ID. `--alias` lets later commands use
`--id lumenbay-escrow`.

## 5. Initialize immediately

`initialize` is a separate call, so anyone could call it between deploy and
initialize. Run it straight after deploying, then verify the admin.

```bash
stellar contract invoke --id lumenbay-escrow --source-account lumenbay-deployer --network testnet -- \
  initialize --admin lumenbay-deployer --fee_collector lumenbay-deployer \
  --fee_bps 100 --min_fee 0 --max_fee 0
```

`fee_bps` is basis points (100 = 1%). `max_fee 0` means no cap.

## 6. Verify

Read-only calls with `--send=no` are simulated and cost nothing.

```bash
stellar contract invoke --id lumenbay-escrow --source-account lumenbay-deployer --network testnet --send=no -- get_admin
stellar contract invoke --id lumenbay-escrow --source-account lumenbay-deployer --network testnet --send=no -- get_fee_bps
stellar contract invoke --id lumenbay-escrow --source-account lumenbay-deployer --network testnet --send=no -- is_paused
```

A second `initialize` must fail with `Error(Contract, #182)` (`AlreadyInitialized`).

## 7. Smoke-test a trade

Amounts are in stroops: 1 XLM = 10,000,000.

```bash
stellar keys generate lumenbay-test-buyer  --network testnet --fund
stellar keys generate lumenbay-test-seller --network testnet --fund
XLM=$(stellar contract id asset --asset native --network testnet)

# Buyer creates a 10 XLM escrow; prints the escrow id
stellar contract invoke --id lumenbay-escrow --source-account lumenbay-test-buyer --network testnet -- \
  create_escrow --buyer lumenbay-test-buyer --seller lumenbay-test-seller --token $XLM --amount 100000000

# Buyer locks the funds in the contract
stellar contract invoke --id lumenbay-escrow --source-account lumenbay-test-buyer --network testnet -- \
  fund_escrow --escrow_id 1

# Buyer confirms delivery; seller receives amount minus the fee
stellar contract invoke --id lumenbay-escrow --source-account lumenbay-test-buyer --network testnet -- \
  release_escrow --escrow_id 1
```

Expected on the first deployment: the seller receives 9.9 XLM, `get_pending_fee` for the
collector shows 0.1 XLM, and `get_escrow --escrow_id 1` reports `"status":"Released"`.
Funding the same escrow twice fails with `#91` (`EscrowAlreadyFunded`).

## Withdrawing fees

```bash
stellar contract invoke --id lumenbay-escrow --source-account lumenbay-deployer --network testnet --send=no -- \
  get_pending_fee --collector lumenbay-deployer --token $XLM
```

Then call `withdraw_fees` as the fee collector (see `src/fees.rs` for its arguments).

## Upgrading the contract

An upgrade swaps the contract's code but keeps its ID, balances and stored
escrows. It goes through a two-step, timelocked admin flow (`src/admin.rs`):

1. `propose_upgrade(new_wasm_hash)` records the proposal and sets
   `ready_at = now + UPGRADE_TIMELOCK_LEDGERS` (34,560 ledgers, about 48 hours).
2. `execute_upgrade()` swaps the code. It fails with `#181`
   (`UpgradeTimelockNotElapsed`) until the current ledger reaches `ready_at`.

`get_pending_upgrade` is public, so anyone can see a proposal during the
window. `cancel_upgrade` withdraws it. Both execute and cancel need the admin.

### The pending testnet upgrade

| | |
|---|---|
| From | `0e5ceac`: `2496636ac795e16b3d1769a2ad63af2552aa05e6b0fdd7b63d35b613ed8ab184` |
| To | `41744f6` (`fix/fee-volume-discount`, i.e. `main` at `24659f3` plus the fee fix): `942b3bee71b620b5556c1d6b7b780b41ba09a98ee0d9a300a2c7aa9208dd176e` |
| Proposed | ledger 5856919, [tx](https://stellar.expert/explorer/testnet/tx/f92ea8cfe22265e65b08f0cdca55bbb631f197d08206ec334790fe211880b2d0) |
| Executable from | ledger **5891479**, about 2026-11-24 00:45 UTC (01:45 Africa/Lagos), give or take an hour; testnet ran about 5.0 s per ledger when this was proposed |
| Superseded | a first proposal for the slim build without the fee fix (`6dc9c278…`, ledger 5856540) was withdrawn with `cancel_upgrade` ([tx](https://stellar.expert/explorer/testnet/tx/d0b8ec88467ce1ebb67f5a24b45f87597d0076b567faa9139baa11a1fabba5eb)), so the contract switches once, straight to the fixed build |

The proposed wasm is not on `main` until `fix/fee-volume-discount` is merged.
Merge it before executing, then confirm that `main` rebuilds to the proposed
hash (step 4). The wasm hash depends only on the source, so a squash or merge
commit does not change it.

### 1. Identify the running build

```bash
ID=CDSLTYUH6YDS5WIW3QB2CRBKPP2YSKURD36ZEK7LKSZBHIGVTKUNFMYG
stellar contract fetch --id $ID --network testnet --out-file deployed.wasm
shasum -a 256 deployed.wasm
```

Find the commit that rebuilds to exactly that hash (here `0e5ceac`), so you can
diff its source against the new build:

```bash
git worktree add --detach ../old 0e5ceac
stellar contract build --manifest-path ../old/Cargo.toml   # prints "Wasm Hash"
```

### 2. Check storage compatibility

Upgraded code must decode the escrows already stored. Before every upgrade,
diff every `#[contracttype]` that is written to storage, especially `Escrow`,
`DataKey` and `VolumeTierConfig`:

```bash
git diff <old-commit> <new-commit> -- contracts/marketx/src/types.rs
```

Never remove, reorder, rename or retype a stored field. To retire a feature,
keep its fields and types (they decode as empty) and remove only the functions.
That is why the slim build still has `Escrow.milestones`, `time_lock` and
`group_buy` and the `Milestone`, `TimeLock` and `GroupBuy` types. From `0e5ceac`
to `41744f6`, no stored type or `DataKey` changed. The fee fix adds only
constants, an event and a function, and changes the *default* tier values.

Also check that no live escrow depends on a removed feature. The slim build
dropped arbiters, appeals, milestones, time locks and group buys. None of live
escrows #1 to #5 had an arbiter, appeal, milestone, time lock or group buy.

### 3. Rehearse on a throwaway contract

Deploy the **old** code as a new contract, create escrows in each live state,
upgrade it to the **new** wasm, then exercise the old escrows. Waiting out the
real timelock (48 h) and appeal window (24 h) is impractical, so the rehearsal
copy lowers just those two constants. Storage layout is unaffected, and the
upgrade target is the unmodified new wasm.

```bash
git worktree add --detach ../old-rehearsal <old-commit>
# in ../old-rehearsal/contracts/marketx/src/types.rs set
#   UPGRADE_TIMELOCK_LEDGERS = 1 and APPEAL_WINDOW_LEDGERS = 1
stellar contract build --manifest-path ../old-rehearsal/Cargo.toml
stellar contract deploy --wasm ../old-rehearsal/target/wasm32v1-none/release/marketx.wasm \
  --source-account lumenbay-deployer --network testnet --alias lumenbay-rehearsal
stellar contract invoke --id lumenbay-rehearsal --source-account lumenbay-deployer --network testnet -- \
  initialize --admin lumenbay-deployer --fee_collector lumenbay-deployer --fee_bps 100 --min_fee 0 --max_fee 0
```

Create and fund escrows as in section 7. `metadata` is hex-encoded bytes passed
as a JSON string. The contract rejects duplicate metadata (`#70`), so vary the
nonce:

```bash
META=$(printf '%s' "lumenbay:<order-uuid>:<listing-uuid>:1" | xxd -p -c 256)
stellar contract invoke --id lumenbay-rehearsal --source-account lumenbay-test-buyer --network testnet -- \
  create_escrow --buyer lumenbay-test-buyer --seller lumenbay-test-seller --token $XLM \
  --amount 10000000 --metadata "\"$META\""
# Disputed state:
stellar contract invoke --id lumenbay-rehearsal --source-account lumenbay-test-buyer --network testnet -- \
  refund_escrow --escrow_id 3 --initiator lumenbay-test-buyer --amount 30000000 \
  --reason '"ProductNotReceived"' --evidence_hash abcd
```

Upgrade the rehearsal contract, then test every path on the old escrows:
`get_escrow`, `release_escrow`, `refund_escrow`, `resolve_dispute` (as the
admin, `--resolution 0` for seller, `1` for buyer), `claim_disputed_funds`, and
a fresh create, fund and release. Check the fee amounts in the
`fee_collected` events.

Rehearsals for this upgrade (all paths passed on escrows created before the
upgrade):

- `CADMG3WZBCFANSNH4QC7XNVT6X7AGGM7DDAADBYIYBJFJTO3EZW4UCKC`: slim build only
  (`6dc9c278…`, since withdrawn).
- `CCMTMYG2Y3ZR5MY4KENDK43DZ5MVZVSTUS53FW7NWNESPEP4SMLJCGLT`: the proposed
  fixed build (`942b3bee…`). On the old code the bug reproduced: the first
  1 XLM release paid 100,000 stroops and the second paid 1 stroop. After the
  upgrade and `set_volume_tiers`, the same buyer's 1 XLM release paid exactly
  100,000 stroops (seller 9,900,000), and a 2 XLM dispute claim paid 200,000.

### 4. Upload the new wasm and propose

```bash
git checkout main && git pull                # after merging fix/fee-volume-discount
stellar contract build                       # must print Wasm Hash 942b3bee…
stellar contract upload --wasm target/wasm32v1-none/release/marketx.wasm \
  --source-account lumenbay-deployer --network testnet      # prints the hash
stellar contract invoke --id $ID --source-account lumenbay-deployer --network testnet -- \
  propose_upgrade --new_wasm_hash 942b3bee71b620b5556c1d6b7b780b41ba09a98ee0d9a300a2c7aa9208dd176e
stellar contract invoke --id $ID --source-account lumenbay-deployer --network testnet --send=no -- \
  get_pending_upgrade
```

To replace a pending proposal with a different build, run `cancel_upgrade`
first and then propose again. Proposing again restarts the 48-hour timelock.

### 5. Execute after the timelock

```bash
stellar ledger latest --network testnet      # wait until "sequence" >= 5891479
stellar contract invoke --id $ID --source-account lumenbay-deployer --network testnet -- \
  execute_upgrade
```

### 6. Correct the stored volume tiers (straight after executing)

Instances initialized before the fee fix store tiers in the wrong units
(0.01 / 0.1 / 1 XLM). The fixed fee arithmetic already bounds them, but a
buyer with prior volume still gets 5% off the fee (0.95% instead of 1%)
until this runs. Set the intended tiers of 1,000 / 10,000 / 100,000 XLM
(1% / 2.5% / 5% off the fee):

```bash
stellar contract invoke --id $ID --source-account lumenbay-deployer --network testnet -- \
  set_volume_tiers --tier_1_threshold 10000000000 --tier_2_threshold 100000000000 \
  --tier_3_threshold 1000000000000 --tier_1_discount_bps 100 --tier_2_discount_bps 250 \
  --tier_3_discount_bps 500
stellar contract invoke --id $ID --source-account lumenbay-deployer --network testnet --send=no -- \
  get_volume_tiers
```

To turn volume discounts off entirely, pass `0` for all three discounts.

### 7. Verify

```bash
stellar contract fetch --id $ID --network testnet --out-file upgraded.wasm
shasum -a 256 upgraded.wasm                  # must equal 942b3bee…
stellar contract info meta --id $ID --network testnet   # name: "Lumenbay Escrow"
for i in 1 2 3 4 5; do
  stellar contract invoke --id $ID --source-account lumenbay-deployer --network testnet --send=no -- \
    get_escrow --escrow_id $i
done
```

Then run a fresh 1 XLM create, fund and release with the test identities
(section 7, with `--metadata`). The `fee_collected` event must show
`fee: "100000"` and the seller must receive `9900000`. Finally, check that
the frontend can still read escrows with `contract.Client.from` plus
`get_escrow`.

### How the fee bug worked (fixed in `41744f6`)

The default tiers were written as 100,000 / 1,000,000 / 10,000,000 in what
looked like whole tokens, but amounts are stroops, so the thresholds were
0.01 / 0.1 / 1 XLM. The tier discount (100 / 250 / 500 bps) was also
*subtracted from* `fee_bps`. With the deployed 1% fee, even tier 1 cut the fee
to 0%, which the rounding floor turned into 1 stroop, for any buyer with at
least 0.01 XLM of prior volume. The fix applies the discount as a share *of
the fee* (capped at half the fee), moves the defaults to real volumes, and
adds the admin-only `set_volume_tiers` used in step 6.
