<div align="center">
  <img width="200px" height="200px" src="https://platarium.com/assets/prevedere/assets/images/icon/plp.png" alt="Platarium logo">
</div>

# Platarium Core

[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![CI](https://img.shields.io/badge/CI-passing-brightgreen.svg)]()

High-performance cryptographic core library for Platarium Network, implemented in Rust.

## 🚀 Features

- **BIP39 Mnemonic Generation** - Generate and validate 24-word mnemonic phrases
- **BIP32 HD Wallet Support** - Hierarchical deterministic key derivation
- **ECDSA secp256k1** - Elliptic curve cryptography for signatures and identification
- **HKDF Key Derivation** - Secure key derivation using HKDF (HMAC-based Key Derivation Function)
- **Dual-Key Signature Scheme** - Sign messages with two keys for enhanced security
- **Transaction Processing** - Complete transaction execution engine with state management
- **Multi-Asset Transfers** - PLP and Token(symbol); amount in minimal units; fee always μPLP
- **State Snapshots** - O(1) immutable snapshots with copy-on-write semantics
- **Transaction Simulation** - Dry-run transactions without modifying global state
- **Dynamic Fee Calculation** - Load-based fee system with micro-PLP (μPLP) units; fee always μPLP
- **Deterministic Execution** - Guaranteed reproducibility (no randomness, no system time)
- **Consensus Building Blocks** - Node registry, dynamic validator selection (L1/L2), L1 confirmation, block assembly, slashing (Modules 1–5)
- **Zero-Cost Abstractions** - Native performance with Rust's type safety

## 📦 Installation

### Prerequisites

- Rust 1.70 or later ([Install Rust](https://www.rust-lang.org/tools/install))
- Cargo (comes with Rust)

### Build

```bash
cd PlatariumCore
cargo build --release
```

### Install CLI Tool

```bash
cargo install --path .
```

## 🖥️ CLI (Command Line Interface)

Platarium Core includes a CLI tool for interacting with the library from the command line.

### Quick Start

```bash
# Generate mnemonic
cargo run --bin platarium-cli -- generate-mnemonic

# Generate keys
cargo run --bin platarium-cli -- generate-keys \
  --mnemonic "your mnemonic phrase here" \
  --alphanumeric "YOURCODE"

# Sign message
cargo run --bin platarium-cli -- sign-message \
  --message '{"from":"Px000001","to":"Px000002","value":"100"}' \
  --mnemonic "your mnemonic" \
  --alphanumeric "YOURCODE"

# Verify signature
cargo run --bin platarium-cli -- verify-signature \
  --message '{"from":"Px000001","to":"Px000002","value":"100"}' \
  --signature "..." \
  --pubkey "..."
```

### CLI Commands

#### Generate Mnemonic

Generate a new BIP39 mnemonic phrase and alphanumeric code:

```bash
platarium-cli generate-mnemonic
```

**Output:**
```
Mnemonic: word1 word2 ... word24
Alphanumeric: ABC123XYZ789
```

#### Generate Keys

Generate cryptographic keys from a mnemonic phrase:

```bash
platarium-cli generate-keys \
  --mnemonic "word1 word2 ... word24" \
  --alphanumeric "ABC123XYZ789" \
  --seed-index 0 \
  --path "m/44'/60'/0'/0/0"  # optional
```

**Options:**
- `--mnemonic` / `-m`: BIP39 mnemonic phrase (required)
- `--alphanumeric` / `-a`: Alphanumeric code (optional, will be generated if not provided)
- `--seed-index` / `-s`: Seed index for key derivation (default: 0)
- `--path` / `-p`: Custom derivation path (optional)

#### Sign Message

Sign a JSON message with both keys (main + HKDF):

```bash
platarium-cli sign-message \
  --message '{"from":"Px000001","to":"Px000002","value":"100"}' \
  --mnemonic "word1 word2 ... word24" \
  --alphanumeric "ABC123XYZ789"
```

**Options:**
- `--message`: JSON message to sign (required)
- `--mnemonic` / `-m`: BIP39 mnemonic phrase (required)
- `--alphanumeric` / `-a`: Alphanumeric code (required)

⚠️ **Note:** `timestamp` is user-provided metadata for message signing only and is not used in transaction execution or consensus.

#### Verify Signature

Verify a message signature:

```bash
platarium-cli verify-signature \
  --message '{"from":"Px000001","to":"Px000002","value":"100"}' \
  --signature "signature_hex_string" \
  --pubkey "public_key_hex_string"
```

**Options:**
- `--message`: JSON message that was signed (required)
- `--signature` / `-s`: Signature in hex format (compact or DER) (required)
- `--pubkey` / `-p`: Public key in hex format (required)

## 🧪 Testing

Run all tests to verify functionality of all modules:

```bash
# Run all tests (integration + module + unit)
cargo test

# Run only integration tests
cargo test --test integration_test

# Run only module tests
cargo test --test module_test

# Run with output
cargo test -- --nocapture

# Or use the test script
./tests/run_all_tests.sh
```

### Test Coverage

- ✅ **13 integration tests** - End-to-end workflow tests
- ✅ **6 module tests** - Module-level integration tests
- ✅ **116 unit tests** - Comprehensive unit test coverage across all modules
  - **9 transaction tests** - Transaction structure, validation, hash, multi-asset
  - **39 state tests** - State management, snapshots, restore, asset/uplp balances
  - **11 execution tests** - Execution logic, simulation, context handling
  - **12 mempool tests** - Transaction pool management (incl. fairness / anti-starvation)
  - **24 fee calculation tests** - Fee computation, load multipliers, micro-PLP
  - **7 determinism tests** - Determinism verification across modules
  - **4 core tests** - Core engine integration
  - **Additional tests** - Mnemonic, keys, signatures, utilities, asset
- ✅ **Full workflow tests** - Complete transaction lifecycle
- ✅ **Snapshot and restore tests** - Included in state tests (25+ tests)
- ✅ **Determinism verification tests** - Cross-module determinism checks

## 📚 Usage

### As a Library

Add to your `Cargo.toml`:

```toml
[dependencies]
platarium-core = { path = "../PlatariumCore" }
```

### Generate Mnemonic

```rust
use platarium_core::*;

let (mnemonic, alphanumeric) = generate_mnemonic()?;
println!("Mnemonic: {}", mnemonic);
println!("Alphanumeric: {}", alphanumeric);
```

### Generate Keys

```rust
use platarium_core::*;

let key_gen = KeyGenerator::new(0, None, None, None)?;
let keys = key_gen.generate_keys()?;

println!("Public Key: {}", keys.public_key);
println!("Private Key: {}", keys.private_key);
println!("Signature Key: {}", keys.signature_key);
```

### Restore Keys

```rust
let restored = key_gen.restore_keys(
    &mnemonic,
    &alphanumeric,
    0,
    None,
)?;
```

### Sign Message

```rust
use platarium_core::*;

let message = serde_json::json!({
    "from": "Px000001",
    "to": "Px000002",
    "value": "100",
    "timestamp": 1234567890
});

let signature_result = sign_with_both_keys(&message, &mnemonic, &alphanumeric)?;

println!("Hash: {}", signature_result.hash);
println!("Main signature: {:?}", signature_result.signatures[0]);
println!("HKDF signature: {:?}", signature_result.signatures[1]);
```

⚠️ **Note:** `timestamp` is user-provided metadata for message signing only and is not used in transaction execution or consensus.

### Verify Signature

```rust
use platarium_core::signature::verify_signature;

let verified = verify_signature(
    &message,
    &signature_result.signatures[0].signature_compact[..128],
    &signature_result.signatures[0].pub_key,
)?;

assert!(verified);
```

## 💸 Transaction Processing

Platarium Core provides a complete transaction processing system with state management, fee calculation, and execution contexts.

### Transaction Structure

Transactions support **multi-asset** transfers. **Fee is always paid in μPLP** (minimum fee unit); the transfer asset does not affect the fee.

```rust
use platarium_core::{Transaction, Core, State, Mempool, Asset};
use std::collections::HashSet;

let tx = Transaction::new(
    "Px000001".to_string(),      // from
    "Px000002".to_string(),      // to
    Asset::PLP,                  // asset (PLP or Token("USDT"), etc.)
    100,                         // amount in minimal units of asset
    1,                           // fee_uplp: always μPLP (min 1)
    0,                           // nonce
    HashSet::new(),              // reads
    HashSet::new(),              // writes
    "sig_main".to_string(),      // main signature
    "sig_derived".to_string(),   // derived signature
)?;
```

**Transaction Fields:**
- `hash` - Deterministic transaction hash
- `from` / `to` - Sender and receiver addresses
- `asset` - Transfer asset: `Asset::PLP` or `Asset::Token(symbol)` (e.g. `"USDT"`, `"NFT:123"`)
- `amount` - Transfer amount in **minimal units of the asset** (u128)
- `fee_uplp` - Fee in **μPLP only** (u128); minimum 1 μPLP. Fee currency is fixed and not configurable.
- `nonce` - Transaction nonce (prevents replay attacks)
- `reads` / `writes` - Address sets for parallel execution support
- `sig_main` / `sig_derived` - Dual signatures for security

### Currency and Fee Rules

- **PLP** = base network currency. **μPLP** = minimum fee unit (1 μPLP = 0.000001 PLP, 1 PLP = 1_000_000 μPLP).
- **Fee**: Always **μPLP only**. Fee currency is fixed to μPLP and is not configurable. Fee = 0 is forbidden.
- **Amount**: In minimal units of `asset` (PLP ⇒ μPLP; tokens ⇒ token-specific minimal units). Asset does not affect fee.
- **Other fee currencies** (ETH, BTC, USD, gas, etc.) are **FORBIDDEN**

### Core Transaction Processing

```rust
use platarium_core::{Core, Transaction, State, Mempool, Asset};
use std::collections::HashSet;

let core = Core::new();

// Initialize sender: asset balance (e.g. PLP) and μPLP for fees
core.state().set_balance(&"Px000001".to_string(), 1000);       // PLP balance
core.state().set_uplp_balance(&"Px000001".to_string(), 10);    // μPLP for fees
core.state().set_nonce(&"Px000001".to_string(), 0);

let tx = Transaction::new(
    "Px000001".to_string(),
    "Px000002".to_string(),
    Asset::PLP,
    100,    // amount in minimal units
    1,      // fee_uplp (μPLP)
    0,
    HashSet::new(),
    HashSet::new(),
    "sig_main".to_string(),
    "sig_derived".to_string(),
)?;

let tx_hash = core.submit_transaction(tx)?;
```

### State Management

State keeps **asset balances** (per address and asset) and **μPLP balances** (for fees) separately. Fee is always paid from μPLP.

```rust
use platarium_core::{State, StateSnapshot, SnapshotableState, Asset};

let state = State::new();

// Set PLP balance and μPLP (fee) balance
state.set_balance(&"Px000001".to_string(), 1000);       // PLP (legacy: set_asset_balance(..., PLP, ...))
state.set_uplp_balance(&"Px000001".to_string(), 10);    // μPLP for fees
state.set_nonce(&"Px000001".to_string(), 0);

let snapshot = state.snapshot();
state.apply_transaction(&tx)?;
state.restore(&snapshot);

assert_eq!(state.get_balance(&"Px000001".to_string()), 1000);
// Asset balance: get_asset_balance(addr, &Asset::PLP) or get_asset_balance(addr, &Asset::Token("USDT"))
// Fee balance: get_uplp_balance(addr)
```

### Transaction Simulation

Simulate transactions without modifying global state:

```rust
use platarium_core::{ExecutionLogic, ExecutionResult, StateSnapshot};

// Create snapshot of current state
let snapshot = state.snapshot();

// Simulate transaction
let result = ExecutionLogic::simulate(&tx, &snapshot);

// Check result using helper methods
if result.is_success() {
    if let Some(final_state) = result.get_final_state() {
        // Transaction would succeed
        // final_state contains the resulting state snapshot
        let new_balance = final_state.get_balance(&"Px000001".to_string());
        println!("New balance would be: {}", new_balance);
    }
} else {
    // Transaction would fail
    if let Some(err) = result.get_error() {
        println!("Simulation failed: {}", err);
    }
}

// Original state unchanged
assert_eq!(state.get_balance(&"Px000001".to_string()), 1000);
```

### Fee Calculation

Dynamic fee calculation based on network load:

```rust
use platarium_core::{MicroPLP, BASE_TX_FEE_MICRO_PLP, calculate_fee_from_load, calculate_load_multiplier, calculate_fee_micro_plp};

// Calculate fee based on network load
let pending_tx_count = 500; // 50% load
let fee = calculate_fee_from_load(pending_tx_count); // 2 μPLP

// Fee buckets:
// 0-30% load   → 1x multiplier → 1 μPLP
// 31-60% load  → 2x multiplier → 2 μPLP
// 61-80% load  → 3x multiplier → 3 μPLP
// 81-100% load → 5x multiplier → 5 μPLP

// Type-safe fee calculation
let base_fee = MicroPLP::new(BASE_TX_FEE_MICRO_PLP); // 1 μPLP
let multiplier = calculate_load_multiplier(pending_tx_count);
let fee = calculate_fee_micro_plp(base_fee, multiplier);
```

### Execution Contexts

Support for production and simulation modes:

```rust
use platarium_core::{ExecutionContext, ExecutionLogic};

// Production mode (commits changes)
let ctx_prod = ExecutionContext::Production;
ExecutionLogic::commit(ctx_prod)?; // OK - commits allowed

// Simulation mode (forbidden to commit)
let ctx_sim = ExecutionContext::Simulation;
ExecutionLogic::commit(ctx_sim)?; // Error: CommitNotAllowedInSimulation

// Execute transaction with context
use platarium_core::{State, Transaction, Asset};
use std::collections::HashSet;
let state = State::new();
let tx = Transaction::new("from".into(), "to".into(), Asset::PLP, 100, 1, 0, HashSet::new(), HashSet::new(), "sig_main".into(), "sig_derived".into())?;

// Execute in production mode
ExecutionLogic::execute_transaction(&state, &tx, ExecutionContext::Production)?;
ExecutionLogic::commit(ExecutionContext::Production)?;

// Execute in simulation mode (changes are temporary)
ExecutionLogic::execute_transaction(&state, &tx, ExecutionContext::Simulation)?;
// Note: In simulation, changes should be rolled back or applied to temporary state
```

### Mempool Management

**Mempool Fairness & Determinism:** Transactions are ordered by `(arrival_index, tx.hash)`.
`arrival_index` is a local monotonic counter (no system time) used only in the mempool for
fairness; the execution layer receives only `Transaction`. See `core::mempool` module docs.

```rust
use platarium_core::Mempool;

let mempool = Mempool::new();

// Add transaction
mempool.add_transaction(tx.clone())?;

// Get all transactions (fair, deterministic order: arrival then hash)
let all_txs = mempool.get_all_transactions();

// Check if transaction exists
if mempool.contains(&tx.hash) {
    println!("Transaction in mempool");
}

// Get transaction count
let count = mempool.len();

// Remove single transaction after execution
mempool.remove_transaction(&tx.hash);

// Remove multiple transactions after block execution
let tx_hashes = vec![tx1.hash.clone(), tx2.hash.clone()];
mempool.remove_transactions(&tx_hashes);

// Clear all transactions
mempool.clear();
```

### Key Features

- ✅ **Deterministic Execution** - Same transactions → same state (always)
- ✅ **State Snapshots** - O(1) snapshot creation with copy-on-write
- ✅ **Transaction Simulation** - Test transactions without side effects
- ✅ **Dynamic Fees** - Load-based fee calculation (1x, 2x, 3x, 5x multipliers); fee always μPLP
- ✅ **Multi-Asset Transfers** - PLP and `Token(symbol)`; amount in minimal units; fee always μPLP
- ✅ **Atomic Operations** - All-or-nothing state updates
- ✅ **Nonce Management** - Prevents replay attacks
- ✅ **Balance Validation** - Asset balance ≥ amount, μPLP balance ≥ fee before execution

## 🏗️ Architecture

```
PlatariumCore/
├── src/
│   ├── lib.rs              # Main library module
│   ├── mnemonic.rs         # Mnemonic generation and validation
│   ├── key_generator.rs    # Key generation (BIP32 + HKDF)
│   ├── signer.rs           # Message signing
│   ├── signature.rs        # Signature verification
│   ├── utils.rs            # Utilities (HKDF, hash, verifyCorrelation)
│   ├── error.rs            # Error handling
│   ├── core/                    # Transaction processing and consensus
│   │   ├── mod.rs               # Core execution engine
│   │   ├── asset.rs             # Asset type (PLP, Token)
│   │   ├── transaction.rs       # Transaction structure and validation
│   │   ├── state.rs             # State management and snapshots
│   │   ├── mempool.rs           # Transaction pool (incl. forced inclusion)
│   │   ├── execution.rs        # Execution logic and simulation
│   │   ├── fee.rs               # Fee calculation (micro-PLP)
│   │   ├── determinism.rs       # Determinism audit and enforcement
│   │   ├── node_registry.rs     # Module 1: Node registry & rating engine
│   │   ├── validator_selection.rs # Module 2: Dynamic validator selection (L1/L2)
│   │   ├── confirmation_layer.rs  # Module 3: L1 transaction confirmation
│   │   ├── block_assembly.rs    # Module 4: Block assembly & L2 block validators
│   │   └── slashing.rs          # Module 5: Slashing & stability engine
│   └── main.rs                  # CLI entry point
├── tests/
│   ├── integration_test.rs     # Integration tests
│   ├── module_test.rs          # Module tests
│   └── run_all_tests.sh        # Test runner script
└── Cargo.toml
```

### Consensus and validation (Modules 1–7)

The core includes deterministic, integer-only consensus building blocks (no RNG or system time):

| Module | File | Purpose |
|--------|------|---------|
| **1. Node Registry & Reputation Engine** | `node_registry.rs` | **Validation Modules Step 1.** Stores `node_id`, `public_key`, `stake`, `reputation_score`, `uptime_score`, `latency_score`, `load_score`, `missed_votes`/`total_votes`. API: `register`, `unregister`, `set_scores` (batch), `set_uptime_score`, `set_latency_score`, `set_load`, `set_vote_stats`, `get_eligible`. |
| **2. Dynamic Validator Selection** | `validator_selection.rs` | **Step 2.** `compute_seed(block_number, prev_finalized_hash)`, `selection_percent_from_load(current_tps, capacity)` (10–30% L1, 10–20% L2), `select_validators(registry, seed, percent)` / `select_validators_with_percent`, `select_l1_l2_validators` → (L1 list, L2 list). |
| **3. L1 Transaction Confirmation** | `confirmation_layer.rs` | **Step 3.** Select 10–30% validators per TX; verify balance/nonce/sig/fee; `process_l1_confirmation(votes)` → (Confirmed/Rejected, to_penalize); `apply_l1_penalties`. |
| **4. Block Assembly & L2** | `block_assembly.rs` | **Step 4.** Form block from TX list (`assemble_block`); select L2 validators; L2 vote ≥70% (`process_l2_block_votes`); finalize or reject (`block_finalized(result)`). |
| **5. Forced-inclusion Mempool** | `mempool.rs` | **Step 5.** Forced-inclusion queue (up to 256): `add_forced_inclusion`, `get_forced_inclusion`, `get_transaction_hashes_for_block(max_count)` → forced first, then regular. `MAX_FORCED_INCLUSION_QUEUE`. |
| **6. Slashing & Stability** | `slashing.rs` | **Step 6.** SlashingReason (NoVote, AgainstMajority, Equivocation, InvalidTx); `apply_slash`, `apply_slash_batch`, `penalty_amounts(reason)`; SUSPENSION_THRESHOLD → status = Suspended. |
| **7. Dynamic Group-Based TX Assignment** | `tx_assignment.rs` | **Step 7.** Required total stake per TX; form verifier groups (stake ≥ required, each validator stake ≥ TX amount); load/reputation ordering; `assign_transactions_to_groups`; after verification apply slashing/penalty on errors. |
| **8. Block Leader Rotation & BFT Finality** | `block_assembly.rs` | **Step 8.** `block_leader_for_height(block_number, l2_validators)` — deterministic leader; leader proposes block; L2 HotStuff-style voting; block **final** after ≥70% votes (safety and deterministic finalization). |
| **9. Deterministic Randomness for Validator Selection** | `validator_selection.rs` | **Step 9.** `global_entropy = hash(prev_finalized_block)`; `seed = SHA256(block_number \|\| global_entropy)` (`compute_seed` / `committee_selection_seed`); deterministic L1/L2 selection so every node can verify committees. |
| **10. Integration & Invariant Testing (Determinism)** | `tests/determinism_invariants_test.rs`, `core/determinism.rs` | **Step 10.** Property-style tests: same inputs → same validator selection; same seed → same committee; same TX/state → same state_root; no float/RNG/system time in consensus path. |

Flow: **TX → Mempool → (assign TX to groups) → L1 validators → Confirmed TX pool → Block assembler → L2 validators → Block finalized.**

## 🔐 Modules

### Mnemonic

- `generate_mnemonic()` - Generate BIP39 mnemonic (24 words)
- `validate_mnemonic()` - Validate mnemonic phrase
- `generate_alphanumeric_part()` - Generate alphanumeric code

### KeyGenerator

- `generate_keys()` - Generate new keys
- `restore_keys()` - Restore keys from mnemonic
- Support for custom derivation paths

### Signer

- `sign_with_both_keys()` - Sign message with two keys (main + HKDF)

### Signature

- `sign_message()` - Sign message with single key
- `verify_signature()` - Verify signature
- `hash_message()` - Hash message with domain separator

### Utils

- `derive_signature_seed_from_master_seed()` - Derive key via HKDF
- `verify_correlation()` - Verify correlation between keys
- `bn_to_hex32()` - Convert to 64-character hex

### Dynamic Group-Based TX Assignment (Validation Modules — Step 7)

- `required_stake_for_tx(tx)` / `required_stake_for_amount(amount)` — required total stake for a group to safely verify the TX.
- `min_validator_stake_for_tx(tx)` / `min_validator_stake_for_amount(amount)` — minimum stake per validator (each must have stake ≥ TX amount).
- `form_verifier_groups(registry, required_total_stake, min_validator_stake, max_groups)` — forms groups with load/reputation ordering; each group total stake ≥ required.
- `assign_transactions_to_groups(txs, registry, max_groups)` — assigns each TX to a verifier group; returns `Vec<TxGroupAssignment>` (tx_hash, group_index, verifier_ids).
- After verification use `process_l1_confirmation` → `apply_l1_penalties` or `apply_slash_batch` for errors.
- `DEFAULT_MIN_REQUIRED_STAKE`, `DEFAULT_MIN_VALIDATOR_STAKE`, `TxAssignmentError`, `TxGroupAssignment`.

### Block Leader Rotation & BFT-style Finality (Validation Modules — Step 8)

- **Leader rotation:** `block_leader_for_height(block_number, l2_validators)` — deterministic leader for height (round-robin over L2 set). `block_leader_index_for_height(block_number, num_validators)` — leader index.
- **Flow:** Leader proposes block (`assemble_block` with `producer_id` = leader) → L2 validators vote (HotStuff-style) → `process_l2_block_votes(votes)` → block **final** when ≥70% Confirm (`L2_CONFIRM_THRESHOLD_PCT`); use `block_finalized(result)`.
- Ensures **safety and deterministic finalization** (BFT-style finality).

### Deterministic Randomness for Validator Selection (Validation Modules — Step 9)

- **Reproducible committee selection:** `global_entropy = hash(prev_finalized_block)` (use previous finalized block hash as bytes). **seed = SHA256(block_number || global_entropy)** — `compute_seed(block_number, global_entropy)` or `committee_selection_seed(block_number, global_entropy)`.
- **Deterministic selection** for L1 and L2: same inputs yield the same committees; **every node can verify** that the L1/L2 committees were selected correctly.

### Integration & Invariant Testing — Determinism (Validation Modules — Step 10)

- **Run:** `cargo test --test determinism_invariants`
- **Invariants (property-style):** Same inputs → same validator selection (L1/L2); same seed → same committee; same state / same TX → same state_root; consensus path double-run yields identical results (no float, RNG, or system time).
- **Determinism module:** `core/determinism.rs` — audit and documentation; unit tests for fee, transaction hash, state snapshot, mempool order.

### Node Registry & Reputation Engine (Validation Modules — Step 1)

- `NodeRegistry` - Thread-safe registry: **register**, **unregister**, **set scores**, **get_eligible**
  - Stored per node: `node_id`, `public_key`, `stake`, `reputation_score`, `uptime_score`, `latency_score`, `load_score`, `missed_votes`, `total_votes`
  - `register(node_id, public_key, stake, max_capacity)` - Add node
  - `unregister(node_id)` - Remove node
  - `set_scores(node_id, uptime_score, latency_score, load_option)` - Batch set scores (load = `Some((current_tasks, max_capacity))`)
  - `set_uptime_score`, `set_latency_score`, `set_load`, `set_stake`, `set_status` - Individual updates
  - `set_vote_stats(node_id, missed_votes, total_votes)` - Bulk vote stats
  - `get_eligible()` - All active nodes, sorted by `node_id` (for validator selection)
- `Node`, `NodeId`, `NodeStatus`, `NodeRegistryError`, `SCORE_SCALE`, `WEIGHT_*` - Types and constants

### Transaction Core

- `Asset` - Transfer asset type (`PLP` | `Token(symbol)`). Exported from `platarium_core`.
- `Transaction` - Canonical transaction structure
  - `new()` - Create new transaction (from, to, asset, amount, fee_uplp, nonce, reads, writes, sig_main, sig_derived)
  - `compute_hash()` - Compute deterministic transaction hash
  - `validate_basic()` - Validate transaction (amount > 0, fee_uplp ≥ 1, signatures)
  - `verify_signatures()` - Verify both signatures (main + derived)
- `Core` - Main transaction processing engine
  - `new()` - Create new Core instance
  - `submit_transaction()` - Submit and execute transaction
  - `state()` - Get state manager reference
  - `mempool()` - Get mempool reference
- `State` - Blockchain state manager with snapshots
  - `new()` - Create new empty state
  - `get_balance(addr)` - Get PLP balance (legacy; 0 if not found)
  - `get_asset_balance(addr, asset)` - Get balance for given asset
  - `get_uplp_balance(addr)` - Get μPLP balance (for fees)
  - `get_nonce(addr)` - Get address nonce (0 if not found)
  - `set_balance(addr, amt)` - Set PLP balance
  - `set_asset_balance(addr, asset, amt)` - Set balance for asset
  - `set_uplp_balance(addr, amt)` - Set μPLP balance
  - `set_nonce(addr, n)` - Set address nonce
  - `apply_transaction(tx)` - Validate and apply transaction
  - `apply_transfer(from, to, asset, amount, fee_uplp, nonce)` - Deduct fee from μPLP, amount from asset; credit amount to receiver, fee to treasury
  - `snapshot()` - Create immutable state snapshot (O(1))
  - `restore(snapshot)` - Restore state from snapshot (rollback)
- `TREASURY_ADDRESS` - Fee recipient constant (`"treasury"`).
- `Mempool` - Transaction pool for pending transactions
  - `new()` - Create new empty mempool
  - `add_transaction()` - Add transaction (prevents duplicates)
  - `get_transaction()` - Get transaction by hash
  - `remove_transaction()` - Remove single transaction
  - `remove_transactions()` - Remove multiple transactions
  - `get_all_transactions()` - Get all transactions (fair order: arrival_index, then hash; see mempool docs)
  - `len()` - Get transaction count
  - `is_empty()` - Check if mempool is empty
  - `contains()` - Check if transaction exists
  - `clear()` - Clear all transactions
- `StateSnapshot` - Immutable state snapshots (O(1) creation)
  - `get_balance()` - Get address balance from snapshot
  - `get_nonce()` - Get address nonce from snapshot
- `ExecutionContext` - Production/Simulation execution modes
  - `Production` - Production mode (commits allowed)
  - `Simulation` - Simulation mode (commits forbidden)
- `ExecutionLogic` - Shared execution logic
  - `validate_transaction()` - Validate transaction (signatures, amount, fee)
  - `check_transaction_applicability()` - Check if transaction can be applied (nonce, balance)
  - `apply_transaction_effects()` - Apply transaction effects to state
  - `execute_transaction()` - Execute transaction (combines all steps)
  - `commit()` - Commit transaction (context-dependent)
  - `simulate()` - Simulate transaction on snapshot
- `ExecutionResult` - Transaction execution results
  - `is_success()` / `is_failure()` - Check execution status
  - `get_final_state()` - Get resulting state snapshot
  - `get_error()` - Get error message if failed

### Fee Calculation

- `MicroPLP` - Type-safe micro-PLP currency type (newtype wrapper around u64)
  - `new()` - Create new MicroPLP value
  - `as_u64()` - Get underlying u64 value
  - `as_plp()` - Convert to PLP (integer part)
  - `remainder_micro_plp()` - Get remainder after PLP conversion
- Constants:
  - `MICRO_PLP_PER_PLP` - Conversion constant (1_000_000)
  - `BASE_TX_FEE_MICRO_PLP` - Base transaction fee (1 μPLP = 0.000001 PLP)
  - `MAX_BATCH_SIZE` - Maximum batch size for load calculation (1000)
  - `MULTIPLIER_1X` - Load multiplier for 0-30% load (1x)
  - `MULTIPLIER_2X` - Load multiplier for 31-60% load (2x)
  - `MULTIPLIER_3X` - Load multiplier for 61-80% load (3x)
  - `MULTIPLIER_5X` - Load multiplier for 81-100% load (5x)
- Functions:
  - `calculate_fee_from_load()` - Calculate fee based on pending transaction count
  - `calculate_fee_from_load_micro_plp()` - Type-safe version returning MicroPLP
  - `calculate_load_multiplier()` - Calculate load multiplier from pending count
  - `calculate_fee()` - Calculate fee from base fee and multiplier
  - `calculate_fee_micro_plp()` - Type-safe version returning MicroPLP
  - `fee_to_plp_string()` - Convert fee to PLP string for display

## 🔒 Security

- **Memory Safety** - Rust's ownership system prevents memory-related vulnerabilities
- **Type Safety** - Compile-time type checking prevents runtime errors
- **No Runtime Dependencies** - Single binary, no external dependencies at runtime
- **Cryptographic Best Practices** - Uses well-tested libraries (secp256k1, BIP39, BIP32)


## 📝 License

MIT License - see [LICENSE](LICENSE) file for details.

**Built with ❤️ by the Platarium team**
