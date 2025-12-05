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
  --message '{"from":"Px1234","to":"Px5678","value":"100"}' \
  --mnemonic "your mnemonic" \
  --alphanumeric "YOURCODE"

# Verify signature
cargo run --bin platarium-cli -- verify-signature \
  --message '{"from":"Px1234","to":"Px5678","value":"100"}' \
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
  --message '{"from":"Px1234","to":"Px5678","value":"100"}' \
  --mnemonic "word1 word2 ... word24" \
  --alphanumeric "ABC123XYZ789"
```

**Options:**
- `--message`: JSON message to sign (required)
- `--mnemonic` / `-m`: BIP39 mnemonic phrase (required)
- `--alphanumeric` / `-a`: Alphanumeric code (required)

#### Verify Signature

Verify a message signature:

```bash
platarium-cli verify-signature \
  --message '{"from":"Px1234","to":"Px5678","value":"100"}' \
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

- ✅ 13 integration tests
- ✅ 6 module tests
- ✅ Unit tests for each module
- ✅ Full workflow tests

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
    "from": "Px1234",
    "to": "Px5678",
    "value": "100",
    "timestamp": 1234567890
});

let signature_result = sign_with_both_keys(&message, &mnemonic, &alphanumeric)?;

println!("Hash: {}", signature_result.hash);
println!("Main signature: {:?}", signature_result.signatures[0]);
println!("HKDF signature: {:?}", signature_result.signatures[1]);
```

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
│   └── main.rs             # CLI entry point
├── tests/
│   ├── integration_test.rs # Integration tests
│   ├── module_test.rs      # Module tests
│   └── run_all_tests.sh    # Test runner script
└── Cargo.toml
```

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

## 🔒 Security

- **Memory Safety** - Rust's ownership system prevents memory-related vulnerabilities
- **Type Safety** - Compile-time type checking prevents runtime errors
- **No Runtime Dependencies** - Single binary, no external dependencies at runtime
- **Cryptographic Best Practices** - Uses well-tested libraries (secp256k1, BIP39, BIP32)


## 📝 License

MIT License - see [LICENSE](LICENSE) file for details.

## 👥 Authors

**Built with ❤️ by the Platarium Team**
