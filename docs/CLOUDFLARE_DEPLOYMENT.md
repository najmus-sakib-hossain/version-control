# Deploying Forge to Cloudflare Workers

This guide shows you how to deploy the Forge API backend to Cloudflare Workers with R2 storage for zero egress fees.

## Why Cloudflare Workers + R2?

**Cost Savings:**
- **AWS S3**: 10TB/month egress = ~$920/month
- **Cloudflare R2**: 10TB/month egress = **$0/month**

For a code hosting platform where users constantly pull data, this eliminates your biggest cost!

## Prerequisites

1. Cloudflare account (free tier works!)
2. Rust installed (`rustup`)
3. `wrangler` CLI installed

```bash
npm install -g wrangler
```

## Step 1: Create Cloudflare R2 Bucket

1. Go to https://dash.cloudflare.com/
2. Navigate to **R2** in the sidebar
3. Click **Create bucket**
4. Name it: `forge-blobs`
5. Click **Create**

## Step 2: Create R2 API Token

1. In R2 dashboard, click **Manage R2 API Tokens**
2. Click **Create API token**
3. Give it a name: `forge-api`
4. Permissions: **Object Read & Write**
5. Click **Create API Token**
6. **Save** the Access Key ID and Secret Access Key!

## Step 3: Configure Environment Variables

Create `.env` file in your Forge project:

```bash
# Copy example
cp .env.example .env

# Edit with your values
nano .env
```

Add your R2 credentials:

```env
R2_ACCOUNT_ID=your_account_id_here
R2_BUCKET_NAME=forge-blobs
R2_ACCESS_KEY_ID=your_access_key_id_here
R2_SECRET_ACCESS_KEY=your_secret_access_key_here
```

Find your Account ID:
- Go to Cloudflare dashboard
- Look for **Account ID** in the sidebar

## Step 4: Create Worker

Create `wrangler.toml` in your project root:

```toml
name = "forge-api"
main = "src/bin/worker.rs"
compatibility_date = "2024-01-01"

# Account details
account_id = "your_account_id_here"

# Workers configuration
workers_dev = true

# R2 bucket binding
[[r2_buckets]]
binding = "FORGE_BLOBS"
bucket_name = "forge-blobs"

# Environment variables (for production)
[env.production]
vars = { ENVIRONMENT = "production" }

[env.production.r2_buckets]
binding = "FORGE_BLOBS"
bucket_name = "forge-blobs"

# Build configuration
[build]
command = "cargo install -q worker-build && worker-build --release"

[build.upload]
format = "modules"
main = "./build/worker/shim.mjs"

[[build.upload.rules]]
globs = ["**/*.wasm"]
type = "CompiledWasm"

# Triggers
[triggers]
crons = []
```

## Step 5: Create Worker Entry Point

Create `src/bin/worker.rs`:

```rust
use worker::*;
use dx_forge::server::api;
use std::sync::Arc;

#[event(start)]
fn start() {
    console_error_panic_hook::set_once();
}

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    // Convert Worker Request to Axum-compatible request
    // This is where worker-axum helper crate would be used
    
    // For now, simple routing
    match req.path().as_str() {
        "/health" => Response::ok("healthy"),
        _ => Response::error("Not Found", 404),
    }
}
```

## Step 6: Test Locally

```bash
# Install wrangler
npm install -g wrangler

# Login to Cloudflare
wrangler login

# Test locally
wrangler dev
```

Visit http://localhost:8787/health

## Step 7: Deploy to Production

```bash
# Deploy to Cloudflare
wrangler deploy

# Your API is now live!
# https://forge-api.your-account.workers.dev
```

## Step 8: Configure Forge CLI to Use Your API

In your local project, configure Forge to use your deployed API:

```bash
# Set API endpoint
forge config set api.endpoint https://forge-api.your-account.workers.dev

# Or use custom domain
forge config set api.endpoint https://api.yourproject.com
```

## Step 9: Custom Domain (Optional)

1. Go to your Worker in Cloudflare dashboard
2. Click **Triggers**
3. Click **Add Custom Domain**
4. Enter: `api.yourproject.com`
5. Click **Add Custom Domain**

Cloudflare automatically handles SSL certificates!

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                     Your Users                              │
│                        ↓                                     │
│                   forge sync                                 │
│                        ↓                                     │
├─────────────────────────────────────────────────────────────┤
│              Cloudflare Workers (Edge)                      │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Axum API (Rust)                                     │  │
│  │  - /api/v1/blobs (upload/download)                   │  │
│  │  - /api/v1/repos/:id/sync                            │  │
│  │  - /health                                            │  │
│  └──────────────────────────────────────────────────────┘  │
│                        ↓                                     │
├─────────────────────────────────────────────────────────────┤
│              Cloudflare R2 (Object Storage)                 │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Blob Storage (content-addressable)                  │  │
│  │  - blobs/ab/cdef123... (SHA-256 addressed)          │  │
│  │  - Zero egress fees!                                 │  │
│  │  - Global replication                                │  │
│  └──────────────────────────────────────────────────────┘  │
├─────────────────────────────────────────────────────────────┤
│              Neon Database (PostgreSQL)                     │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Metadata Storage                                    │  │
│  │  - User accounts                                     │  │
│  │  - Repository metadata                               │  │
│  │  - Operation logs                                    │  │
│  └──────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

## Cost Breakdown

### AWS S3 Alternative (Traditional)
```
Storage: 1TB @ $0.023/GB        = $23.55/mo
Egress: 10TB @ $0.09/GB         = $921.60/mo
Requests: 10M @ $0.004/1000     = $40.00/mo
───────────────────────────────────────────
Total:                          = $985.15/mo
```

### Cloudflare R2 + Workers (Forge)
```
Storage: 1TB @ $0.015/GB        = $15.36/mo
Egress: 10TB @ $0.00/GB         = $0.00/mo
Operations: 10M @ $0.36/1M      = $3.60/mo
Workers: 10M requests FREE      = $0.00/mo
───────────────────────────────────────────
Total:                          = $18.96/mo
```

**Savings: $966/month (98% cost reduction!)**

## Production Checklist

- [ ] R2 bucket created
- [ ] API token generated and saved
- [ ] `.env` configured with credentials
- [ ] `wrangler.toml` configured
- [ ] Worker deployed: `wrangler deploy`
- [ ] Custom domain configured (optional)
- [ ] Health check working: `curl https://your-api.workers.dev/health`
- [ ] Blob upload tested
- [ ] Blob download tested
- [ ] Neon database connected (optional)

## Monitoring

View logs in Cloudflare dashboard:

```bash
# Stream logs
wrangler tail

# View in dashboard
# https://dash.cloudflare.com/workers/overview
```

## Scaling

Cloudflare Workers automatically scale to handle:
- Millions of requests per second
- Global edge deployment (200+ locations)
- Zero configuration required
- No servers to manage

## Troubleshooting

### "Bucket not found"
- Check `R2_BUCKET_NAME` matches exactly
- Verify bucket exists in R2 dashboard

### "Authentication failed"
- Verify `R2_ACCESS_KEY_ID` and `R2_SECRET_ACCESS_KEY`
- Check API token permissions include Read & Write

### "Module not found"
- Run: `cargo build --release`
- Run: `wrangler deploy` again

## Next Steps

1. **Add authentication**: Implement JWT tokens
2. **Rate limiting**: Use Cloudflare Workers KV
3. **Monitoring**: Set up Cloudflare Analytics
4. **CDN**: Enable Cloudflare CDN for static assets
5. **DDoS protection**: Included free with Cloudflare

## Learn More

- [Cloudflare Workers Docs](https://developers.cloudflare.com/workers/)
- [Cloudflare R2 Docs](https://developers.cloudflare.com/r2/)
- [worker-rs Crate](https://github.com/cloudflare/workers-rs)
- [Axum Docs](https://docs.rs/axum)

---

**Questions?** Open an issue on GitHub or join our Discord!
