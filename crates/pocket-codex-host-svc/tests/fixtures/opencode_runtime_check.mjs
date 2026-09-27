// Characterize the pinned external server without invoking a model.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { access, mkdir, mkdtemp, realpath, writeFile } from 'node:fs/promises';
import net from 'node:net';
import { tmpdir } from 'node:os';
import { isAbsolute, join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';

const binary = process.env.PCX_TEST_OPENCODE_BINARY;
if (!binary || !isAbsolute(binary)) {
  console.error('PCX_TEST_OPENCODE_BINARY must be an explicit absolute path to OpenCode 1.18.32.');
  process.exitCode = 1;
} else {
  await run().catch(() => {
    console.error('OpenCode fixture setup or cleanup failed; no server error content is printed.');
    process.exitCode = 1;
  });
}

async function run() {
  await access(binary);
  const root = await mkdtemp(join(tmpdir(), 'pocket-opencode-fixture-'));
  const directory = join(root, 'project space \u4e2d\u6587');
  const otherDirectory = join(root, 'other');
  const env = {
    LANG: 'en_US.UTF-8', TMPDIR: root, TMP: root, TEMP: root,
    OPENCODE_DISABLE_AUTOUPDATE: 'true', OPENCODE_DISABLE_MODELS_FETCH: 'true',
    OPENCODE_DISABLE_PROJECT_CONFIG: 'true', OPENCODE_EXPERIMENTAL_DISABLE_FILEWATCHER: 'true',
    OPENCODE_CONFIG_CONTENT: '{"plugin":[],"model":"fixture/unused"}',
  };
  if (process.platform === 'win32') {
    const systemRoot = process.env.SystemRoot || process.env.SYSTEMROOT;
    assert.ok(systemRoot && isAbsolute(systemRoot), 'Windows SystemRoot is required');
    env.SystemRoot = systemRoot;
    env.WINDIR = systemRoot;
    env.PATH = `${join(systemRoot, 'System32')};${systemRoot}`;
  } else {
    env.PATH = '/usr/bin:/bin:/usr/sbin:/sbin';
  }
  for (const [key, name] of Object.entries({
    HOME: 'home', USERPROFILE: 'home', APPDATA: 'appdata', LOCALAPPDATA: 'localappdata',
    XDG_CONFIG_HOME: 'config', XDG_DATA_HOME: 'data', XDG_CACHE_HOME: 'cache',
    XDG_STATE_HOME: 'state', OPENCODE_CONFIG_DIR: 'opencode-config',
  })) {
    env[key] = join(root, name);
    await mkdir(env[key], { recursive: true, mode: 0o700 });
  }
  await mkdir(directory, { mode: 0o700 });
  await mkdir(otherDirectory, { mode: 0o700 });
  const canonicalDirectory = await realpath(directory);
  env.OPENCODE_SERVER_PASSWORD = randomBytes(32).toString('hex');
  const basic = Buffer.from(`opencode:${env.OPENCODE_SERVER_PASSWORD}`).toString('base64');
  const authorization = `Basic ${basic}`;
  const redact = value => String(value).replaceAll(env.OPENCODE_SERVER_PASSWORD, '[REDACTED]').replaceAll(basic, '[REDACTED]');
  const probe = net.createServer();
  const port = await new Promise((resolve, reject) => {
    probe.once('error', reject);
    probe.listen(0, '127.0.0.1', () => {
      const selected = probe.address().port;
      probe.close(error => error ? reject(error) : resolve(selected));
    });
  });
  const origin = `http://127.0.0.1:${port}`;
  const child = spawn(binary, ['serve', '--hostname', '127.0.0.1', '--port', String(port), '--no-mdns'], {
    cwd: directory, env, stdio: ['ignore', 'pipe', 'pipe'],
  });
  let spawnFailed = false;
  let closed = false;
  child.on('error', () => { spawnFailed = true; });
  const childClosed = new Promise(resolve => child.once('close', () => { closed = true; resolve(); }));
  let logs = '';
  let logOverflow = false;
  for (const output of [child.stdout, child.stderr]) {
    output.setEncoding('utf8');
    output.on('data', chunk => {
      if (logs.length + chunk.length <= 4 * 1024 * 1024) logs += chunk;
      else logOverflow = true;
    });
  }
  const lifecycle = new AbortController();
  const interrupt = () => lifecycle.abort();
  process.on('SIGINT', interrupt);
  process.on('SIGTERM', interrupt);
  const results = { binary, root, origin, ownedPid: child.pid, passed: false, checks: [], events: [] };
  let stage = 'startup';
  let stream;
  let streamTimer;
  let eventReader;
  let eventTask;
  let streamFailed = false;
  let rawEvents = '';
  const check = (name, value) => {
    results.checks.push({ name, value });
    console.log(redact(JSON.stringify({ name, value })));
  };
  const query = '?directory=' + encodeURIComponent(directory);
  const request = (path, options = {}) => fetch(origin + path, {
    ...options, headers: { authorization, ...options.headers }, redirect: 'error',
    signal: AbortSignal.any([lifecycle.signal, AbortSignal.timeout(15000)]),
  });
  const json = async (path, options, status = 200) => {
    const response = await request(path, options);
    assert.equal(response.status, status);
    return { response, body: await response.json() };
  };
  const post = body => ({ method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) });
  try {
    let ready = false;
    for (let attempt = 0; attempt < 120; attempt++) {
      assert.ok(!spawnFailed && !closed, 'Fixture process failed before readiness');
      lifecycle.signal.throwIfAborted();
      try { if ((await request('/global/health')).ok) { ready = true; break; } } catch {}
      await delay(250, undefined, { signal: lifecycle.signal });
    }
    assert.ok(ready, 'Fixture readiness timed out');
    stage = 'health and authentication';
    const health = await json('/global/health');
    assert.deepEqual(health.body, { healthy: true, version: '1.18.32' });
    check('health', { status: health.response.status, body: health.body });
    for (const [label, value] of [['missing', undefined], ['wrong', 'Basic ' + Buffer.from('opencode:wrong').toString('base64')]]) {
      const response = await fetch(origin + '/global/health', {
        headers: value ? { authorization: value } : {}, redirect: 'error',
        signal: AbortSignal.any([lifecycle.signal, AbortSignal.timeout(15000)]),
      });
      assert.equal(response.status, 401);
      assert.equal(response.headers.get('www-authenticate'), 'Basic realm="Secure Area"');
      await response.body?.cancel();
      check('authentication ' + label, { status: response.status });
    }
    stage = 'OpenAPI contract';
    const { body: schema } = await json('/doc');
    assert.equal(schema.openapi, '3.1.0');
    const required = {
      '/global/health': ['get'], '/session': ['get', 'post'], '/session/status': ['get'],
      '/session/{sessionID}': ['get'], '/session/{sessionID}/message': ['get'],
      '/session/{sessionID}/message/{messageID}': ['get'],
      '/session/{sessionID}/prompt_async': ['post'], '/session/{sessionID}/abort': ['post'],
      '/event': ['get'], '/permission': ['get'], '/permission/{requestID}/reply': ['post'],
      '/question': ['get'], '/question/{requestID}/reply': ['post'], '/question/{requestID}/reject': ['post'],
    };
    for (const [path, methods] of Object.entries(required)) {
      for (const method of methods) assert.ok(schema.paths?.[path]?.[method], `Missing ${method} ${path}`);
    }
    await writeFile(join(root, 'openapi.json'), redact(JSON.stringify(schema, null, 2)), { mode: 0o600 });
    check('doc', { openapi: schema.openapi, verifiedRoutes: required });
    stage = 'session and SSE connection';
    assert.deepEqual((await json('/session' + query + '&limit=100')).body, []);
    check('initial sessions', { status: 200, count: 0 });
    stream = new AbortController();
    lifecycle.signal.addEventListener('abort', () => stream.abort(), { once: true });
    streamTimer = setTimeout(() => stream.abort(), 60000);
    const eventResponse = await fetch(origin + '/event' + query, {
      headers: { authorization }, redirect: 'error',
      signal: stream.signal,
    });
    assert.equal(eventResponse.status, 200);
    assert.equal(eventResponse.headers.get('content-type'), 'text/event-stream');
    const decoder = new TextDecoder('utf-8', { fatal: true });
    eventReader = eventResponse.body.getReader();
    eventTask = (async () => {
      try {
        for (;;) {
          const { value, done } = await eventReader.read();
          if (done) break;
          rawEvents += decoder.decode(value, { stream: true });
          assert.ok(rawEvents.length <= 8 * 1024 * 1024, 'Fixture SSE collection exceeded its limit');
        }
        rawEvents += decoder.decode();
        if (!stream.signal.aborted) streamFailed = true;
      } catch {
        if (!stream.signal.aborted) streamFailed = true;
      }
    })();
    check('SSE connection', { status: 200, contentType: 'text/event-stream' });
    const { body: session } = await json('/session' + query, post({ title: 'Pocket isolated API fixture' }));
    assert.match(session.id, /^ses_/);
    assert.equal(session.directory, canonicalDirectory);
    assert.equal(session.title, 'Pocket isolated API fixture');
    assert.equal(session.version, '1.18.32');
    check('create session', { status: 200, session });
    const messagePath = `/session/${session.id}/message` + query;
    const empty = await json(messagePath + '&limit=20');
    assert.deepEqual(empty.body, []);
    assert.equal(empty.response.headers.get('x-next-cursor'), null);
    check('empty bounded history', { status: 200, nextCursor: null });
    stage = 'noReply messages and history pagination';
    for (let i = 1; i <= 3; i++) {
      const response = await request(`/session/${session.id}/prompt_async` + query, post({
        noReply: true, model: { providerID: 'fixture', modelID: 'unused' },
        parts: [{ type: 'text', text: `Fixture message ${i}` }],
      }));
      assert.equal(response.status, 204);
      let settled = false;
      for (let retry = 0; retry < 40; retry++) {
        const { body: history } = await json(messagePath + '&limit=20');
        assert.ok(history.every(message => message.info.role === 'user'));
        if (history.length === i && history.at(-1)?.parts.some(part => part.text === `Fixture message ${i}`)) {
          settled = true;
          break;
        }
        await delay(100, undefined, { signal: lifecycle.signal });
      }
      assert.ok(settled, 'Message parts failed to settle');
    }
    check('noReply prompts', { status: 204, count: 3, userMessagesOnly: true });
    const { response: tail, body: messages } = await json(messagePath + '&limit=2');
    assert.equal(messages.length, 2);
    assert.equal(messages[0].parts[0].text, 'Fixture message 2');
    assert.equal(messages[1].parts[0].text, 'Fixture message 3');
    const cursor = tail.headers.get('x-next-cursor');
    assert.ok(cursor);
    const link = tail.headers.get('link');
    assert.ok(link?.endsWith('; rel="next"'));
    const linkedUrl = new URL(link.slice(1, link.indexOf('>')));
    assert.equal(linkedUrl.origin, origin);
    assert.equal(linkedUrl.searchParams.get('before'), cursor);
    check('bounded history tail', { status: 200, count: messages.length, nextCursor: cursor, link, messages });
    const before = await json(messagePath + '&limit=2&before=' + encodeURIComponent(cursor));
    assert.equal(before.body.length, 1);
    assert.equal(before.body[0].parts[0].text, 'Fixture message 1');
    assert.equal(before.response.headers.get('x-next-cursor'), null);
    check('history before', { status: 200, count: 1, nextCursor: null });
    const single = await json(`/session/${session.id}/message/${messages[0].info.id}` + query);
    assert.deepEqual(single.body, messages[0]);
    check('single message', { status: 200 });
    stage = 'pending interactions and idle status';
    for (const [name, expected] of [['permission', []], ['question', []], ['session/status', {}]]) {
      const response = await json('/' + name + query);
      assert.deepEqual(response.body, expected);
      check(name, { status: 200, body: response.body });
    }
    for (const [path, body, tag] of [
      ['/permission/per_missing/reply', { reply: 'once' }, 'PermissionNotFoundError'],
      ['/question/que_missing/reply', { answers: [['yes']] }, 'QuestionNotFoundError'],
      ['/question/que_missing/reject', {}, 'QuestionNotFoundError'],
    ]) {
      const response = await json(path + query, post(body), 404);
      assert.equal(response.body._tag, tag);
      check('expired ' + path, { status: 404, tag });
    }
    stage = 'directory routing and idle abort';
    const scoped = await json('/session?directory=' + encodeURIComponent(otherDirectory) + '&limit=100');
    assert.deepEqual(scoped.body, []);
    const cross = await json(`/session/${session.id}?directory=` + encodeURIComponent(otherDirectory));
    assert.equal(cross.body.id, session.id);
    assert.equal(cross.body.directory, canonicalDirectory);
    check('directory is not authorization', { otherDirectoryListCount: 0, sessionReadWithOtherDirectory: 200, actualDirectory: cross.body.directory });
    const aborted = await json(`/session/${session.id}/abort` + query, { method: 'POST' });
    assert.equal(aborted.body, true);
    assert.deepEqual((await json('/global/health')).body, health.body);
    check('idle abort', { status: 200, body: true, healthAfter: 200 });
    stage = 'SSE frame contracts';
    await delay(10500, undefined, { signal: lifecycle.signal });
    stream.abort();
    await eventReader.cancel().catch(() => {});
    await eventTask;
    lifecycle.signal.throwIfAborted();
    assert.ok(!streamFailed, 'Unexpected SSE failure');
    // Cancellation can cut the last frame; only complete frames are meaningful.
    for (const frame of rawEvents.split(/\r?\n\r?\n/).slice(0, -1).filter(Boolean)) {
      const data = frame.split(/\r?\n/).filter(line => line.startsWith('data:')).map(line => line.slice(5).trimStart()).join('\n');
      if (data) results.events.push({ frameHasId: /^id:/m.test(frame), payload: JSON.parse(data) });
    }
    const types = [...new Set(results.events.map(event => event.payload.type))];
    for (const type of ['server.connected', 'server.heartbeat', 'session.created', 'session.updated', 'message.updated', 'message.part.updated', 'session.status', 'session.idle']) {
      assert.ok(types.includes(type), `Missing SSE event ${type}`);
    }
    assert.ok(results.events.every(event => !event.frameHasId));
    assert.ok(results.events.every(event => typeof event.payload.id === 'string' && typeof event.payload.properties === 'object'));
    const createdEvent = results.events.find(event => event.payload.type === 'session.created');
    assert.equal(createdEvent.payload.properties.info.directory, canonicalDirectory);
    assert.ok(results.events.some(event => event.payload.type === 'session.status' && event.payload.properties.status.type === 'idle'));
    check('SSE frames', { types, hasFrameId: false, payloadKeys: Object.keys(results.events[0].payload) });
    stage = 'fixture output secret scan';
    assert.ok(!logOverflow, 'Fixture log exceeded its limit');
    const captured = logs + rawEvents + JSON.stringify(results);
    assert.ok(!captured.includes(env.OPENCODE_SERVER_PASSWORD));
    assert.ok(!captured.includes(basic));
    check('fixture output secret scan', { plaintext: false, basicEncoded: false });
    results.passed = true;
  } catch {
    results.failedStage = stage;
    console.error(`OpenCode fixture failed during ${stage}; server content omitted.`);
    process.exitCode = 1;
  } finally {
    stream?.abort();
    lifecycle.abort();
    clearTimeout(streamTimer);
    if (!closed) {
      child.kill('SIGTERM');
      const escalation = setTimeout(() => { if (!closed) child.kill('SIGKILL'); }, 5000);
      try { await childClosed; } finally { clearTimeout(escalation); }
    }
    await eventReader?.cancel().catch(() => {});
    await eventTask;
    process.off('SIGINT', interrupt);
    process.off('SIGTERM', interrupt);
    results.exitCode = child.exitCode;
    results.signalCode = child.signalCode;
    await writeFile(join(root, 'results.json'), redact(JSON.stringify(results, null, 2)), { mode: 0o600 });
    await writeFile(join(root, 'server.log'), redact(logs), { mode: 0o600 });
    console.log(redact(JSON.stringify({ resultPath: join(root, 'results.json'), ownedPid: child.pid, passed: results.passed, exitCode: child.exitCode, signalCode: child.signalCode })));
  }
}
