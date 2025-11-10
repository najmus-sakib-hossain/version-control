---

# Forge: The Successor to Git & The Heart of DX

![Forge Logo](https://img.shields.io/badge/Forge-v1.0-blueviolet)
![Build Status](https://img.shields.io/github/actions/workflow/status/your-repo/forge/rust.yml?branch=main)
![Rust Version](https://img.shields.io/badge/rust-1.65+-93450a.svg)
![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)

**Forge is the successor to Git.** It is a ground-up reimagining of version control and developer tooling, designed for the modern era of AI-assisted, component-driven development. As a standalone, binary-first platform, Forge completely replaces Git with a simpler, more powerful, and fully automated workflow.

Forge is not just a tool you use; it is the foundation your project is built upon, serving as its version control system, package manager, and intelligent runtime orchestrator for the entire `dx` ecosystem.

---

## The Forge Paradigm: A New Era of Development

Forge is built on a set of revolutionary principles designed to eliminate friction and keep you in a state of creative flow.

### 1. Human-Readable IDs: `20251011-1`
Forge abolishes random SHA-1 hashes in favor of a predictable, date-based identification system. This makes navigating your project's history as easy as remembering a date.

-   **The Old Way (Git):** `git switch 5a3b4c1e9f2d...`
-   **The Forge Way:** `git switch 20251011-1` *(The 1st commit on Oct 11, 2025)*

This simple change makes your history immediately navigable and transforms commands like `diff`, `switch`, and `cherry-pick` into intuitive actions.

### 2. Separating Existence from Evolution
Forge understands that creating a file is different from changing it. It automates the tedious parts of version control so you can focus on what matters.

-   **Automatic History:** When you create or delete a file, Forge's daemon instantly records this as an `[AUTO]` event in the project history. **No commands needed.**
-   **Meaningful Commits:** You only use `forge commit` to save **content modifications**. This creates a clean, high-signal log focused on your creative intent, not on boilerplate file management.

### 3. Seamless Git Compatibility
Despite its revolutionary internals, Forge maintains **100% compatibility with Git's command-line interface**.

-   There is **zero learning curve** for millions of developers.
-   `forge add`, `forge commit`, `forge push`, `forge log`, `forge branch`—every command you already know works instantly, but is backed by a more intelligent and intuitive engine.

---

## The DX Orchestrator: Forge as Master Conductor

Forge is the central nervous system of your project, managing the entire lifecycle of your `dx` tools.

### Integrated Package & Version Manager
Your project's `dx.toml` file is the single source of truth for your tooling. Forge reads this file and acts as a complete package manager.

```toml
# dx.toml
[dx]
# Forge manages the installation and versioning of all dx tools.
tools = [
  "style@1.2.0",
  "check@1.1.0",
  "ui@latest"
]

# Forge intelligently schedules when each tool should run.
[dx.run_order]
on_change = [
  "dx-style",  # 1. First, generate CSS from utility classes.
  "dx-check"   # 2. Then, format and lint the changed file.
]
```

When you run `dx watch`, Forge ensures the correct versions of these tools are downloaded and available, eliminating dependency hell for your development environment.

### The Intelligent Runtime Scheduler
Forge understands that the *order* of operations is critical. It uses the `run_order` to create an intelligent execution pipeline, preventing race conditions and ensuring a flawless experience.

1.  You add a utility class to your code.
2.  Forge detects the change and dispatches an event to `dx-style` first.
3.  After `dx-style` generates the necessary CSS, Forge then dispatches an event to `dx-check` to format your file.

This orchestration ensures tools always operate on the most up-to-date state, making the system feel magical and "just work."

---

## The Smart Update System: Red, Yellow, & Green Branching

This is Forge's killer feature for dependency management. It intelligently updates DX-managed components (from `dx-ui`, `dx-icon`, etc.) using a traffic light system, all without a manifest file.

**The Mechanism:** When `dx-ui` adds a component like `Button.tsx`, Forge hashes its content and stores that `base_hash` in its internal `.dx/forge/component_state.json` file. This `base_hash` is the reference point for all future updates.

When a new version of `Button.tsx` is released, Forge compares the `BASE` (from its state), your `LOCAL` file, and the new `REMOTE` version.

### 🟢 Green Branch: Automatic & Safe
-   **Condition:** You have **not** modified the local component (`hash(LOCAL) == base_hash`).
-   **Action:** Forge automatically and safely overwrites your local file with the new version. Your component is updated seamlessly in the background.
-   **Example:**
    ```bash
    $ forge update Button
    🟢 components/Button.tsx updated to v2.0.0 (auto-updated)
    ```

### 🟡 Yellow Branch: Merged & Preserved
-   **Condition:** You **have** modified the local component, but your changes **do not conflict** with the upstream updates.
-   **Action:** Forge performs a 3-way merge, combining the author's updates with your custom modifications. Your work is preserved.
-   **Example:**
    ```bash
    $ forge update Button
    🟡 components/Button.tsx updated to v2.0.0 (merged with local changes)
    ```

### 🔴 Red Branch: Conflict & Manual Action
-   **Condition:** You and the component author have both modified the **same lines of code**, resulting in a merge conflict.
-   **Action:** Forge **protects your code** and does not make any changes. It provides detailed conflict information.
-   **Example:**
    ```bash
    $ forge update Button
    🔴 CONFLICT: components/Button.tsx v2.0.0
       │ Update conflicts with your local changes:
       │ Conflict at lines 15-20
       └ Run forge resolve to resolve
    ```

### Managing Components

```bash
# Register a component for tracking
$ forge register components/Button.tsx --source dx-ui --name Button --version 1.0.0

# List all managed components
$ forge components
📦 Managed Components
═══════════════════════════════════════════════════════
● Button v1.0.0
   Source: dx-ui
   Path:   components/Button.tsx

# Update a specific component
$ forge update Button

# Update all components
$ forge update all
```

---

## Intelligent Detection: LSP vs File Watching

Forge automatically detects your development environment and chooses the optimal change detection method.

### 📡 LSP-Based Detection (Preferred)

When a DX code editor extension is installed, Forge uses the **Language Server Protocol** for change detection:

-   **Lower Latency:** Direct editor events without file system polling
-   **Precise Tracking:** Exact character-level edits from the editor
-   **Better Integration:** Seamless with editor features and workflows  
-   **Reduced CPU Usage:** No continuous file system scanning

**Installation:**
```bash
# VS Code
code --install-extension dx.forge-extension

# Or manually: Search "DX Forge" in editor extension marketplace
```

### 👁️ File Watching (Fallback)

When no LSP extension is detected, Forge falls back to high-performance file system watching:

-   **Ultra-Fast:** Sub-35µs rapid change detection (typically 1-2µs)
-   **Dual-Mode:** Rapid events + quality analysis with full operation details
-   **Production-Ready:** Optimized with memory-mapped I/O and SIMD acceleration

**Detection at Startup:**
```bash
$ forge watch

# With LSP extension:
📡 LSP-based detection mode enabled
→ Listening for LSP events...

# Without LSP extension:
👁️  File watching mode (no LSP extension detected)
✔ Starting operation-level tracking...
```

The system automatically chooses the best method—you don't need to configure anything!

---

## Ecosystem Safeguards: Your Safety Net

Forge's deep integration with the `dx` ecosystem allows it to provide powerful protections.

### Special Treatment for DX-Generated Files
Forge knows which files in your project are managed by `dx` tools. It uses this knowledge to protect you from common mistakes.

-   **Deletion Protection:** If you try to delete a managed file like `Button.tsx`, Forge intercepts the action and prompts you for confirmation, explaining the consequences.
-   **Contextual Hints:** If you start editing a pristine `dx` component, the editor can provide a subtle notification that your changes will be smartly merged during future updates (the Yellow Branch).
-   **Clean Status:** The `forge status` command provides a clean overview, separating your own files from the status of managed `dx` components.

### The Archive: Never Lose a Line of Code
Forge believes automation should never lead to data loss. When a `dx` tool determines a file is no longer needed, it is not deleted.

-   **Action:** Forge moves the file to a special `.dx/archive/` directory, time-stamped for easy identification.
-   **User Control:** You have full control over the archive with simple commands:
    -   `forge archive list`: See all archived files.
    -   `forge archive restore <file>`: Instantly restore a file to its original location.
    -   `forge archive purge`: Permanently delete archived files you are sure you no longer need.

---

## Getting Started: A Glimpse of the Future

1.  **Initialize Your Project:**
    ```bash
    $ forge init my-awesome-project
    $ cd my-awesome-project
    ```
2.  **Start the Forge Daemon:**
    ```bash
    # This single command starts the file watcher, scheduler, and all dx tools.
    $ dx watch
    ```
3.  **Create & Modify:**
    -   Create a new file, `src/app.js`. Forge automatically detects it and adds it to the history. No command needed.
    -   Modify the file with your code.
4.  **Save Your Work:**
    ```bash
    # Stage your content changes.
    $ forge add src/app.js

    # Commit your work with a meaningful message and get a human-readable ID.
    $ forge commit -m "Implement initial app logic"
    > [20251011-1] saved 1 file modification.
    ```
5.  **View Your History:**
    ```bash
    $ forge log
    > commit 20251011-1 (HEAD -> main)
    > Author: You
    >
    >     Implement initial app logic
    >
    > event 20251011-0 [AUTO]
    > Type:   ADD
    > Target: src/app.js
    ```
---

By unifying version control, package management, and runtime orchestration into a single, cohesive platform, Forge delivers on the ultimate promise of `dx`: an environment where the developer can exist in a pure state of creative flow. **Welcome to the future of development.**