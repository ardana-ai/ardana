// The two hosts of the standalone suite (`cargo xtask e2e standalone`), offline, each on a free port of 127.0.0.1 and
// so of an origin of its own:
// - the site, the standalone playground's `dist` as static files under `/playground/`, with the cross-origin isolation
//   headers the landing sends there (`static/_headers`);
// - a stand-in of Hugging Face serving the hub cache (`tmp/hf/hub`) as huggingface.co serves a repository's files,
//   `/<org>/<repo>/resolve/<commit>/<file>`: CORS open to any origin, its headers exposed, the whole file with its
//   length or the rest of it from `Range: bytes=<from>-` (206), never compressed.
// Usage: node standalone-hosts.mjs <dist> <hub cache>. Prints `{"site": <url>, "hub": <url>}` on one line once both
// listen, then serves until it is stopped.
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';

const [dist, cache] = process.argv.slice(2);
if (!dist || !cache) {
  console.error('usage: node standalone-hosts.mjs <dist> <hub cache>');
  process.exit(2);
}

const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript',
  '.mjs': 'text/javascript',
  '.css': 'text/css',
  '.wasm': 'application/wasm',
  '.json': 'application/json',
  '.svg': 'image/svg+xml',
  '.ico': 'image/x-icon',
  '.woff2': 'font/woff2',
  '.txt': 'text/plain; charset=utf-8',
};

const PREFIX = '/playground/';

/** The file `relative` names under `root`, when it is one there. */
function inside(root, relative) {
  const file = path.resolve(root, relative);
  const within = file.startsWith(path.resolve(root) + path.sep);
  return within && fs.statSync(file, { throwIfNoEntry: false })?.isFile() ? file : null;
}

const site = http.createServer((req, res) => {
  const { pathname } = new URL(req.url, 'http://site');
  if (pathname === PREFIX.slice(0, -1)) {
    res.writeHead(301, { location: PREFIX }).end();
    return;
  }
  const relative = pathname.startsWith(PREFIX) ? decodeURIComponent(pathname.slice(PREFIX.length)) : null;
  const file = relative === null ? null : inside(dist, relative === '' ? 'index.html' : relative);
  if (!file) {
    res.writeHead(404).end();
    return;
  }
  res.writeHead(200, {
    'content-type': TYPES[path.extname(file)] ?? 'application/octet-stream',
    'content-length': fs.statSync(file).size,
    'cross-origin-opener-policy': 'same-origin',
    'cross-origin-embedder-policy': 'require-corp',
  });
  fs.createReadStream(file).pipe(res);
});

const hub = http.createServer((req, res) => {
  const cors = { 'access-control-allow-origin': '*', 'access-control-expose-headers': '*' };
  const parts = new URL(req.url, 'http://hub').pathname.split('/').map(decodeURIComponent);
  // ['', org, repo, 'resolve', commit, ...file]
  const [, org, repo, resolve, commit, ...name] = parts;
  const file =
    resolve === 'resolve' && name.length > 0
      ? inside(path.join(cache, `models--${org}--${repo}`, 'snapshots', commit), name.join('/'))
      : null;
  if (!file || req.method !== 'GET') {
    res.writeHead(404, cors).end();
    return;
  }
  const size = fs.statSync(file).size;
  const from = /^bytes=(\d+)-$/.exec(req.headers.range ?? '')?.[1];
  if (from !== undefined && Number(from) < size) {
    const start = Number(from);
    res.writeHead(206, {
      ...cors,
      'accept-ranges': 'bytes',
      'content-length': size - start,
      'content-range': `bytes ${start}-${size - 1}/${size}`,
    });
    fs.createReadStream(file, { start }).pipe(res);
    return;
  }
  res.writeHead(200, { ...cors, 'accept-ranges': 'bytes', 'content-length': size });
  fs.createReadStream(file).pipe(res);
});

const listen = (server) =>
  new Promise((listening) => server.listen(0, '127.0.0.1', () => listening(`http://127.0.0.1:${server.address().port}`)));
const [siteUrl, hubUrl] = await Promise.all([listen(site), listen(hub)]);
console.log(JSON.stringify({ site: siteUrl, hub: hubUrl }));
