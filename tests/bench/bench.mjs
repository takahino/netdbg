// Speed comparison of two debug engines over the same DAP traffic (netdbg against netcoredbg).
// The client is a minimal DAP client in Node (its own parsing cost is small and the same for both engines).
//
//   node tests/bench/bench.mjs --engine new=<netdbg.exe> --engine old=<netcoredbg.exe> [--runs 5] [--repeat 20]
//                              [--target net10=<exe>] [--target net48=<exe>] [--out target/bench/result.json] [--log <dir>]
// --log: netdbg writes its log (time, function evaluations per request) to <dir>/<engine>-<target>-<n>.log.
//
// netcoredbg needs, for a .NET Framework target, NETCOREDBG_DESKTOP_CLR=1 and NETCOREDBG_HOST_CORECLR (an x86 coreclr.dll);
// the script sets them for an engine named netcoredbg.exe (as the extension did). Each scenario runs in a fresh session,
// `--runs` times; the median is reported. Times are wall-clock milliseconds as the client sees them.

import { spawn, execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
const source = join(root, 'tests', 'apps', 'Console', 'Program.cs');

function parseArgs() {
    const args = process.argv.slice(2);
    const options = { engines: [], targets: [], runs: 5, repeat: 20, out: join(root, 'target', 'bench', 'result.json'), log: undefined };
    for (let i = 0; i < args.length; i++) {
        const [name, value] = [args[i], args[i + 1]];
        if (name === '--engine' || name === '--target') {
            const [label, path] = value.split(/=(.*)/s);
            (name === '--engine' ? options.engines : options.targets).push({ label, path });
            i++;
        } else if (name === '--runs' || name === '--repeat') {
            options[name.slice(2)] = Number(value);
            i++;
        } else if (name === '--log') {
            options.log = resolve(value);
            i++;
        } else if (name === '--out') {
            options.out = resolve(value);
            i++;
        }
    }
    if (!options.targets.length) {
        options.targets.push({ label: 'net10', path: join(root, 'target', 'apps', 'x86', 'Console', 'SmokeConsole.exe') });
        options.targets.push({ label: 'net48', path: join(root, 'target', 'apps', 'x86', 'Console48', 'SmokeConsole48.exe') });
    }
    return options;
}

function markerLine(marker) {
    const lines = readFileSync(source, 'utf8').split(/\r?\n/);
    const index = lines.findIndex((l) => new RegExp(`// @${marker}(?![\\w-])`).test(l));
    if (index < 0) {
        throw new Error(`marker @${marker} not found`);
    }
    return index + 1;
}

/** The newest x86 coreclr.dll (what the extension handed netcoredbg for .NET Framework targets). */
function x86CoreClr() {
    const shared = join(process.env['ProgramFiles(x86)'] ?? 'C:\\Program Files (x86)', 'dotnet', 'shared', 'Microsoft.NETCore.App');
    const versions = existsSync(shared) ? readdirSync(shared).filter((v) => existsSync(join(shared, v, 'coreclr.dll'))) : [];
    versions.sort((a, b) => b.localeCompare(a, undefined, { numeric: true }));
    return versions.length ? join(shared, versions[0], 'coreclr.dll') : undefined;
}

let logCount = 0;

class Dap {
    constructor(exe, framework, logName) {
        const env = { ...process.env };
        if (/netcoredbg\.exe$/i.test(exe)) {
            const host = x86CoreClr();
            if (host) {
                env.NETCOREDBG_HOST_CORECLR = host;
            }
            if (framework) {
                env.NETCOREDBG_DESKTOP_CLR = '1';
            }
        }
        const args = ['--interpreter=vscode'];
        if (logName && /netdbg\.exe$/i.test(exe)) {
            args.push(`--log=${logName}-${++logCount}.log`);
        }
        this.process = spawn(exe, args, { cwd: dirname(exe), env, stdio: ['pipe', 'pipe', 'ignore'] });
        this.seq = 1;
        this.buffer = Buffer.alloc(0);
        this.pending = new Map();
        this.events = [];
        this.waiters = [];
        this.process.stdout.on('data', (chunk) => this.receive(chunk));
    }

    receive(chunk) {
        this.buffer = Buffer.concat([this.buffer, chunk]);
        for (;;) {
            const end = this.buffer.indexOf('\r\n\r\n');
            if (end < 0) {
                return;
            }
            const length = Number(/Content-Length: (\d+)/i.exec(this.buffer.subarray(0, end).toString('ascii'))[1]);
            if (this.buffer.length < end + 4 + length) {
                return;
            }
            const message = JSON.parse(this.buffer.subarray(end + 4, end + 4 + length).toString('utf8'));
            this.buffer = this.buffer.subarray(end + 4 + length);
            if (message.type === 'response') {
                this.pending.get(message.request_seq)?.(message);
                this.pending.delete(message.request_seq);
            } else if (message.type === 'event') {
                const waiter = this.waiters.findIndex((w) => w.match(message));
                if (waiter >= 0) {
                    this.waiters.splice(waiter, 1)[0].resolve(message);
                } else {
                    this.events.push(message);
                }
            }
        }
    }

    request(command, args = {}, timeout = 60_000) {
        const seq = this.seq++;
        const body = Buffer.from(JSON.stringify({ seq, type: 'request', command, arguments: args }), 'utf8');
        this.process.stdin.write(Buffer.concat([Buffer.from(`Content-Length: ${body.length}\r\n\r\n`, 'ascii'), body]));
        return new Promise((resolve, reject) => {
            const timer = setTimeout(() => reject(new Error(`${command} timed out`)), timeout);
            this.pending.set(seq, (m) => {
                clearTimeout(timer);
                resolve(m);
            });
        });
    }

    event(name, match = () => true, timeout = 60_000) {
        const index = this.events.findIndex((e) => e.event === name && match(e));
        if (index >= 0) {
            return Promise.resolve(this.events.splice(index, 1)[0]);
        }
        return new Promise((resolve, reject) => {
            const timer = setTimeout(() => reject(new Error(`event ${name} timed out`)), timeout);
            this.waiters.push({ match: (e) => e.event === name && match(e), resolve: (e) => (clearTimeout(timer), resolve(e)) });
        });
    }

    /** Peak working set of the engine process, in MB (read with PowerShell, outside the timings). */
    peakMemory() {
        try {
            const out = execFileSync('powershell.exe', ['-NoProfile', '-Command', `(Get-Process -Id ${this.process.pid}).PeakWorkingSet64`], { encoding: 'utf8' });
            return Math.round(Number(out.trim()) / 1024 / 102.4) / 10;
        } catch {
            return undefined;
        }
    }

    async close() {
        try {
            await this.request('disconnect', { terminateDebuggee: true }, 10_000);
        } catch {
            // the engine may already be gone
        }
        this.process.kill();
    }
}

const now = () => performance.now();
const median = (values) => {
    const sorted = [...values].sort((a, b) => a - b);
    const middle = Math.floor(sorted.length / 2);
    return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
};

/** Starts a session stopped at `breakpoints` (lines, optional conditions); returns the session, its thread and the time to the stop. */
async function startStopped(engine, target, breakpoints) {
    const dap = new Dap(engine.path, target.label === 'net48', options.log && join(options.log, `${engine.label}-${target.label}`));
    const started = now();
    await dap.request('initialize', { adapterID: 'coreclr', linesStartAt1: true, columnsStartAt1: true, pathFormat: 'path', supportsVariableType: true });
    for (const [command, args] of [
        ['launch', { program: target.path, args: [], cwd: dirname(target.path), stopAtEntry: false, justMyCode: true }],
        ['setBreakpoints', { source: { path: source }, breakpoints }],
        ['configurationDone', {}],
    ]) {
        const response = await dap.request(command, args);
        if (!response.success) {
            await dap.close();
            throw new Error(`${engine.label} ${target.label}: ${command} failed: ${response.message}`);
        }
    }
    const stop = await dap.event('stopped');
    return { dap, threadId: stop.body.threadId, elapsed: now() - started };
}

async function time(repeat, action) {
    const started = now();
    for (let i = 0; i < repeat; i++) {
        await action();
    }
    return (now() - started) / repeat;
}

/** One session at @values: start, the inspection requests, the expressions, the end. Returns ms per scenario. */
async function inspectSession(engine, target, repeat) {
    const result = {};
    const { dap, threadId, elapsed } = await startStopped(engine, target, [{ line: markerLine('values') }]);
    result['start → first stop'] = elapsed;
    try {
        const frames = (await dap.request('stackTrace', { threadId, startFrame: 0, levels: 20 })).body.stackFrames;
        const frameId = frames[0].id;
        result['stackTrace'] = await time(repeat, () => dap.request('stackTrace', { threadId, startFrame: 0, levels: 20 }));
        result['scopes + variables (locals)'] = await time(repeat, async () => {
            const scopes = (await dap.request('scopes', { frameId })).body.scopes;
            await dap.request('variables', { variablesReference: scopes[0].variablesReference });
        });
        // Every local with children expanded once (as when the Variables view opens each of them).
        const scopes = (await dap.request('scopes', { frameId })).body.scopes;
        const locals = (await dap.request('variables', { variablesReference: scopes[0].variablesReference })).body.variables;
        const expandable = locals.filter((v) => v.variablesReference > 0);
        result['locals with children (count)'] = expandable.length;
        result['expand every local with children'] = await time(Math.max(1, Math.round(repeat / 4)), async () => {
            for (const v of expandable) {
                await dap.request('variables', { variablesReference: v.variablesReference });
            }
        });
        for (const [label, expression] of [
            ['evaluate: arithmetic (sum + id)', 'sum + id'],
            ['evaluate: property (names.Count)', 'names.Count'],
            ['evaluate: method call (label.ToUpper())', 'label.ToUpper()'],
        ]) {
            const first = await dap.request('evaluate', { expression, frameId, context: 'watch' });
            result[label] = first.success ? await time(repeat, () => dap.request('evaluate', { expression, frameId, context: 'watch' })) : null;
        }
        result['peak memory (MB)'] = dap.peakMemory();
        const ended = now();
        await dap.request('continue', { threadId });
        await dap.event('terminated', () => true, 30_000);
        result['continue → end of program'] = now() - ended;
    } finally {
        await dap.close();
    }
    return result;
}

/** Steps over the lines of Main from @copy (five `next`s). Returns ms per step. */
async function stepSession(engine, target) {
    const { dap, threadId } = await startStopped(engine, target, [{ line: markerLine('copy') }]);
    try {
        return {
            'next (per step)': await time(5, async () => {
                await dap.request('next', { threadId });
                await dap.event('stopped');
            }),
        };
    } finally {
        await dap.close();
    }
}

/** A conditional breakpoint that is false 2 times and true once (@loop, i == 2): time to the stop, start included. */
async function conditionSession(engine, target) {
    const { dap, elapsed } = await startStopped(engine, target, [{ line: markerLine('loop'), condition: 'i == 2' }]);
    await dap.close();
    return { 'start → stop at a conditional breakpoint (i == 2)': elapsed };
}

const options = parseArgs();

async function main() {
    const results = {};
    for (const target of options.targets) {
        for (const engine of options.engines) {
            // One session first that is not counted: the engine's and the program's files come into the disk cache.
            await inspectSession(engine, target, 1);
            const runs = [];
            for (let run = 0; run < options.runs; run++) {
                const merged = { ...(await inspectSession(engine, target, options.repeat)), ...(await stepSession(engine, target)), ...(await conditionSession(engine, target)) };
                runs.push(merged);
                process.stderr.write(`${target.label} ${engine.label} run ${run + 1}/${options.runs}\n`);
            }
            const keys = Object.keys(runs[0]);
            results[target.label] ??= {};
            results[target.label][engine.label] = Object.fromEntries(keys.map((k) => {
                const values = runs.map((r) => r[k]).filter((v) => typeof v === 'number');
                return [k, values.length ? Math.round(median(values) * 10) / 10 : null];
            }));
        }
    }
    mkdirSync(dirname(options.out), { recursive: true });
    writeFileSync(options.out, JSON.stringify({ date: new Date().toISOString(), runs: options.runs, repeat: options.repeat, results }, null, 2));
    for (const [target, byEngine] of Object.entries(results)) {
        const engines = Object.keys(byEngine);
        console.log(`\n## ${target}\n`);
        console.log(`| scenario | ${engines.join(' | ')} | ratio (${engines[0]} / ${engines[1] ?? engines[0]}) |`);
        console.log(`|---|${engines.map(() => '---:').join('|')}|---:|`);
        for (const key of Object.keys(byEngine[engines[0]])) {
            const values = engines.map((e) => byEngine[e][key]);
            const ratio = values.length > 1 && values[0] != null && values[1] ? (values[0] / values[1]).toFixed(2) : '';
            console.log(`| ${key} | ${values.map((v) => (v == null ? 'n/a' : v)).join(' | ')} | ${ratio} |`);
        }
    }
}

main().catch((e) => {
    console.error(e);
    process.exit(1);
});
