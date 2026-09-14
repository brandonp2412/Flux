'use strict';

const fs = require('fs');
const path = require('path');
const vscode = require('vscode');
const { LspConnection } = require('./lsp-connection');

let client = null;
let output = null;
let diagnostics = null;
let terminal = null;
let extensionContext = null;

const selector = [{ language: 'flux', scheme: 'file' }];

async function activate(context) {
  extensionContext = context;
  output = vscode.window.createOutputChannel('Flux');
  diagnostics = vscode.languages.createDiagnosticCollection('flux');
  context.subscriptions.push(output, diagnostics);

  context.subscriptions.push(
    vscode.commands.registerCommand('flux.run', () => runFluxCommand('run')),
    vscode.commands.registerCommand('flux.check', () => runFluxCommand('check')),
    vscode.commands.registerCommand('flux.restartServer', restartClient),
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (event.affectsConfiguration('flux.executablePath')) {
        restartClient();
      }
    }),
  );

  await startClient(context);
}

async function deactivate() {
  if (client) {
    await stopClient();
  }
  if (terminal) {
    terminal.dispose();
    terminal = null;
  }
}

function workspaceFolderForDocument(document) {
  if (document) {
    const folder = vscode.workspace.getWorkspaceFolder(document.uri);
    if (folder) {
      return folder;
    }
  }
  return vscode.workspace.workspaceFolders && vscode.workspace.workspaceFolders[0];
}

function resolveFluxExecutable(folder) {
  const configured = vscode.workspace.getConfiguration('flux', folder && folder.uri)
    .get('executablePath', 'flux');
  if (configured && configured !== 'flux') {
    return configured;
  }

  if (folder) {
    const repoTool = path.join(folder.uri.fsPath, 'tools', 'flux');
    try {
      fs.accessSync(repoTool, fs.constants.X_OK);
      return repoTool;
    } catch (_) {
      // Normal Flux projects use the installed `flux` executable from PATH.
    }
  }

  return 'flux';
}

function rootUri() {
  const active = vscode.window.activeTextEditor && vscode.window.activeTextEditor.document;
  const folder = workspaceFolderForDocument(active);
  if (folder) {
    return folder.uri;
  }
  if (active && active.uri.scheme === 'file') {
    return vscode.Uri.file(path.dirname(active.uri.fsPath));
  }
  return null;
}

async function startClient(context) {
  if (client) {
    return;
  }

  const active = vscode.window.activeTextEditor && vscode.window.activeTextEditor.document;
  const folder = workspaceFolderForDocument(active);
  const executable = resolveFluxExecutable(folder);
  const root = rootUri();
  const connection = new LspConnection(executable, ['lsp'], {
    cwd: root ? root.fsPath : undefined,
  });

  connection.on('stderr', (text) => output.append(text));
  connection.on('connectionError', (error) => {
    output.appendLine(`Flux LSP: ${error.message}`);
  });
  connection.handleRequest('workspace/configuration', (params) => {
    const count = params && Array.isArray(params.items) ? params.items.length : 0;
    return Array.from({ length: count }, () => ({}));
  });
  connection.handleRequest('workspace/workspaceFolders', () => workspaceFolders());
  connection.handleRequest('client/registerCapability', () => null);
  connection.handleRequest('client/unregisterCapability', () => null);
  connection.handleRequest('window/showMessageRequest', () => null);
  connection.handleRequest('workspace/applyEdit', () => ({ applied: false }));
  connection.on('notification:textDocument/publishDiagnostics', publishDiagnostics);

  connection.start();

  let initializeResult;
  try {
    initializeResult = await connection.request('initialize', {
      processId: process.pid,
      clientInfo: { name: 'Flux VS Code', version: extensionVersion(context) },
      rootUri: root ? root.toString() : null,
      workspaceFolders: workspaceFolders(),
      capabilities: clientCapabilities(),
      initializationOptions: null,
    });
  } catch (error) {
    connection.closed = true;
    if (connection.child) {
      connection.child.kill();
    }
    const message = executable === 'flux'
      ? 'Flux language server could not start. Install the Flux CLI (`cargo install --path . --locked`) or set `flux.executablePath`.'
      : `Flux language server could not start from ${executable}.`;
    output.appendLine(`${message}\n${error.message}`);
    vscode.window.showErrorMessage(message);
    return;
  }

  connection.notify('initialized', {});

  const disposables = registerLanguageFeatures(connection, initializeResult.capabilities || {});
  disposables.push(
    vscode.workspace.onDidOpenTextDocument((document) => didOpen(connection, document)),
    vscode.workspace.onDidChangeTextDocument((event) => didChange(connection, event)),
    vscode.workspace.onDidCloseTextDocument((document) => didClose(connection, document)),
  );

  client = { connection, disposables };
  for (const document of vscode.workspace.textDocuments) {
    didOpen(connection, document);
  }
  output.appendLine(`Flux LSP started: ${executable} lsp`);
}

async function stopClient() {
  if (!client) {
    return;
  }
  const current = client;
  client = null;
  for (const disposable of current.disposables) {
    disposable.dispose();
  }
  diagnostics.clear();
  await current.connection.shutdown();
}

async function restartClient() {
  await stopClient();
  if (extensionContext) {
    await startClient(extensionContext);
  }
}

function extensionVersion(context) {
  return (context.extension && context.extension.packageJSON && context.extension.packageJSON.version) || '0.1.0';
}

function workspaceFolders() {
  return (vscode.workspace.workspaceFolders || []).map((folder) => ({
    uri: folder.uri.toString(),
    name: folder.name,
  }));
}

function clientCapabilities() {
  const tokenTypes = [
    'namespace', 'type', 'class', 'enum', 'interface', 'struct', 'typeParameter',
    'parameter', 'variable', 'property', 'enumMember', 'event', 'function', 'method',
    'macro', 'keyword', 'modifier', 'comment', 'string', 'number', 'regexp',
    'operator', 'decorator',
  ];
  return {
    workspace: {
      workspaceFolders: true,
      configuration: true,
      workspaceEdit: { documentChanges: true },
    },
    textDocument: {
      synchronization: { didSave: true },
      completion: {
        completionItem: {
          snippetSupport: true,
          documentationFormat: ['markdown', 'plaintext'],
        },
      },
      hover: { contentFormat: ['markdown', 'plaintext'] },
      definition: {},
      references: {},
      rename: {},
      formatting: {},
      codeAction: {
        codeActionLiteralSupport: {
          codeActionKind: { valueSet: ['quickfix', 'refactor', 'source'] },
        },
        isPreferredSupport: true,
      },
      signatureHelp: {
        signatureInformation: { documentationFormat: ['markdown', 'plaintext'] },
      },
      inlayHint: {},
      semanticTokens: {
        dynamicRegistration: false,
        tokenTypes,
        tokenModifiers: [],
        formats: ['relative'],
        requests: { range: false, full: true },
      },
    },
    general: { positionEncodings: ['utf-16'] },
  };
}

function didOpen(connection, document) {
  if (document.languageId !== 'flux' || document.uri.scheme !== 'file') {
    return;
  }
  connection.notify('textDocument/didOpen', {
    textDocument: {
      uri: document.uri.toString(),
      languageId: 'flux',
      version: document.version,
      text: document.getText(),
    },
  });
}

function didChange(connection, event) {
  const document = event.document;
  if (document.languageId !== 'flux' || document.uri.scheme !== 'file') {
    return;
  }
  connection.notify('textDocument/didChange', {
    textDocument: { uri: document.uri.toString(), version: document.version },
    contentChanges: [{ text: document.getText() }],
  });
}

function didClose(connection, document) {
  if (document.languageId !== 'flux' || document.uri.scheme !== 'file') {
    return;
  }
  connection.notify('textDocument/didClose', {
    textDocument: { uri: document.uri.toString() },
  });
  diagnostics.delete(document.uri);
}

function registerLanguageFeatures(connection, capabilities) {
  const disposables = [];

  if (capabilities.completionProvider) {
    const triggers = capabilities.completionProvider.triggerCharacters || [];
    disposables.push(vscode.languages.registerCompletionItemProvider(
      selector,
      {
        provideCompletionItems: async (document, position) => {
          const result = await request(connection, 'textDocument/completion', textDocumentPosition(document, position));
          const items = result && Array.isArray(result.items) ? result.items : (result || []);
          const converted = Array.isArray(items) ? items.map(toCompletionItem) : [];
          return result && Array.isArray(result.items)
            ? new vscode.CompletionList(converted, Boolean(result.isIncomplete))
            : converted;
        },
      },
      ...triggers,
    ));
  }

  if (capabilities.hoverProvider) {
    disposables.push(vscode.languages.registerHoverProvider(selector, {
      provideHover: async (document, position) => {
        const result = await request(connection, 'textDocument/hover', textDocumentPosition(document, position));
        if (!result) return null;
        return new vscode.Hover(toMarkdown(result.contents), result.range ? toRange(result.range) : undefined);
      },
    }));
  }

  if (capabilities.definitionProvider) {
    disposables.push(vscode.languages.registerDefinitionProvider(selector, {
      provideDefinition: async (document, position) => {
        const result = await request(connection, 'textDocument/definition', textDocumentPosition(document, position));
        if (!result) return null;
        const values = Array.isArray(result) ? result : [result];
        return values.map(toLocation).filter(Boolean);
      },
    }));
  }

  if (capabilities.referencesProvider) {
    disposables.push(vscode.languages.registerReferenceProvider(selector, {
      provideReferences: async (document, position, context) => {
        const result = await request(connection, 'textDocument/references', {
          ...textDocumentPosition(document, position),
          context: { includeDeclaration: context.includeDeclaration },
        });
        return Array.isArray(result) ? result.map(toLocation).filter(Boolean) : [];
      },
    }));
  }

  if (capabilities.renameProvider) {
    disposables.push(vscode.languages.registerRenameProvider(selector, {
      provideRenameEdits: async (document, position, newName) => {
        const result = await request(connection, 'textDocument/rename', {
          ...textDocumentPosition(document, position),
          newName,
        });
        return result ? toWorkspaceEdit(result) : null;
      },
    }));
  }

  if (capabilities.documentFormattingProvider) {
    disposables.push(vscode.languages.registerDocumentFormattingEditProvider(selector, {
      provideDocumentFormattingEdits: async (document, options) => {
        const result = await request(connection, 'textDocument/formatting', {
          textDocument: { uri: document.uri.toString() },
          options: { tabSize: options.tabSize, insertSpaces: options.insertSpaces },
        });
        return Array.isArray(result) ? result.map(toTextEdit) : [];
      },
    }));
  }

  if (capabilities.codeActionProvider) {
    disposables.push(vscode.languages.registerCodeActionsProvider(selector, {
      provideCodeActions: async (document, range, context) => {
        const result = await request(connection, 'textDocument/codeAction', {
          textDocument: { uri: document.uri.toString() },
          range: fromRange(range),
          context: { diagnostics: context.diagnostics.map(fromDiagnostic) },
        });
        return Array.isArray(result) ? result.map(toCodeAction) : [];
      },
    }, { providedCodeActionKinds: [vscode.CodeActionKind.QuickFix] }));
  }

  if (capabilities.signatureHelpProvider) {
    const triggers = capabilities.signatureHelpProvider.triggerCharacters || [];
    disposables.push(vscode.languages.registerSignatureHelpProvider(selector, {
      provideSignatureHelp: async (document, position) => {
        const result = await request(connection, 'textDocument/signatureHelp', textDocumentPosition(document, position));
        return result ? toSignatureHelp(result) : null;
      },
    }, ...triggers));
  }

  if (capabilities.inlayHintProvider) {
    disposables.push(vscode.languages.registerInlayHintsProvider(selector, {
      provideInlayHints: async (document, range) => {
        const result = await request(connection, 'textDocument/inlayHint', {
          textDocument: { uri: document.uri.toString() },
          range: fromRange(range),
        });
        return Array.isArray(result) ? result.map(toInlayHint) : [];
      },
    }));
  }

  if (capabilities.semanticTokensProvider && capabilities.semanticTokensProvider.legend) {
    const provider = capabilities.semanticTokensProvider;
    const legend = new vscode.SemanticTokensLegend(
      provider.legend.tokenTypes || [],
      provider.legend.tokenModifiers || [],
    );
    disposables.push(vscode.languages.registerDocumentSemanticTokensProvider(selector, {
      provideDocumentSemanticTokens: async (document) => {
        const result = await request(connection, 'textDocument/semanticTokens/full', {
          textDocument: { uri: document.uri.toString() },
        });
        return new vscode.SemanticTokens(Uint32Array.from((result && result.data) || []), result && result.resultId);
      },
    }, legend));
  }

  return disposables;
}

async function request(connection, method, params) {
  try {
    return await connection.request(method, params);
  } catch (error) {
    output.appendLine(`${method}: ${error.message}`);
    return null;
  }
}

function publishDiagnostics(params) {
  if (!params || !params.uri) {
    return;
  }
  const uri = vscode.Uri.parse(params.uri);
  diagnostics.set(uri, (params.diagnostics || []).map((item) => {
    const diagnostic = new vscode.Diagnostic(
      toRange(item.range),
      item.message,
      severity(item.severity),
    );
    diagnostic.source = item.source || 'flux';
    if (item.code !== undefined) diagnostic.code = item.code;
    if (Array.isArray(item.relatedInformation)) {
      diagnostic.relatedInformation = item.relatedInformation.map((info) => new vscode.DiagnosticRelatedInformation(
        new vscode.Location(vscode.Uri.parse(info.location.uri), toRange(info.location.range)),
        info.message,
      ));
    }
    return diagnostic;
  }));
}

function severity(value) {
  switch (value) {
    case 2: return vscode.DiagnosticSeverity.Warning;
    case 3: return vscode.DiagnosticSeverity.Information;
    case 4: return vscode.DiagnosticSeverity.Hint;
    default: return vscode.DiagnosticSeverity.Error;
  }
}

function textDocumentPosition(document, position) {
  return {
    textDocument: { uri: document.uri.toString() },
    position: fromPosition(position),
  };
}

function fromPosition(position) {
  return { line: position.line, character: position.character };
}

function toPosition(position) {
  return new vscode.Position(position.line, position.character);
}

function fromRange(range) {
  return { start: fromPosition(range.start), end: fromPosition(range.end) };
}

function toRange(range) {
  return new vscode.Range(toPosition(range.start), toPosition(range.end));
}

function toLocation(value) {
  if (value.uri && value.range) {
    return new vscode.Location(vscode.Uri.parse(value.uri), toRange(value.range));
  }
  if (value.targetUri && value.targetSelectionRange) {
    return new vscode.Location(vscode.Uri.parse(value.targetUri), toRange(value.targetSelectionRange));
  }
  return null;
}

function toTextEdit(edit) {
  return new vscode.TextEdit(toRange(edit.range), edit.newText || '');
}

function toWorkspaceEdit(value) {
  const edit = new vscode.WorkspaceEdit();
  if (value.changes) {
    for (const [uriText, edits] of Object.entries(value.changes)) {
      const uri = vscode.Uri.parse(uriText);
      for (const change of edits) {
        edit.replace(uri, toRange(change.range), change.newText || '');
      }
    }
  }
  if (Array.isArray(value.documentChanges)) {
    for (const change of value.documentChanges) {
      if (!change.textDocument || !Array.isArray(change.edits)) continue;
      const uri = vscode.Uri.parse(change.textDocument.uri);
      for (const textEdit of change.edits) {
        edit.replace(uri, toRange(textEdit.range), textEdit.newText || '');
      }
    }
  }
  return edit;
}

function toCompletionItem(item) {
  const label = typeof item.label === 'string' ? item.label : item.label.label;
  const kind = item.kind ? Math.max(0, item.kind - 1) : vscode.CompletionItemKind.Text;
  const result = new vscode.CompletionItem(label, kind);
  result.detail = item.detail;
  if (item.documentation) result.documentation = toMarkdown(item.documentation);
  if (item.insertText !== undefined) {
    result.insertText = item.insertTextFormat === 2
      ? new vscode.SnippetString(item.insertText)
      : item.insertText;
  }
  if (item.textEdit && item.textEdit.range) {
    result.textEdit = new vscode.TextEdit(toRange(item.textEdit.range), item.textEdit.newText || '');
  }
  result.sortText = item.sortText;
  result.filterText = item.filterText;
  result.preselect = item.preselect;
  return result;
}

function toMarkdown(value) {
  if (value === null || value === undefined) {
    return new vscode.MarkdownString('');
  }
  if (typeof value === 'object' && !Array.isArray(value) && value.kind && value.value !== undefined) {
    return new vscode.MarkdownString(value.value);
  }

  const markdown = new vscode.MarkdownString();
  const parts = Array.isArray(value) ? value : [value];
  for (const part of parts) {
    if (typeof part === 'string') {
      markdown.appendMarkdown(part);
    } else if (part && part.language && part.value !== undefined) {
      markdown.appendCodeblock(part.value, part.language);
    } else if (part && part.value !== undefined) {
      markdown.appendMarkdown(part.value);
    }
  }
  return markdown;
}

function toCodeAction(item) {
  if (item.command && item.title === undefined) {
    return item.command;
  }
  const kind = item.kind ? new vscode.CodeActionKind(item.kind) : vscode.CodeActionKind.Empty;
  const action = new vscode.CodeAction(item.title, kind);
  if (item.edit) action.edit = toWorkspaceEdit(item.edit);
  if (item.command) action.command = item.command;
  if (item.isPreferred !== undefined) action.isPreferred = item.isPreferred;
  return action;
}

function toSignatureHelp(value) {
  const help = new vscode.SignatureHelp();
  help.activeSignature = value.activeSignature || 0;
  help.activeParameter = value.activeParameter || 0;
  help.signatures = (value.signatures || []).map((signature) => {
    const converted = new vscode.SignatureInformation(signature.label, signature.documentation ? toMarkdown(signature.documentation) : undefined);
    converted.parameters = (signature.parameters || []).map((parameter) => new vscode.ParameterInformation(
      parameter.label,
      parameter.documentation ? toMarkdown(parameter.documentation) : undefined,
    ));
    return converted;
  });
  return help;
}

function toInlayHint(value) {
  const label = Array.isArray(value.label)
    ? value.label.map((part) => part.value).join('')
    : value.label;
  const kind = value.kind === 1 ? vscode.InlayHintKind.Type
    : value.kind === 2 ? vscode.InlayHintKind.Parameter
      : undefined;
  const hint = new vscode.InlayHint(toPosition(value.position), label, kind);
  hint.paddingLeft = value.paddingLeft;
  hint.paddingRight = value.paddingRight;
  return hint;
}

function fromDiagnostic(value) {
  return {
    range: fromRange(value.range),
    message: value.message,
    severity: value.severity + 1,
    source: value.source,
    code: value.code,
  };
}

function shellQuote(value) {
  return `'${String(value).replace(/'/g, `'"'"'`)}'`;
}

function targetForCommand() {
  const document = vscode.window.activeTextEditor && vscode.window.activeTextEditor.document;
  const folder = workspaceFolderForDocument(document);
  if (folder) {
    return { folder, target: folder.uri.fsPath };
  }
  if (document && document.languageId === 'flux' && document.uri.scheme === 'file') {
    return { folder: null, target: document.uri.fsPath };
  }
  return { folder: null, target: null };
}

function runFluxCommand(subcommand) {
  const { folder, target } = targetForCommand();
  if (!target) {
    vscode.window.showErrorMessage('Open a Flux file or workspace first.');
    return;
  }
  const executable = resolveFluxExecutable(folder);
  if (!terminal || terminal.exitStatus !== undefined) {
    terminal = vscode.window.createTerminal({ name: 'Flux', cwd: folder ? folder.uri.fsPath : path.dirname(target) });
  }
  terminal.show(true);
  terminal.sendText(`${shellQuote(executable)} ${subcommand} ${shellQuote(target)}`);
}

module.exports = { activate, deactivate };
