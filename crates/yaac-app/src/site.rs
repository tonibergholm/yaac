//! A local test site for the harness. It listens on 127.0.0.1 only and serves pages that play the
//! part of a remote page: the content views load them from `http://127.0.0.1:<port>` (a different
//! origin from `app://ui`), and the pages report what they saw back here. Nothing leaves the
//! machine. Dev builds only.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Default)]
struct Record {
    /// `(name, result)` from `/report`, in arrival order.
    reports: Vec<(String, String)>,
    /// `(who, Cookie header)` from `/echo`, in arrival order.
    cookies: Vec<(String, String)>,
}

/// The running site. Clone-free: share it through `Arc`.
pub struct Site {
    pub port: u16,
    record: Arc<Mutex<Record>>,
}

impl Site {
    /// Starts the listener on an ephemeral port, on a background thread.
    pub fn start() -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let record = Arc::new(Mutex::new(Record::default()));
        let shared = record.clone();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let shared = shared.clone();
                thread::spawn(move || handle(stream, port, &shared));
            }
        });
        Ok(Self { port, record })
    }

    /// Reports received so far, as `(name, result)`.
    pub fn reports(&self) -> Vec<(String, String)> {
        self.record
            .lock()
            .map(|r| r.reports.clone())
            .unwrap_or_default()
    }

    /// The last value reported under `name`.
    pub fn report(&self, name: &str) -> Option<String> {
        self.reports()
            .into_iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
    }

    /// The Cookie header of the last `/echo?who=<who>` request.
    pub fn cookie_for(&self, who: &str) -> Option<String> {
        self.record
            .lock()
            .ok()?
            .cookies
            .iter()
            .rev()
            .find(|(w, _)| w == who)
            .map(|(_, c)| c.clone())
    }
}

fn handle(stream: TcpStream, port: u16, record: &Mutex<Record>) {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() {
        return;
    }
    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) if line == "\r\n" || line == "\n" => break,
            Ok(_) => {
                if let Some((name, value)) = line.split_once(':') {
                    headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
                }
            }
        }
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("/");
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let query = parse_query(query);
    let mut stream = reader.into_inner();

    let (status, extra, body) = route(method, path, &query, &headers, port, record);
    let head = format!(
        "HTTP/1.1 {status}\r\n{extra}Content-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&body);
    let _ = stream.flush();
    let mut sink = Vec::new();
    let _ = stream.read_to_end(&mut sink);
}

type Response = (&'static str, String, Vec<u8>);

fn route(
    method: &str,
    path: &str,
    query: &HashMap<String, String>,
    headers: &HashMap<String, String>,
    port: u16,
    record: &Mutex<Record>,
) -> Response {
    let _ = method;
    let _ = headers;
    match path {
        "/remote" => ("200 OK", String::new(), REMOTE.replace("__PORT__", &port.to_string()).into_bytes()),
        "/set-cookie" => (
            "200 OK",
            "Set-Cookie: yaac_probe=alpha; Path=/\r\n".into(),
            SET_COOKIE_PAGE.as_bytes().to_vec(),
        ),
        "/echo" => {
            let who = query.get("who").cloned().unwrap_or_default();
            let cookie = headers.get("cookie").cloned().unwrap_or_default();
            if let Ok(mut r) = record.lock() {
                r.cookies.push((who.clone(), cookie));
            }
            let page = ECHO_PAGE.replace("__WHO__", &who);
            ("200 OK", String::new(), page.into_bytes())
        }
        "/redirect" => (
            "302 Found",
            "Location: app://ui/\r\n".into(),
            b"redirect".to_vec(),
        ),
        "/meta-refresh" => (
            "200 OK",
            String::new(),
            b"<!doctype html><meta http-equiv=\"refresh\" content=\"0;url=app://ui/\"><title>meta</title>".to_vec(),
        ),
        "/report" => {
            let name = query.get("attempt").or_else(|| query.get("name")).cloned().unwrap_or_default();
            let result = query.get("result").cloned().unwrap_or_default();
            // One line per attempt, with no page content beyond the short result token.
            println!("E-IPC-1 attempt={name} result={result}");
            if let Ok(mut r) = record.lock() {
                r.reports.push((name, result));
            }
            ("204 No Content", String::new(), Vec::new())
        }
        _ => ("404 Not Found", String::new(), b"not found".to_vec()),
    }
}

fn parse_query(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (k.to_string(), v.replace("%20", " ")))
        .collect()
}

const SET_COOKIE_PAGE: &str = "<!doctype html><title>set</title><script>\
localStorage.setItem('yaac', 'alpha');\
fetch('/report?attempt=set-cookie&result=stored', {mode:'no-cors'});\
document.title = 'stored';</script>";

const ECHO_PAGE: &str = "<!doctype html><title>echo</title><script>\
fetch('/report?attempt=ls-__WHO__&result=' + encodeURIComponent(String(localStorage.getItem('yaac'))), {mode:'no-cors'});\
document.title = 'echo';</script>";

/// The remote page. Each probe reports one line through `/report`. The page does not depend on any
/// result: the harness checks the engine's counters and the stub's calls.
const REMOTE: &str = r#"<!doctype html><meta charset="utf-8"><title>remote</title>
<script>
const RPC = 'app://ui/api/v1/rpc', PAGE = 'app://ui/sidebar.html', BASE = 'http://127.0.0.1:__PORT__';
function report(name, result) {
  fetch('/report?attempt=' + encodeURIComponent(name) + '&result=' + encodeURIComponent(result), { mode: 'no-cors' }).catch(function () {});
}
function sleep(ms) { return new Promise(function (r) { setTimeout(r, ms); }); }
async function attempt(name, fn) {
  try { report(name, await fn()); } catch (e) { report(name, 'rejected:' + (e && e.name)); }
  await sleep(120);
}
function xhr(method, url, type, body) {
  return new Promise(function (resolve) {
    const x = new XMLHttpRequest();
    x.open(method, url, true);
    if (type) x.setRequestHeader('content-type', type);
    x.onload = function () { resolve('ok:' + x.status); };
    x.onerror = function () { resolve('error'); };
    x.send(body === undefined ? null : body);
  });
}
function frame(name, attrs) {
  return new Promise(function (resolve) {
    const f = document.createElement('iframe');
    f.name = name;
    Object.keys(attrs).forEach(function (k) { f.setAttribute(k, attrs[k]); });
    f.onload = function () { resolve('frame-load'); };
    document.body.appendChild(f);
    setTimeout(function () { resolve('frame-timeout'); }, 1500);
  });
}
const JSON_T = 'application/json';
(async function () {
  await attempt('fetch-get-rpc', async () => 'ok:' + (await fetch(RPC)).status);
  await attempt('fetch-post-text-rpc', async () => 'ok:' + (await fetch(RPC, { method: 'POST', headers: { 'content-type': 'text/plain' }, body: '{}' })).status);
  await attempt('fetch-post-json-rpc', async () => 'ok:' + (await fetch(RPC, { method: 'POST', headers: { 'content-type': JSON_T }, body: '{}' })).status);
  await attempt('fetch-post-nocors-rpc', async () => 'opaque:' + (await fetch(RPC, { method: 'POST', mode: 'no-cors', body: '{}' })).type);
  await attempt('fetch-credentials-rpc', async () => 'ok:' + (await fetch(RPC, { method: 'POST', credentials: 'include', headers: { 'content-type': JSON_T }, body: '{}' })).status);
  await attempt('fetch-sidebar-page', async () => 'ok:' + (await fetch(PAGE)).status);
  await attempt('xhr-post-text-rpc', () => xhr('POST', RPC, 'text/plain', '{}'));
  await attempt('xhr-post-json-rpc', () => xhr('POST', RPC, JSON_T, '{}'));
  await attempt('xhr-get-sidebar-page', () => xhr('GET', PAGE, null, null));
  await attempt('form-post-rpc', async () => {
    const f = document.createElement('form');
    f.method = 'post'; f.action = RPC; f.target = 'form-frame';
    const frm = document.createElement('iframe'); frm.name = 'form-frame'; document.body.appendChild(frm);
    document.body.appendChild(f); f.submit();
    await sleep(800); return 'submitted';
  });
  await attempt('anchor-click-app', async () => {
    const a = document.createElement('a'); a.href = 'app://ui/'; a.target = 'anchor-frame';
    const frm = document.createElement('iframe'); frm.name = 'anchor-frame'; document.body.appendChild(frm);
    document.body.appendChild(a); a.click(); await sleep(500); return 'clicked';
  });
  await attempt('window-open-app', async () => { const w = window.open('app://ui/'); return 'open:' + (w === null ? 'null' : 'window'); });
  await attempt('iframe-app-sidebar', () => frame('iframe-sidebar', { src: 'app://ui/sidebar.html' }));
  await attempt('srcdoc-fetch-rpc', () => frame('srcdoc-rpc', { srcdoc: "<script>fetch('app://ui/api/v1/rpc',{method:'POST',mode:'no-cors',body:'{}'})<\/script>" }));
  await attempt('data-iframe-fetch-rpc', () => frame('data-rpc', { src: "data:text/html,<script>fetch('app://ui/api/v1/rpc',{method:'POST',mode:'no-cors',body:'{}'})<\/script>" }));
  await attempt('javascript-iframe-rpc', () => frame('js-rpc', { src: "javascript:fetch('app://ui/api/v1/rpc',{method:'POST',mode:'no-cors',body:'{}'})" }));
  await attempt('blob-iframe-rpc', () => {
    const url = URL.createObjectURL(new Blob(["<script>fetch('app://ui/api/v1/rpc',{method:'POST',mode:'no-cors',body:'{}'})<\/script>"], { type: 'text/html' }));
    return frame('blob-rpc', { src: url });
  });
  await attempt('iframe-redirect-app', () => frame('redirect-frame', { src: '/redirect' }));
  await attempt('iframe-meta-refresh-app', () => frame('meta-frame', { src: '/meta-refresh' }));
  report('e-ipc-done', 'remote');
  await sleep(300);
  location.href = 'app://ui/';
})();
</script>
"#;
