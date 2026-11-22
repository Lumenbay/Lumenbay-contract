# Deploying Lumenbay to Stellar testnet

A step-by-step runbook. Every command was used for the current testnet deployment.

## Current testnet deployment

| | |
|---|---|
| Network | Stellar testnet (`Test SDF Network ; September 2015`) |
| Escrow contract | `CDSLTYUH6YDS5WIW3QB2CRBKPP2YSKURD36ZEK7LKSZBHIGVTKUNFMYG` |
| Wasm hash (running) | `2496636ac795e16b3d1769a2ad63af2552aa05e6b0fdd7b63d35b613ed8ab184` (commit `0e5ceac`) |
| Pending upgrade | `6dc9c278d7c592f1506c6a07f5ed324bb9e19d2592a263a93ac6175ad00e9bb5` (commit `24659f3`, slim build), proposed at ledger 5856540, executable from ledger 5891100. See [Upgrading the contract](#upgrading-the-contract). |
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

The commands below are the ones used to propose the upgrade from `0e5ceac`
(hash `2496636a…`) to the slim build on `main` at `24659f3` (hash `6dc9c278…`).

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
diff every `#[contracttype]` that is written to storage, especially `Escrow`
and `DataKey`:

```bash
git diff <old-commit> <new-commit> -- contracts/marketx/src/types.rs
```

Never remove, reorder, rename or retype a stored field. To retire a feature,
keep its fields and types (they decode as empty) and remove only the functions.
That is why the slim build still has `Escrow.milestones`, `time_lock` and
`group_buy` and the `Milestone`, `TimeLock` and `GroupBuy` types. For
`0e5ceac` to `24659f3`, `types.rs` is byte-identical.

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
a fresh create, fund and release.

Rehearsal for this upgrade: contract
`CADMG3WZBCFANSNH4QC7XNVT6X7AGGM7DDAADBYIYBJFJTO3EZW4UCKC`. Every path passed
on escrows created before the upgrade.

### 4. Upload the new wasm and propose

```bash
stellar contract build                       # on the new commit; note the Wasm Hash
stellar contract upload --wasm target/wasm32v1-none/release/marketx.wasm \
  --source-account lumenbay-deployer --network testnet      # prints the hash
stellar contract invoke --id $ID --source-account lumenbay-deployer --network testnet -- \
  propose_upgrade --new_wasm_hash 6dc9c278d7c592f1506c6a07f5ed324bb9e19d2592a263a93ac6175ad00e9bb5
stellar contract invoke --id $ID --source-account lumenbay-deployer --network testnet --send=no -- \
  get_pending_upgrade
```

### 5. Execute after the timelock

```bash
stellar ledger latest --network testnet      # wait until "sequence" >= ready_at
stellar contract invoke --id $ID --source-account lumenbay-deployer --network testnet -- \
  execute_upgrade
```

### 6. Verify

```bash
stellar contract fetch --id $ID --network testnet --out-file upgraded.wasm
shasum -a 256 upgraded.wasm                  # must equal the proposed hash
stellar contract info meta --id $ID --network testnet   # name: "Lumenbay Escrow"
for i in 1 2 3 4; do
  stellar contract invoke --id $ID --source-account lumenbay-deployer --network testnet --send=no -- \
    get_escrow --escrow_id $i
done
```

Then run a fresh create, fund and release with the test identities (section 7,
with `--metadata`), and check that the frontend can still read escrows with
`contract.Client.from` plus `get_escrow`.

### Notes

- Fee volume tiers: `get_volume_tiers` defaults to thresholds of 100,000 /
  1,000,000 / 10,000,000 stroops (0.01 / 0.1 / 1 XLM) with discounts of 100 /
  250 / 500 bps, and `calculate_fee_internal` subtracts the discount from
  `fee_bps`. With `fee_bps = 100`, even tier 1 cancels the whole fee, so any
  buyer with at least 0.01 XLM of prior volume pays the 1-stroop rounding
  minimum instead of 1%. Seen in the rehearsal: a 1 XLM release paid a fee
  of 1 stroop. This predates the upgrade (fee code is unchanged), and no
  function sets the tiers, so fixing it needs a code change.
- The slim build removed arbiter staking and appeals. Before upgrading, check
  that no stored escrow has an arbiter, stake or appeal (`get_appeal`). None
  did here.
