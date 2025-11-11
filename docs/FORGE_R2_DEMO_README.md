# Forge R2 Demo - Complete Test

This example demonstrates the complete Forge version control system with Cloudflare R2 storage.

## 🎯 What This Demo Does

1. **Creates a Forge Repository** (not Git!) in `examples/forge-demo/`
2. **Processes 5 demo files** and converts them to binary blobs
3. **Uploads all blobs to Cloudflare R2** using your credentials
4. **Verifies uploads** by downloading and comparing hashes
5. **Tests traffic branches** (Red/Yellow/Green)
6. **Detects LSP support** (dx code editor extension)
7. **Generates commit log** showing version history

## 📁 Demo Repository Structure

```
examples/forge-demo/
├── .forge/              # Forge metadata (NOT .git!)
│   └── config.toml      # Repository configuration
├── src/
│   ├── main.rs          # Demo application entry point
│   └── lib.rs           # Library code with tests
├── Cargo.toml           # Rust project manifest
├── README.md            # Repository documentation
└── .gitignore           # Excludes forge-demo from main git
```

## 🚀 Quick Start

### Prerequisites

1. Cloudflare account with R2 enabled
2. R2 bucket created (e.g., `forge`)
3. R2 API token with "Admin Read & Write" permissions
4. Credentials in `.env` file (root of forge project)

### Run the Demo

**Option 1: Using the shell script (recommended)**

```bash
# From the forge project root
cd /f/Code/forge
bash examples/run_r2_demo.sh
```

**Option 2: Manual execution**

```bash
# Copy .env.example to .env (if not done already)
cp .env.example .env

# Edit .env with your R2 credentials
# (Already done in your case)

# Run the demo
cargo run --example r2_demo
```

## 📊 Expected Output

```
╔══════════════════════════════════════════════════════════════╗
║  Forge R2 Demo - Complete Version Control System Test       ║
╚══════════════════════════════════════════════════════════════╝

📋 Step 1: Loading R2 Configuration...
   ✓ Account ID: dea502ae...
   ✓ Bucket: forge
   ✓ Endpoint: https://dea502ae...r2.cloudflarestorage.com
   ✓ R2 Storage client initialized

🔍 Step 2: Testing LSP Detection...
   ✓ No LSP detected, using file watching fallback

🚦 Step 3: Initializing Traffic Branch System...
   ✓ Traffic branch manager initialized
   ✓ Available branches: Red, Yellow, Green

📁 Step 4: Processing Forge Demo Repository Files...
   ✓ Processed: README.md (1234 bytes)
   ✓ Processed: Cargo.toml (156 bytes)
   ✓ Processed: src/main.rs (432 bytes)
   ✓ Processed: src/lib.rs (378 bytes)
   ✓ Processed: .forge/config.toml (289 bytes)

   📊 Statistics:
      Total files: 5
      Original size: 2489 bytes
      Compressed size: 1245 bytes
      Compression ratio: 50.0%

☁️  Step 5: Uploading Blobs to Cloudflare R2...
   Endpoint: https://dea502ae...r2.cloudflarestorage.com
   Bucket: forge

   📤 Uploading README.md (a1b2c3d4...)... ✓
   📤 Uploading Cargo.toml (e5f6g7h8...)... ✓
   📤 Uploading src/main.rs (i9j0k1l2...)... ✓
   📤 Uploading src/lib.rs (m3n4o5p6...)... ✓
   📤 Uploading .forge/config.toml (q7r8s9t0...)... ✓

   📊 Upload Statistics:
      Successfully uploaded: 5/5
      Total bytes uploaded: 1245 bytes

🔄 Step 6: Verifying Uploads (Download Test)...
   📥 Downloading a1b2c3d4... ✓ Verified
   📥 Downloading e5f6g7h8... ✓ Verified
   📥 Downloading i9j0k1l2... ✓ Verified
   📥 Downloading m3n4o5p6... ✓ Verified
   📥 Downloading q7r8s9t0... ✓ Verified

   📊 Verification Statistics:
      Successfully verified: 5/5

🚦 Step 7: Testing Traffic Branch System...
   Testing Green branch (safe deployments)...
   ✓ Update analyzed: Recommended branch = Green

📜 Step 8: Generating Forge Commit Log...

   Commit History:
   ─────────────────────────────────────────────────────────────
   commit a1b2c3d4
   Author: forge-demo
   Date:   2025-11-10 12:00:00 UTC
   
       Add README.md
   
   commit e5f6g7h8
   Author: forge-demo
   Date:   2025-11-10 12:00:01 UTC
   
       Add Cargo.toml
   ...
   ─────────────────────────────────────────────────────────────

🌐 Step 9: R2 Storage URLs...

   You can verify the uploads in Cloudflare Dashboard:
   URL: https://dash.cloudflare.com/?to=/:account/r2/overview/buckets/forge

   Blob paths in R2:
   • README.md → blobs/a1/b2c3d4...
   • Cargo.toml → blobs/e5/f6g7h8...
   • src/main.rs → blobs/i9/j0k1l2...
   • src/lib.rs → blobs/m3/n4o5p6...
   • .forge/config.toml → blobs/q7/r8s9t0...

╔══════════════════════════════════════════════════════════════╗
║  Demo Complete! Summary:                                     ║
╠══════════════════════════════════════════════════════════════╣
║  ✓ Files processed:  5                                       ║
║  ✓ Blobs uploaded to R2:  5                                  ║
║  ✓ Blobs verified:  5                                        ║
║  ✓ Compression ratio: 50.0%                                  ║
║  ✓ Traffic branches: Active                                  ║
║  ✓ LSP detection: Tested                                     ║
╠══════════════════════════════════════════════════════════════╣
║  🎉 Forge is fully operational with R2 storage!              ║
╚══════════════════════════════════════════════════════════════╝
```

## 🔍 Verifying in Cloudflare Dashboard

After running the demo, verify the uploads:

1. Go to <https://dash.cloudflare.com/?to=/:account/r2/overview>
2. Click on your bucket (e.g., `forge`)
3. Navigate to `blobs/` directory
4. You should see subdirectories like `a1/`, `e5/`, etc.
5. Each contains a blob file with the full SHA-256 hash

## 📂 File Structure in R2

```
forge/                          # Your R2 bucket
└── blobs/                      # Blob storage directory
    ├── a1/                     # First 2 chars of hash
    │   └── b2c3d4...           # Remaining hash chars (blob file)
    ├── e5/
    │   └── f6g7h8...
    ├── i9/
    │   └── j0k1l2...
    ├── m3/
    │   └── n4o5p6...
    └── q7/
        └── r8s9t0...
```

## 🚦 Traffic Branch System

The demo tests the traffic branch system:

- **Green Branch**: Safe production deployments (low risk)
- **Yellow Branch**: Testing/staging deployments (medium risk)
- **Red Branch**: Development/experimental (high risk)

The system automatically recommends the appropriate branch based on:
- Change magnitude (lines modified)
- Hash similarity (content similarity)
- Previous component state

## 🔧 How It Works

### 1. Blob Creation

Each file is converted to a binary blob:

```rust
pub struct Blob {
    pub hash: String,           // SHA-256 content address
    pub metadata: BlobMetadata, // File path, size, timestamp, author
    pub content: Vec<u8>,       // Raw file content
}
```

### 2. Binary Format

Blobs are serialized as:

```
[metadata_len: u32][json_metadata: JSON][content: bytes]
```

### 3. Compression

LZ4 compression is applied:
- Text files: 50-75% compression
- Binary files: 10-30% compression
- 10-50x faster than gzip

### 4. Content Addressing

SHA-256 hashing ensures:
- ✓ Deduplication (identical content → same hash)
- ✓ Integrity verification (tampering detected)
- ✓ Efficient storage (no duplicates)

### 5. R2 Upload

Blobs are uploaded via S3-compatible API:
- AWS Signature V4 authentication
- PUT requests to `/<bucket>/blobs/<hash>`
- Zero egress fees (free downloads)

## 💰 Cost Estimate

For this demo (5 files, ~2.5 KB total):

```
Storage:   ~2.5 KB → $0.00/month (within free tier)
Uploads:   5 writes → $0.00 (negligible)
Downloads: 5 reads  → $0.00 (negligible)
Egress:    $0.00 (FREE!)

Total: $0.00/month
```

Compare to AWS S3:
- Storage: $0.00
- Uploads: $0.00
- Downloads: $0.00
- **Egress: Would cost $0.09/GB if scaled up**

**R2 advantage: Zero egress fees forever!**

## 🐛 Troubleshooting

### Error: "Invalid credentials"

```bash
# Check your .env file
cat .env | grep R2_

# Verify credentials at:
# https://dash.cloudflare.com/?to=/:account/r2/overview
```

### Error: "Bucket not found"

```bash
# Create bucket at:
# https://dash.cloudflare.com/?to=/:account/r2/overview

# Verify bucket name in .env matches exactly
```

### Error: "Permission denied"

```bash
# Recreate API token with "Admin Read & Write" permissions
# https://dash.cloudflare.com/?to=/:account/r2/overview
# Click "Manage R2 API Tokens"
```

### Error: "Failed to read file"

```bash
# Ensure demo directory exists
ls examples/forge-demo/

# If missing, the example script creates it automatically
```

## 🧪 Testing Changes

Modify a file and run the demo again:

```bash
# Edit a demo file
echo "// New comment" >> examples/forge-demo/src/main.rs

# Run demo again
cargo run --example r2_demo

# You should see:
# - New blob created with different hash
# - Both old and new blobs in R2
# - Delta detection in traffic branch system
```

## 🔐 Security Notes

1. **Never commit .env to Git** - Contains sensitive credentials
2. **Use Account API tokens** for production (not User tokens)
3. **Scope tokens** to specific buckets when possible
4. **Rotate credentials** regularly (every 90 days recommended)
5. **Enable 2FA** on your Cloudflare account

## 📚 Related Documentation

- [Storage Analysis](../docs/STORAGE_ANALYSIS.md) - Format comparison (FlatBuffers vs Git)
- [FAQ](../docs/FAQ.md) - Common questions and answers
- [Quick Start](../docs/QUICK_START.md) - 5-minute setup guide
- [R2 Implementation](../docs/R2_IMPLEMENTATION_SUMMARY.md) - Technical details

## 🎓 Learn More

- **Cloudflare R2**: <https://developers.cloudflare.com/r2/>
- **S3 API Compatibility**: <https://developers.cloudflare.com/r2/api/s3/api/>
- **Forge Documentation**: `docs/` directory

## 🤝 Contributing

Found an issue or want to improve the demo?

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Run the demo to test
5. Submit a pull request

---

**Made with ❤️ using Forge VCS and Cloudflare R2**
