'use strict';

const { spawn } = require('child_process');
const { EventEmitter } = require('events');

class LspConnection extends EventEmitter {
  constructor(command, args = [], options = {}) {
    super();
    this.command = command;
    this.args = args;
    this.options = options;
    this.child = null;
    this.buffer = Buffer.alloc(0);
    this.nextId = 1;
    this.pending = new Map();
    this.requestHandlers = new Map();
    this.closed = false;
  }

  start() {
    if (this.child) {
      return;
    }

    this.child = spawn(this.command, this.args, {
      cwd: this.options.cwd,
      env: this.options.env || process.env,
      stdio: ['pipe', 'pipe', 'pipe'],
    });

    this.child.stdout.on('data', (chunk) => this._onData(chunk));
    this.child.stderr.on('data', (chunk) => this.emit('stderr', chunk.toString()));
    this.child.on('error', (error) => this._fail(error));
    this.child.on('exit', (code, signal) => {
      if (!this.closed) {
        this._fail(new Error(`Flux language server exited (${signal || code})`));
      }
      this.emit('exit', code, signal);
    });
  }

  handleRequest(method, handler) {
    this.requestHandlers.set(method, handler);
  }

  request(method, params) {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      try {
        this._send({ jsonrpc: '2.0', id, method, params });
      } catch (error) {
        this.pending.delete(id);
        reject(error);
      }
    });
  }

  notify(method, params) {
    this._send({ jsonrpc: '2.0', method, params });
  }

  async shutdown() {
    if (!this.child || this.closed) {
      return;
    }

    try {
      await Promise.race([
        this.request('shutdown', null),
        new Promise((resolve) => setTimeout(resolve, 750)),
      ]);
    } catch (_) {
      // Best-effort shutdown; the process is terminated below if needed.
    }

    try {
      this.notify('exit', null);
    } catch (_) {
      // stdin may already be closed.
    }

    this.closed = true;
    const child = this.child;
    this.child = null;
    setTimeout(() => {
      if (child.exitCode === null && child.signalCode === null) {
        child.kill();
      }
    }, 250);
  }

  _send(message) {
    if (!this.child || !this.child.stdin.writable) {
      throw new Error('Flux language server is not running');
    }

    const body = JSON.stringify(message);
    const header = `Content-Length: ${Buffer.byteLength(body, 'utf8')}\r\n\r\n`;
    this.child.stdin.write(header, 'ascii');
    this.child.stdin.write(body, 'utf8');
  }

  _onData(chunk) {
    this.buffer = Buffer.concat([this.buffer, chunk]);

    while (true) {
      const headerEnd = this.buffer.indexOf('\r\n\r\n');
      if (headerEnd < 0) {
        return;
      }

      const header = this.buffer.subarray(0, headerEnd).toString('ascii');
      const lengthMatch = header.match(/(?:^|\r\n)Content-Length:\s*(\d+)/i);
      if (!lengthMatch) {
        this._fail(new Error('Flux language server sent an LSP message without Content-Length'));
        return;
      }

      const length = Number(lengthMatch[1]);
      const bodyStart = headerEnd + 4;
      const bodyEnd = bodyStart + length;
      if (this.buffer.length < bodyEnd) {
        return;
      }

      const body = this.buffer.subarray(bodyStart, bodyEnd).toString('utf8');
      this.buffer = this.buffer.subarray(bodyEnd);

      try {
        this._onMessage(JSON.parse(body));
      } catch (error) {
        this._fail(new Error(`Invalid JSON from Flux language server: ${error.message}`));
        return;
      }
    }
  }

  _onMessage(message) {
    if (message.id !== undefined && message.method === undefined) {
      const pending = this.pending.get(message.id);
      if (!pending) {
        return;
      }
      this.pending.delete(message.id);
      if (message.error) {
        const error = new Error(message.error.message || 'Flux language server request failed');
        error.code = message.error.code;
        error.data = message.error.data;
        pending.reject(error);
      } else {
        pending.resolve(message.result);
      }
      return;
    }

    if (message.method && message.id !== undefined) {
      const handler = this.requestHandlers.get(message.method);
      Promise.resolve(handler ? handler(message.params) : null)
        .then((result) => this._send({ jsonrpc: '2.0', id: message.id, result }))
        .catch((error) => this._send({
          jsonrpc: '2.0',
          id: message.id,
          error: { code: -32603, message: error.message || String(error) },
        }));
      return;
    }

    if (message.method) {
      this.emit('notification', message.method, message.params);
      this.emit(`notification:${message.method}`, message.params);
    }
  }

  _fail(error) {
    for (const pending of this.pending.values()) {
      pending.reject(error);
    }
    this.pending.clear();
    this.emit('connectionError', error);
  }
}

module.exports = { LspConnection };
