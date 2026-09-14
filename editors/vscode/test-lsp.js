'use strict';

const fs = require('fs');
const path = require('path');
const { pathToFileURL } = require('url');
const { LspConnection } = require('./lsp-connection');

async function main() {
  const root = path.resolve(__dirname, '../..');
  const sourcePath = path.join(root, 'examples', 'hello_app.flux');
  const uri = pathToFileURL(sourcePath).toString();
  const connection = new LspConnection(path.join(root, 'tools', 'flux'), ['lsp'], { cwd: root });
  let stderr = '';
  connection.on('stderr', (text) => { stderr += text; });
  connection.start();

  try {
    const initialized = await connection.request('initialize', {
      processId: process.pid,
      clientInfo: { name: 'Flux VS Code smoke test', version: '0.1.0' },
      rootUri: pathToFileURL(root).toString(),
      workspaceFolders: [{ uri: pathToFileURL(root).toString(), name: 'flux' }],
      capabilities: {
        general: { positionEncodings: ['utf-16'] },
        workspace: { workspaceFolders: true },
        textDocument: {
          completion: {},
          hover: {},
          definition: {},
          references: {},
          rename: {},
          formatting: {},
          codeAction: {},
          signatureHelp: {},
          inlayHint: {},
          semanticTokens: {
            tokenTypes: [
              'namespace', 'type', 'class', 'enum', 'interface', 'struct', 'typeParameter',
              'parameter', 'variable', 'property', 'enumMember', 'event', 'function', 'method',
              'macro', 'keyword', 'modifier', 'comment', 'string', 'number', 'regexp',
              'operator', 'decorator',
            ],
            tokenModifiers: [],
            formats: ['relative'],
            requests: { full: true, range: false },
          },
        },
      },
    });

    const capabilities = initialized.capabilities || {};
    const required = [
      ['completionProvider', capabilities.completionProvider],
      ['hoverProvider', capabilities.hoverProvider],
      ['definitionProvider', capabilities.definitionProvider],
      ['referencesProvider', capabilities.referencesProvider],
      ['renameProvider', capabilities.renameProvider],
      ['documentFormattingProvider', capabilities.documentFormattingProvider],
      ['codeActionProvider', capabilities.codeActionProvider],
      ['signatureHelpProvider', capabilities.signatureHelpProvider],
      ['inlayHintProvider', capabilities.inlayHintProvider],
      ['semanticTokensProvider', capabilities.semanticTokensProvider],
    ];
    const missing = required.filter(([, value]) => !value).map(([name]) => name);
    if (missing.length) {
      throw new Error(`missing LSP capabilities: ${missing.join(', ')}`);
    }

    connection.notify('initialized', {});
    connection.notify('textDocument/didOpen', {
      textDocument: {
        uri,
        languageId: 'flux',
        version: 1,
        text: fs.readFileSync(sourcePath, 'utf8'),
      },
    });

    const completion = await connection.request('textDocument/completion', {
      textDocument: { uri },
      position: { line: 0, character: 0 },
    });
    const completionItems = Array.isArray(completion) ? completion : completion && completion.items;
    if (!Array.isArray(completionItems) || completionItems.length === 0) {
      throw new Error('completion returned no items');
    }

    const semanticTokens = await connection.request('textDocument/semanticTokens/full', {
      textDocument: { uri },
    });
    if (!semanticTokens || !Array.isArray(semanticTokens.data) || semanticTokens.data.length === 0) {
      throw new Error('semantic tokens returned no data');
    }

    connection.notify('textDocument/didClose', { textDocument: { uri } });
    process.stdout.write(`ok: Flux LSP (${completionItems.length} completions, ${semanticTokens.data.length / 5} semantic tokens)\n`);
  } finally {
    await connection.shutdown();
  }

  if (stderr.trim()) {
    process.stderr.write(stderr);
  }
}

main().catch((error) => {
  process.stderr.write(`${error.stack || error.message}\n`);
  process.exitCode = 1;
});
