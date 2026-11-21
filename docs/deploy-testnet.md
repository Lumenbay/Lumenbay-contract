# Deploying Lumenbay to Stellar testnet

A step-by-step runbook. Every command was used for the current testnet deployment.

## Current testnet deployment

| | |
|---|---|
| Network | Stellar testnet (`Test SDF Network ; September 2015`) |
| Escrow contract | `CDSLTYUH6YDS5WIW3QB2CRBKPP2YSKURD36ZEK7LKSZBHIGVTKUNFMYG` |
| Wasm hash | `2496636ac795e16b3d1769a2ad63af2552aa05e6b0fdd7b63d35b613ed8ab184` |
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
