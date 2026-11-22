// Speed comparison inside VS Code: the C# extension's debugger (vsdbg, the debugger of Visual Studio's family) against
// netdbg (through the netdbg extension), over the same requests sent with DebugSession.customRequest.
// Both go through VS Code the same way; VS Code's own requests after each stop are let finish before measuring.
// vsdbg is x64 only, so the targets are the x64 builds. Run by tests/bench/vscode.ps1 (VS Code's --extensionTestsPath).
//
// Environment: BENCH_ROOT (the netdbg repository), BENCH_OUT (result JSON), BENCH_RUNS (default 5),
// BENCH_REPEAT (default 20), BENCH_ENGINES (default "vsdbg,new").

const vscode = require('vscode');
const cp = require('child_process');
const fs = require('fs');
const path = require('path');

const root = process.env.BENCH_ROOT;
const source = path.join(root, 'tests', 'apps', 'Console', 'Program.cs');
const runs = Number(process.env.BENCH_RUNS || 5);
const repeat = Number(process.env.BENCH_REPEAT || 20);
const engines = (process.env.BENCH_ENGINES || 'vsdbg,new').split(',');

const targets = [
    { label: 'net10 x64', program: path.join(root, 'target', 'apps', 'x64', 'Console', 'SmokeConsole.exe'), vsdbgType: 'coreclr' },
    { label: 'net48 x64', program: path.join(root, 'target', 'apps', 'x64', 'Console48', 'SmokeConsole48.exe'), vsdbgType: 'clr' },
];

/** The process names of each engine (for its memory). */
const engineProcesses = { vsdbg: ['vsdbg-ui', 'vsdbg'], new: ['netdbg'] };

function markerLine(marker) {
    const lines = fs.readFileSync(source, 'utf8').split(/\r?\n/);
    const index = lines.findIndex((l) => new RegExp(`// @${marker}(?![\\w-])`).test(l));
    if (index < 0) {
        throw new Error(`marker @${marker} not found`);
    }
    return index + 1;
}

// Every message between VS Code and the adapters, per session: events to wait for, and the time of the last message.
const sessions = new Map();
function state(id) {
    if (!sessions.has(id)) {
        sessions.set(id, { events: [], waiters: [], last: performance.now() });
    }
    return sessions.get(id);
}
vscode.debug.registerDebugAdapterTrackerFactory('*', {
    createDebugAdapterTracker(session) {
        const s = state(session.id);
        return {
            onWillReceiveMessage: () => {
                s.last = performance.now();
            },
            onDidSendMessage: (m) => {
                s.last = performance.now();
                if (m.type !== 'event') {
                    return;
                }
                const waiter = s.waiters.findIndex((w) => w.name === m.event);
                if (waiter >= 0) {
                    s.waiters.splice(waiter, 1)[0].resolve(m);
                } else {
                    s.events.push(m);
                }
            },
            onError: (e) => {
                fs.appendFileSync(process.env.BENCH_OUT + '.log', `adapter error (${session.type}): ${e && e.message}\n`);
            },
            onExit: (code, signal) => {
                if (code) {
                    fs.appendFileSync(process.env.BENCH_OUT + '.log', `adapter exited (${session.type}): code ${code} ${signal ?? ''}\n`);
                }
                for (const w of s.waiters.splice(0)) {
                    w.resolve({ event: 'exit' });
                }
            },
        };
    },
});

function waitEvent(sessionId, name, timeout = 120_000) {
    const s = state(sessionId);
    const index = s.events.findIndex((e) => e.event === name);
    if (index >= 0) {
        return Promise.resolve(s.events.splice(index, 1)[0]);
    }
    return new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error(`event ${name} timed out`)), timeout);
        s.waiters.push({ name, resolve: (m) => (clearTimeout(timer), resolve(m)) });
    });
}

/** Waits until no DAP message has passed for `quietMs` (VS Code's own requests after a stop have finished). */
async function settle(sessionId, quietMs = 200) {
    const s = state(sessionId);
    while (performance.now() - s.last < quietMs) {
        await new Promise((r) => setTimeout(r, 20));
    }
}

const now = () => performance.now();
async function time(count, action) {
    const started = now();
    for (let i = 0; i < count; i++) {
        await action();
    }
    return (now() - started) / count;
}

function configuration(engine, target) {
    const common = { name: `bench ${engine}`, request: 'launch', program: target.program, args: [], cwd: path.dirname(target.program), stopAtEntry: false, justMyCode: true };
    return engine === 'vsdbg' ? { ...common, type: target.vsdbgType, console: 'internalConsole' } : { ...common, type: 'netdbg' };
}

function peakMemory(engine) {
    try {
        const names = engineProcesses[engine].map((n) => `'${n}'`).join(',');
        const out = cp.execFileSync('powershell.exe', ['-NoProfile', '-Command', `(Get-Process -Name ${names} -ErrorAction SilentlyContinue | Measure-Object PeakWorkingSet64 -Sum).Sum`], { encoding: 'utf8' });
        return Math.round(Number(out.trim()) / 1024 / 102.4) / 10;
    } catch {
        return undefined;
    }
}

/** Starts a session stopped at the breakpoints; returns the session, the stopped thread and the time to the stop. */
async function startStopped(engine, target, breakpoints) {
    vscode.debug.removeBreakpoints(vscode.debug.breakpoints);
    vscode.debug.addBreakpoints(breakpoints.map((b) => new vscode.SourceBreakpoint(new vscode.Location(vscode.Uri.file(source), new vscode.Position(b.line - 1, 0)), true, b.condition)));
    let started = now();
    const sessionStarted = new Promise((resolve) => {
        const d = vscode.debug.onDidStartDebugSession((s) => {
            d.dispose();
            resolve(s);
        });
    });
    // The C# extension registers its debug adapter a while after it is activated (it locates a .NET runtime first):
    // until then a session does not start. Retry for up to 20 s, then give up so that the reason is looked into.
    const waitStarted = now();
    let attempts = 0;
    // (Before that, startDebugging may also throw: "Couldn't find a debug adapter descriptor".)
    const tryStart = () => Promise.resolve(vscode.debug.startDebugging(undefined, configuration(engine, target))).catch(() => false);
    while (!(await tryStart())) {
        if (now() - waitStarted > 20_000) {
            throw new Error(`${engine} ${target.label}: the session did not start in 20 s (${attempts + 1} attempts)`);
        }
        attempts++;
        await new Promise((r) => setTimeout(r, 1000));
        started = now();
    }
    if (attempts) {
        fs.appendFileSync(process.env.BENCH_OUT + '.log', `${engine} ${target.label}: started after ${attempts} retries (${Math.round(now() - waitStarted)} ms)\n`);
    }
    const session = await sessionStarted;
    const stop = await waitEvent(session.id, 'stopped');
    const elapsed = now() - started;
    await settle(session.id);
    return { session, threadId: stop.body.threadId, elapsed };
}

async function end(session) {
    const ended = new Promise((resolve) => {
        const d = vscode.debug.onDidTerminateDebugSession((s) => {
            if (s.id === session.id) {
                d.dispose();
                resolve();
            }
        });
    });
    // The session may be ending already (the program ran to its end): stopping it then answers "Canceled".
    await Promise.resolve(vscode.debug.stopDebugging(session)).catch(() => undefined);
    await Promise.race([ended, new Promise((r) => setTimeout(r, 15_000))]);
    sessions.delete(session.id);
}

async function inspectSession(engine, target) {
    const result = {};
    const { session, threadId, elapsed } = await startStopped(engine, target, [{ line: markerLine('values') }]);
    result['start → first stop'] = elapsed;
    try {
        const frames = (await session.customRequest('stackTrace', { threadId, startFrame: 0, levels: 20 })).stackFrames;
        const frameId = frames[0].id;
        result['stackTrace'] = await time(repeat, () => session.customRequest('stackTrace', { threadId, startFrame: 0, levels: 20 }));
        result['scopes + variables (locals)'] = await time(repeat, async () => {
            const scopes = (await session.customRequest('scopes', { frameId })).scopes;
            await session.customRequest('variables', { variablesReference: scopes[0].variablesReference });
        });
        const scopes = (await session.customRequest('scopes', { frameId })).scopes;
        const locals = (await session.customRequest('variables', { variablesReference: scopes[0].variablesReference })).variables;
        const expandable = locals.filter((v) => v.variablesReference > 0);
        result['locals with children (count)'] = expandable.length;
        result['expand every local with children'] = await time(Math.max(1, Math.round(repeat / 4)), async () => {
            for (const v of expandable) {
                await session.customRequest('variables', { variablesReference: v.variablesReference });
            }
        });
        for (const [label, expression] of [
            ['evaluate: arithmetic (sum + id)', 'sum + id'],
            ['evaluate: property (names.Count)', 'names.Count'],
            ['evaluate: method call (label.ToUpper())', 'label.ToUpper()'],
        ]) {
            try {
                await session.customRequest('evaluate', { expression, frameId, context: 'watch' });
                result[label] = await time(repeat, () => session.customRequest('evaluate', { expression, frameId, context: 'watch' }));
            } catch {
                result[label] = null;
            }
        }
        result['peak memory (MB)'] = peakMemory(engine);
        const ended = now();
        await session.customRequest('continue', { threadId });
        await waitEvent(session.id, 'terminated', 30_000);
        result['continue → end of program'] = now() - ended;
    } finally {
        await end(session);
    }
    return result;
}

async function stepSession(engine, target) {
    const { session, threadId } = await startStopped(engine, target, [{ line: markerLine('copy') }]);
    try {
        // Each step is timed up to its stop; VS Code's own requests after it are let finish outside the time.
        let total = 0;
        for (let i = 0; i < 5; i++) {
            const started = now();
            const stopped = waitEvent(session.id, 'stopped');
            await session.customRequest('next', { threadId });
            await stopped;
            total += now() - started;
            await settle(session.id);
        }
        return { 'next (per step)': total / 5 };
    } finally {
        await end(session);
    }
}

async function conditionSession(engine, target) {
    const { session, elapsed } = await startStopped(engine, target, [{ line: markerLine('loop'), condition: 'i == 2' }]);
    await end(session);
    return { 'start → stop at a conditional breakpoint (i == 2)': elapsed };
}

const median = (values) => {
    const sorted = [...values].sort((a, b) => a - b);
    const middle = Math.floor(sorted.length / 2);
    return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
};

async function run() {
    const log = (line) => fs.appendFileSync(process.env.BENCH_OUT + '.log', line + '\n');
    try {
        await measure(log);
    } catch (e) {
        log(`failed: ${e && e.stack ? e.stack : e}`);
        throw e;
    }
}

/** The C# extension installs its packages and finds a .NET runtime on first use; the debug types exist after that. */
async function ready(log) {
    for (const id of ['ms-dotnettools.csharp', 'takahino.netdbg']) {
        const extension = vscode.extensions.getExtension(id);
        if (!extension) {
            throw new Error(`extension ${id} is not installed`);
        }
        const started = now();
        await extension.activate();
        log(`activated ${id} in ${Math.round(now() - started)} ms`);
    }
}

async function measure(log) {
    const results = {};
    await ready(log);
    for (const target of targets) {
        for (const engine of engines) {
            log(`${target.label} ${engine}: warm-up`);
            await inspectSession(engine, target);
            const all = [];
            for (let i = 0; i < runs; i++) {
                all.push({ ...(await inspectSession(engine, target)), ...(await stepSession(engine, target)), ...(await conditionSession(engine, target)) });
                log(`${target.label} ${engine}: run ${i + 1}/${runs}`);
            }
            results[target.label] ??= {};
            results[target.label][engine] = Object.fromEntries(Object.keys(all[0]).map((k) => {
                const values = all.map((r) => r[k]).filter((v) => typeof v === 'number');
                return [k, values.length ? Math.round(median(values) * 10) / 10 : null];
            }));
        }
    }
    vscode.debug.removeBreakpoints(vscode.debug.breakpoints);
    fs.writeFileSync(process.env.BENCH_OUT, JSON.stringify({ date: new Date().toISOString(), runs, repeat, vscode: vscode.version, results }, null, 2));
}

module.exports = { run };
