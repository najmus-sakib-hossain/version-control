import * as vscode from 'vscode';
import * as path from 'path';
import * as child_process from 'child_process';
import { ForgeWatcher } from './forgeWatcher';

let forgeWatcher: ForgeWatcher | undefined;
let outputChannel: vscode.OutputChannel;
let statusBarItem: vscode.StatusBarItem;

export function activate(context: vscode.ExtensionContext) {
    console.log('🚀 DX Forge LSP Extension activated!');

    // Create output channel
    outputChannel = vscode.window.createOutputChannel('Forge LSP');
    context.subscriptions.push(outputChannel);

    // Create status bar item
    statusBarItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 100);
    statusBarItem.text = '$(eye) Forge LSP';
    statusBarItem.tooltip = 'Forge LSP: Inactive';
    statusBarItem.command = 'forge.start';
    context.subscriptions.push(statusBarItem);
    statusBarItem.show();

    // Register commands
    context.subscriptions.push(
        vscode.commands.registerCommand('forge.start', () => startWatching())
    );

    context.subscriptions.push(
        vscode.commands.registerCommand('forge.stop', () => stopWatching())
    );

    context.subscriptions.push(
        vscode.commands.registerCommand('forge.clearOutput', () => {
            outputChannel.clear();
            logInfo('✨ Output cleared');
        })
    );

    context.subscriptions.push(
        vscode.commands.registerCommand('forge.showAST', () => showCurrentFileAST())
    );

    // Auto-start if configured
    const config = vscode.workspace.getConfiguration('forge');
    if (config.get('autoStart', true)) {
        startWatching();
    }
}

async function startWatching() {
    if (forgeWatcher) {
        vscode.window.showInformationMessage('Forge LSP is already running');
        return;
    }

    const workspaceFolders = vscode.workspace.workspaceFolders;
    if (!workspaceFolders || workspaceFolders.length === 0) {
        vscode.window.showErrorMessage('No workspace folder open');
        return;
    }

    const rootPath = workspaceFolders[0].uri.fsPath;

    outputChannel.clear();
    logHeader('🚀 DX FORGE LSP');
    logInfo(`Monitoring: ${rootPath}`);
    logDivider();

    // Check if forge binary exists
    const forgeBinary = await findForgeBinary(rootPath);
    if (!forgeBinary) {
        vscode.window.showErrorMessage('Forge binary not found. Please build the forge project first.');
        logError('❌ Forge binary not found');
        logInfo('💡 Run: cargo build --release');
        return;
    }

    logSuccess(`✅ Found Forge binary: ${forgeBinary}`);
    logDivider();

    forgeWatcher = new ForgeWatcher(rootPath, forgeBinary, outputChannel);
    await forgeWatcher.start();

    // Update status bar
    statusBarItem.text = '$(eye) Forge LSP: Active';
    statusBarItem.tooltip = 'Forge LSP: Monitoring changes';
    statusBarItem.command = 'forge.stop';
    statusBarItem.backgroundColor = new vscode.ThemeColor('statusBarItem.prominentBackground');

    vscode.window.showInformationMessage('🚀 Forge LSP started');
}

function stopWatching() {
    if (!forgeWatcher) {
        vscode.window.showInformationMessage('Forge LSP is not running');
        return;
    }

    forgeWatcher.stop();
    forgeWatcher = undefined;

    logDivider();
    logInfo('⏹️  Forge LSP stopped');

    // Update status bar
    statusBarItem.text = '$(eye) Forge LSP';
    statusBarItem.tooltip = 'Forge LSP: Inactive';
    statusBarItem.command = 'forge.start';
    statusBarItem.backgroundColor = undefined;

    vscode.window.showInformationMessage('⏹️  Forge LSP stopped');
}

async function findForgeBinary(rootPath: string): Promise<string | null> {
    // Check common locations
    const possiblePaths = [
        path.join(rootPath, 'target', 'release', 'forge-cli.exe'),
        path.join(rootPath, 'target', 'release', 'forge-cli'),
        path.join(rootPath, 'target', 'debug', 'forge-cli.exe'),
        path.join(rootPath, 'target', 'debug', 'forge-cli'),
        'forge-cli.exe',
        'forge-cli'
    ];

    const fs = require('fs');
    for (const binPath of possiblePaths) {
        const fullPath = path.isAbsolute(binPath) ? binPath : path.join(rootPath, binPath);
        if (fs.existsSync(fullPath)) {
            return fullPath;
        }
    }

    return null;
}

async function showCurrentFileAST() {
    const editor = vscode.window.activeTextEditor;
    if (!editor) {
        vscode.window.showWarningMessage('No active file');
        return;
    }

    if (!forgeWatcher) {
        vscode.window.showWarningMessage('Forge LSP is not running. Start it first.');
        return;
    }

    const document = editor.document;
    const filePath = document.uri.fsPath;

    outputChannel.show();
    logDivider();
    logHeader('📊 FILE AST');
    logInfo(`File: ${path.basename(filePath)}`);
    logInfo(`Path: ${filePath}`);
    logInfo(`Lines: ${document.lineCount}`);
    logInfo(`Language: ${document.languageId}`);
    logDivider();

    // Use forgeWatcher to get AST
    await forgeWatcher.showFileAST(filePath);

    logDivider();
}

// Logging helpers
function logHeader(text: string) {
    outputChannel.appendLine('');
    outputChannel.appendLine('═'.repeat(80));
    outputChannel.appendLine(`  ${text}`);
    outputChannel.appendLine(`  ${new Date().toLocaleTimeString()}`);
    outputChannel.appendLine('═'.repeat(80));
    outputChannel.appendLine('');
}

function logDivider() {
    outputChannel.appendLine('─'.repeat(80));
}

function logInfo(message: string) {
    const timestamp = new Date().toISOString().substr(11, 12);
    outputChannel.appendLine(`[${timestamp}] ℹ️  ${message}`);
}

function logSuccess(message: string) {
    const timestamp = new Date().toISOString().substr(11, 12);
    outputChannel.appendLine(`[${timestamp}] ✅ ${message}`);
}

function logError(message: string) {
    const timestamp = new Date().toISOString().substr(11, 12);
    outputChannel.appendLine(`[${timestamp}] ❌ ${message}`);
}

function logWarning(message: string) {
    const timestamp = new Date().toISOString().substr(11, 12);
    outputChannel.appendLine(`[${timestamp}] ⚠️  ${message}`);
}

export function deactivate() {
    if (forgeWatcher) {
        forgeWatcher.stop();
    }
}
