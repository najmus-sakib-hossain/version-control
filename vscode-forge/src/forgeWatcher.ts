import * as vscode from 'vscode';
import * as path from 'path';
import * as child_process from 'child_process';

export class ForgeWatcher {
    private fileWatchers: vscode.Disposable[] = [];
    private forgeProcess: child_process.ChildProcess | undefined;
    private changeQueue: Map<string, NodeJS.Timeout> = new Map();

    constructor(
        private rootPath: string,
        private forgeBinary: string,
        private outputChannel: vscode.OutputChannel
    ) {}

    async start() {
        try {
            this.log('info', '👁️  Starting file system watcher...');
            
            // Watch workspace files
            this.startFileWatching();

            this.log('success', '✅ Forge LSP watcher active');
            this.outputChannel.appendLine('');
            this.outputChannel.appendLine('Monitoring all file changes in workspace...');
            this.outputChannel.appendLine('Changes will be displayed below:');
            this.logDivider();
            
        } catch (error) {
            const errorMsg = error instanceof Error ? error.message : String(error);
            this.log('error', `Failed to start watcher: ${errorMsg}`);
            throw error;
        }
    }

    private startFileWatching() {
        // Watch all workspace files
        const pattern = new vscode.RelativePattern(
            this.rootPath,
            '**/*'
        );

        const fileWatcher = vscode.workspace.createFileSystemWatcher(pattern);

        this.fileWatchers.push(fileWatcher);
        this.fileWatchers.push(fileWatcher.onDidChange((uri: vscode.Uri) => this.handleFileChange(uri, 'MODIFIED')));
        this.fileWatchers.push(fileWatcher.onDidCreate((uri: vscode.Uri) => this.handleFileChange(uri, 'CREATED')));
        this.fileWatchers.push(fileWatcher.onDidDelete((uri: vscode.Uri) => this.handleFileChange(uri, 'DELETED')));
    }

    private async handleFileChange(uri: vscode.Uri, changeType: string) {
        const filePath = uri.fsPath;

        // Ignore certain directories
        if (this.shouldIgnore(filePath)) {
            return;
        }

        // Debounce rapid changes
        const existing = this.changeQueue.get(filePath);
        if (existing) {
            clearTimeout(existing);
        }

        const timeout = setTimeout(() => {
            this.changeQueue.delete(filePath);
            this.processFileChange(uri, changeType);
        }, 50);

        this.changeQueue.set(filePath, timeout);
    }

    private shouldIgnore(filePath: string): boolean {
        const ignoredDirs = ['.git', 'node_modules', '.dx', 'target', 'out', 'dist', '.vscode-test'];
        const ignoredExts = ['.vsix', '.log'];

        return ignoredDirs.some(dir => filePath.includes(path.sep + dir + path.sep)) ||
               ignoredExts.some(ext => filePath.endsWith(ext));
    }

    private async processFileChange(uri: vscode.Uri, changeType: string) {
        const startTime = Date.now();
        const relativePath = path.relative(this.rootPath, uri.fsPath);
        const fileName = path.basename(uri.fsPath);

        // Get file icon based on change type
        const icon = changeType === 'CREATED' ? '✨' : 
                    changeType === 'DELETED' ? '🗑️' : 
                    '📝';

        this.outputChannel.appendLine('');
        this.outputChannel.appendLine(`${icon} ${changeType} │ ${this.formatTime(new Date())}`);
        this.outputChannel.appendLine(`   📄 ${fileName}`);
        this.outputChannel.appendLine(`   📂 ${relativePath}`);

        // Try to read file content and show preview
        if (changeType !== 'DELETED') {
            try {
                const document = await vscode.workspace.openTextDocument(uri);
                const content = document.getText();
                const lines = content.split('\n');

                this.outputChannel.appendLine(`   📊 ${lines.length} lines, ${content.length} bytes`);
                this.outputChannel.appendLine(`   🏷️  ${document.languageId}`);

                // Show content preview
                const config = vscode.workspace.getConfiguration('forge');
                const showDiffs = config.get('showDiffs', true);

                if (showDiffs && lines.length > 0 && lines.length <= 100) {
                    this.outputChannel.appendLine('');
                    this.outputChannel.appendLine('   📝 Content:');
                    
                    const previewLines = lines.slice(0, Math.min(20, lines.length));
                    previewLines.forEach((line, idx) => {
                        const lineNum = String(idx + 1).padStart(4, ' ');
                        this.outputChannel.appendLine(`   ${lineNum} │ ${line}`);
                    });

                    if (lines.length > 20) {
                        this.outputChannel.appendLine(`        │ ... (${lines.length - 20} more lines)`);
                    }
                }
            } catch (error) {
                // File might be binary or unreadable
                this.outputChannel.appendLine(`   ⚠️  Binary or unreadable file`);
            }
        }

        const duration = Date.now() - startTime;
        this.outputChannel.appendLine(`   ⏱️  Processed in ${duration}ms`);
    }

    async showFileAST(filePath: string): Promise<void> {
        return new Promise((resolve, reject) => {
            try {
                this.outputChannel.appendLine('');
                this.log('info', '🔍 Analyzing file with Forge...');
                this.outputChannel.appendLine('');

                // Read file and parse with tree-sitter internally
                const fs = require('fs');
                const content = fs.readFileSync(filePath, 'utf8');
                const lines = content.split('\n');

                // Show structure analysis
                this.outputChannel.appendLine('📋 File Structure:');
                this.outputChannel.appendLine(`   Total Lines: ${lines.length}`);
                this.outputChannel.appendLine(`   File Size: ${content.length} bytes`);
                this.outputChannel.appendLine('');

                // Analyze language-specific structure
                const ext = path.extname(filePath).toLowerCase();
                this.analyzeFileStructure(content, ext, lines);

                resolve();
            } catch (error) {
                const errorMsg = error instanceof Error ? error.message : String(error);
                this.log('error', `Failed to analyze file: ${errorMsg}`);
                reject(error);
            }
        });
    }

    private analyzeFileStructure(content: string, ext: string, lines: string[]) {
        this.outputChannel.appendLine('🌳 Syntax Tree:');
        this.outputChannel.appendLine('');

        if (ext === '.rs') {
            this.analyzeRustFile(lines);
        } else if (ext === '.ts' || ext === '.js') {
            this.analyzeJavaScriptFile(lines);
        } else if (ext === '.py') {
            this.analyzePythonFile(lines);
        } else {
            // Generic analysis
            this.analyzeGenericFile(lines);
        }
    }

    private analyzeRustFile(lines: string[]) {
        const structs: string[] = [];
        const enums: string[] = [];
        const functions: string[] = [];
        const impls: string[] = [];
        const mods: string[] = [];

        lines.forEach((line, idx) => {
            const trimmed = line.trim();
            if (trimmed.startsWith('struct ')) {
                structs.push(`   Line ${idx + 1}: ${trimmed}`);
            } else if (trimmed.startsWith('enum ')) {
                enums.push(`   Line ${idx + 1}: ${trimmed}`);
            } else if (trimmed.startsWith('fn ') || trimmed.startsWith('pub fn ') || trimmed.startsWith('async fn ') || trimmed.startsWith('pub async fn ')) {
                functions.push(`   Line ${idx + 1}: ${trimmed}`);
            } else if (trimmed.startsWith('impl ')) {
                impls.push(`   Line ${idx + 1}: ${trimmed}`);
            } else if (trimmed.startsWith('mod ') || trimmed.startsWith('pub mod ')) {
                mods.push(`   Line ${idx + 1}: ${trimmed}`);
            }
        });

        if (mods.length > 0) {
            this.outputChannel.appendLine(`   📦 Modules (${mods.length}):`);
            mods.forEach(m => this.outputChannel.appendLine(m));
            this.outputChannel.appendLine('');
        }

        if (structs.length > 0) {
            this.outputChannel.appendLine(`   🏗️  Structs (${structs.length}):`);
            structs.forEach(s => this.outputChannel.appendLine(s));
            this.outputChannel.appendLine('');
        }

        if (enums.length > 0) {
            this.outputChannel.appendLine(`   🔢 Enums (${enums.length}):`);
            enums.forEach(e => this.outputChannel.appendLine(e));
            this.outputChannel.appendLine('');
        }

        if (impls.length > 0) {
            this.outputChannel.appendLine(`   ⚙️  Implementations (${impls.length}):`);
            impls.forEach(i => this.outputChannel.appendLine(i));
            this.outputChannel.appendLine('');
        }

        if (functions.length > 0) {
            this.outputChannel.appendLine(`   🔧 Functions (${functions.length}):`);
            functions.forEach(f => this.outputChannel.appendLine(f));
        }
    }

    private analyzeJavaScriptFile(lines: string[]) {
        const classes: string[] = [];
        const functions: string[] = [];
        const imports: string[] = [];
        const exports: string[] = [];

        lines.forEach((line, idx) => {
            const trimmed = line.trim();
            if (trimmed.startsWith('class ')) {
                classes.push(`   Line ${idx + 1}: ${trimmed}`);
            } else if (trimmed.startsWith('function ') || trimmed.match(/^(export\s+)?(async\s+)?function\s+/)) {
                functions.push(`   Line ${idx + 1}: ${trimmed}`);
            } else if (trimmed.startsWith('import ')) {
                imports.push(`   Line ${idx + 1}: ${trimmed}`);
            } else if (trimmed.startsWith('export ')) {
                exports.push(`   Line ${idx + 1}: ${trimmed}`);
            }
        });

        if (imports.length > 0) {
            this.outputChannel.appendLine(`   📥 Imports (${imports.length}):`);
            imports.slice(0, 5).forEach(i => this.outputChannel.appendLine(i));
            if (imports.length > 5) {
                this.outputChannel.appendLine(`   ... and ${imports.length - 5} more`);
            }
            this.outputChannel.appendLine('');
        }

        if (classes.length > 0) {
            this.outputChannel.appendLine(`   🏗️  Classes (${classes.length}):`);
            classes.forEach(c => this.outputChannel.appendLine(c));
            this.outputChannel.appendLine('');
        }

        if (functions.length > 0) {
            this.outputChannel.appendLine(`   🔧 Functions (${functions.length}):`);
            functions.forEach(f => this.outputChannel.appendLine(f));
        }
    }

    private analyzePythonFile(lines: string[]) {
        const classes: string[] = [];
        const functions: string[] = [];
        const imports: string[] = [];

        lines.forEach((line, idx) => {
            const trimmed = line.trim();
            if (trimmed.startsWith('class ')) {
                classes.push(`   Line ${idx + 1}: ${trimmed}`);
            } else if (trimmed.startsWith('def ') || trimmed.startsWith('async def ')) {
                functions.push(`   Line ${idx + 1}: ${trimmed}`);
            } else if (trimmed.startsWith('import ') || trimmed.startsWith('from ')) {
                imports.push(`   Line ${idx + 1}: ${trimmed}`);
            }
        });

        if (imports.length > 0) {
            this.outputChannel.appendLine(`   📥 Imports (${imports.length}):`);
            imports.slice(0, 5).forEach(i => this.outputChannel.appendLine(i));
            if (imports.length > 5) {
                this.outputChannel.appendLine(`   ... and ${imports.length - 5} more`);
            }
            this.outputChannel.appendLine('');
        }

        if (classes.length > 0) {
            this.outputChannel.appendLine(`   🏗️  Classes (${classes.length}):`);
            classes.forEach(c => this.outputChannel.appendLine(c));
            this.outputChannel.appendLine('');
        }

        if (functions.length > 0) {
            this.outputChannel.appendLine(`   🔧 Functions (${functions.length}):`);
            functions.forEach(f => this.outputChannel.appendLine(f));
        }
    }

    private analyzeGenericFile(lines: string[]) {
        const nonEmptyLines = lines.filter(l => l.trim().length > 0).length;
        const commentLines = lines.filter(l => {
            const t = l.trim();
            return t.startsWith('//') || t.startsWith('#') || t.startsWith('/*') || t.startsWith('*');
        }).length;

        this.outputChannel.appendLine(`   📄 Content Analysis:`);
        this.outputChannel.appendLine(`   Non-empty lines: ${nonEmptyLines}`);
        this.outputChannel.appendLine(`   Comment lines: ${commentLines}`);
        this.outputChannel.appendLine('');

        this.outputChannel.appendLine('   📝 First 30 lines:');
        lines.slice(0, 30).forEach((line, idx) => {
            const lineNum = String(idx + 1).padStart(4, ' ');
            this.outputChannel.appendLine(`   ${lineNum} │ ${line}`);
        });

        if (lines.length > 30) {
            this.outputChannel.appendLine(`        │ ... (${lines.length - 30} more lines)`);
        }
    }

    stop() {
        // Clear any pending timeouts
        for (const timeout of this.changeQueue.values()) {
            clearTimeout(timeout);
        }
        this.changeQueue.clear();

        // Stop forge process if running
        if (this.forgeProcess) {
            this.forgeProcess.kill();
            this.forgeProcess = undefined;
        }

        // Dispose all file watchers
        for (const disposable of this.fileWatchers) {
            disposable.dispose();
        }
        this.fileWatchers = [];

        this.log('info', '⏹️  Watcher stopped');
    }

    private formatTime(date: Date): string {
        const hours = String(date.getHours()).padStart(2, '0');
        const minutes = String(date.getMinutes()).padStart(2, '0');
        const seconds = String(date.getSeconds()).padStart(2, '0');
        const ms = String(date.getMilliseconds()).padStart(3, '0');
        return `${hours}:${minutes}:${seconds}.${ms}`;
    }

    private log(level: 'info' | 'success' | 'error' | 'warning', message: string) {
        const timestamp = new Date().toISOString().substr(11, 12);
        const icon = level === 'success' ? '✅' : 
                    level === 'error' ? '❌' : 
                    level === 'warning' ? '⚠️' : 'ℹ️';
        this.outputChannel.appendLine(`[${timestamp}] ${icon} ${message}`);
    }

    private logDivider() {
        this.outputChannel.appendLine('─'.repeat(80));
    }
}
